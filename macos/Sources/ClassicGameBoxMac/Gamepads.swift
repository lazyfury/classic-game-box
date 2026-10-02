// Swift-native gamepad input, using Apple's GameController framework.
//
// This replaces `gilrs` on the embedded host: Swift owns the `GCController`s,
// maps them to libretro joypad ids and pushes a per-port snapshot into Rust.
// Apple's mapping is correct for the Xbox Wireless Controller over Bluetooth,
// which `gilrs` mislabels here.

import CGBNative
import Foundation
import GameController

final class Gamepads {
    /// The Rust app, set once it has started.
    var app: OpaquePointer?

    /// Controller → libretro port (0 or 1), in connection order.
    private var ports: [ObjectIdentifier: Int] = [:]

    func start() {
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(controllerConnected(_:)),
            name: .GCControllerDidConnect,
            object: nil
        )
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(controllerDisconnected(_:)),
            name: .GCControllerDidDisconnect,
            object: nil
        )
        GCController.startWirelessControllerDiscovery {}
        for controller in GCController.controllers() {
            attach(controller)
        }
    }

    @objc private func controllerConnected(_ notification: Notification) {
        guard let controller = notification.object as? GCController else { return }
        attach(controller)
    }

    @objc private func controllerDisconnected(_ notification: Notification) {
        guard let controller = notification.object as? GCController else { return }
        guard let port = ports.removeValue(forKey: ObjectIdentifier(controller)) else { return }
        if let app {
            cgb_host_gamepad_connected(app, UInt32(port), false)
        }
    }

    private func attach(_ controller: GCController) {
        guard ports[ObjectIdentifier(controller)] == nil else { return }
        let used = Set(ports.values)
        guard let port = (0..<2).first(where: { !used.contains($0) }) else { return }
        ports[ObjectIdentifier(controller)] = port
        controller.playerIndex = port == 0 ? .index1 : .index2

        if let app {
            cgb_host_gamepad_connected(app, UInt32(port), true)
        }
        guard let pad = controller.extendedGamepad else { return }
        pad.valueChangedHandler = { [weak self] pad, _ in
            self?.push(pad, port: port)
        }
        push(pad, port: port)
    }

    /// Build the libretro bitmask + axes and hand them to Rust.
    private func push(_ pad: GCExtendedGamepad, port: Int) {
        guard let app else { return }
        var buttons: UInt32 = 0
        func set(_ id: Int, _ pressed: Bool) {
            if pressed { buttons |= UInt32(1) << UInt32(id) }
        }

        set(CGB_JOYPAD_B, pad.buttonA.isPressed) // bottom / confirm
        set(CGB_JOYPAD_A, pad.buttonB.isPressed) // right
        set(CGB_JOYPAD_Y, pad.buttonX.isPressed) // left
        set(CGB_JOYPAD_X, pad.buttonY.isPressed) // top
        set(CGB_JOYPAD_L, pad.leftShoulder.isPressed)
        set(CGB_JOYPAD_R, pad.rightShoulder.isPressed)
        set(CGB_JOYPAD_L2, pad.leftTrigger.isPressed)
        set(CGB_JOYPAD_R2, pad.rightTrigger.isPressed)
        set(CGB_JOYPAD_L3, pad.leftThumbstickButton?.isPressed ?? false)
        set(CGB_JOYPAD_R3, pad.rightThumbstickButton?.isPressed ?? false)
        set(CGB_JOYPAD_UP, pad.dpad.up.isPressed)
        set(CGB_JOYPAD_DOWN, pad.dpad.down.isPressed)
        set(CGB_JOYPAD_LEFT, pad.dpad.left.isPressed)
        set(CGB_JOYPAD_RIGHT, pad.dpad.right.isPressed)
        set(CGB_JOYPAD_START, pad.buttonMenu.isPressed)
        set(CGB_JOYPAD_SELECT, pad.buttonOptions?.isPressed ?? false)

        // GameController's Y is positive up; libretro wants positive down, so
        // the Y axes are negated on the way in.
        let leftX = pad.leftThumbstick.xAxis.value
        let leftY = pad.leftThumbstick.yAxis.value
        let rightX = pad.rightThumbstick.xAxis.value
        let rightY = pad.rightThumbstick.yAxis.value

        // Like the gilrs path: past the deadzone the stick also sets the
        // matching D-pad bit, so a core that reads only buttons still moves.
        if leftX < -0.5 { set(CGB_JOYPAD_LEFT, true) }
        if leftX > 0.5 { set(CGB_JOYPAD_RIGHT, true) }
        if leftY > 0.5 { set(CGB_JOYPAD_UP, true) }
        if leftY < -0.5 { set(CGB_JOYPAD_DOWN, true) }

        cgb_host_gamepad_state(
            app,
            UInt32(port),
            buttons,
            Self.axis(leftX),
            Self.axis(-leftY),
            Self.axis(rightX),
            Self.axis(-rightY)
        )
    }

    /// A `-1..1` axis as libretro's `-32768..32767`.
    private static func axis(_ value: Float) -> Int16 {
        let clamped = max(-1, min(1, value))
        return Int16((clamped * 32767).rounded())
    }
}
