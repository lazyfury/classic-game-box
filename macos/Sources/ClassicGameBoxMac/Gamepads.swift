// Swift-native gamepad input, using Apple's GameController framework.
//
// Swift owns the `GCController`s and reports each one's raw state by a **device
// slot** (connection order) through the C ABI. The app maps slots to libretro
// ports, so the assignment UI works the same as the `gilrs` path — and Apple's
// mapping is correct for the Xbox Wireless Controller over Bluetooth, where
// `gilrs` mislabels it.

import CGBNative
import Foundation
import GameController

final class Gamepads {
    /// The Rust app, set once it has started.
    var app: OpaquePointer?

    /// Controller → device slot, in connection order.
    private var slots: [ObjectIdentifier: Int] = [:]
    /// Slot → the controller behind it, so a synthetic disconnect can name it.
    private var controllers: [Int: GCController] = [:]

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
        reconcile()
    }

    @objc private func controllerConnected(_ notification: Notification) {
        // Reconcile *before* attaching: a Bluetooth blip can drop a
        // `DidDisconnect`, and without this a reconnect would leave a phantom
        // slot holding the old port (a stuck direction) while the live pad is
        // moved to another.
        reconcile()
    }

    @objc private func controllerDisconnected(_ notification: Notification) {
        guard let controller = notification.object as? GCController else { return }
        detach(controller)
    }

    /// Make the slot table match `GCController.controllers()`: drop any slot
    /// whose controller is gone (a missed `DidDisconnect`), then (re)attach
    /// every live controller. Safe to call as often as the input loop likes; it
    /// never leaves a "connected" slot behind and it refreshes each pad's state,
    /// which covers a push handler that stopped firing.
    func reconcile() {
        let live = Set(GCController.controllers().map { ObjectIdentifier($0) })
        // Collect first: mutating `slots` while iterating it is not allowed.
        for (id, slot) in slots.filter({ !live.contains($0.key) }) {
            slots.removeValue(forKey: id)
            let gone = controllers.removeValue(forKey: slot)
            if let app {
                let name = gone.map(Self.name) ?? "Controller"
                name.withCString { cgb_host_gamepad_device(app, UInt32(slot), $0, false) }
            }
        }
        for controller in GCController.controllers() {
            attach(controller)
        }
    }

    private func detach(_ controller: GCController) {
        guard let slot = slots.removeValue(forKey: ObjectIdentifier(controller)) else { return }
        controllers.removeValue(forKey: slot)
        if let app {
            Self.name(controller).withCString {
                cgb_host_gamepad_device(app, UInt32(slot), $0, false)
            }
        }
    }

    private func attach(_ controller: GCController) {
        // Only declare a device once it reports state: a controller whose profile
        // is not ready yet would otherwise take a port and sit frozen (and its
        // disconnect might never be seen). `reconcile()` retries.
        guard let pad = controller.extendedGamepad else { return }
        let id = ObjectIdentifier(controller)
        let slot: Int
        if let known = slots[id] {
            slot = known
        } else {
            let used = Set(slots.values)
            guard let free = (0..<16).first(where: { !used.contains($0) }) else { return }
            slot = free
            slots[id] = free
            if let app {
                Self.name(controller).withCString {
                    cgb_host_gamepad_device(app, UInt32(slot), $0, true)
                }
            }
        }
        controllers[slot] = controller
        // Re-arm and re-push even for an already-known pad, so a handler that
        // lapsed is restored and the current state is sent again.
        pad.valueChangedHandler = { [weak self] pad, _ in
            self?.push(pad, slot: slot)
        }
        push(pad, slot: slot)
    }

    /// A display name for the device list.
    private static func name(_ controller: GCController) -> String {
        controller.vendorName ?? controller.productCategory
    }

    /// Build the libretro bitmask + axes and hand them to Rust.
    private func push(_ pad: GCExtendedGamepad, slot: Int) {
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
            UInt32(slot),
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
