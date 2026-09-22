// ---------------------------------------------------------------------------
// fc-gamepad -- the console's controller ports, read from real pads.
//
// One job: turn whatever the players are holding into the eight switches of an
// NES controller each, and say so on stdout. It knows nothing about the
// emulator, the Electron process, or what a button press does; it only reports.
//
// The output is JSON Lines -- one object per line, flushed immediately, so a
// reader on the other end can parse it with a loop and a `JSON.parse`. The two
// messages are:
//
//   {"type":"hello","version":2,"pid":1234}
//       Once, at startup. Its arrival is the only proof the helper is alive,
//       which is worth having when a pad "does not work".
//
//   {"type":"pads","pads":[
//        {"index":0,"id":"Xbox Wireless Controller","buttons":{
//             "A":true,"B":false,"SELECT":false,"START":false,
//             "UP":false,"DOWN":false,"LEFT":false,"RIGHT":false}},
//        {"index":1,"id":"DualSense Wireless Controller","buttons":{...}}]}
//       On connect, and then only when something changes. Polling happens at
//       60Hz, but a steady stream of "nothing changed" would be noise on a
//       pipe that the other end has to parse, so it is filtered here. The
//       whole list is sent, not one pad at a time, because the list is what
//       the renderer draws and what a disconnection is measured against.
//
// `index` is a slot, not a device: it is what the settings screen offers as
// "player 1" or "player 2", and it is the only name a pad has, because two
// identical controllers report the same `id`. A pad keeps its slot while it
// stays connected, and the lowest free slot is reused after it goes.
//
// The mapping is the console's, not the framework's, and it is deliberately
// the same eight switches the renderer's browser GamepadSource produces, so a
// pad behaves the same on either path. A stick duplicates the d-pad because
// most people reach for the stick; the deadzone is 0.5 because a worn stick
// drifts and a drifting stick walks the player into a wall. A is the right hand
// face button, which is index 0 on a modern pad -- the one with the letter A
// printed on it.
//
// stdin is a signal, not input: when the parent closes it, this exits. That is
// what stops a killed Electron from leaving an orphan process behind.
// ---------------------------------------------------------------------------

import Foundation
import GameController

/// The eight names, and the order is the order of the protocol. It is also the
/// order packages/fc-core/src/ffi/emulator_api.h's enum uses, so the whole chain
/// -- helper, main process, renderer, core -- agrees on which switch is which.
/// Everything the helper reports: the console's eight, and the ones the
/// console has no switch for -- the shoulders, the triggers, the stick clicks,
/// the two extra face buttons and the home button.
///
/// The second group exists so that a *command* (pause, screenshot, save) can be
/// bound to a button that does not also press something in the game. They are
/// reported all the same, on every line, because a reading is a fixed shape and
/// a missing key is a key the renderer would have to defend against.
private let buttonNames = [
    "A", "B", "SELECT", "START", "UP", "DOWN", "LEFT", "RIGHT",
    "L1", "R1", "L2", "R2", "L3", "R3",
    "FACE_X", "FACE_Y", "GUIDE",
]

/// Everything up, which is also the starting state and the "pad has gone" state.
private func allReleased() -> [String: Bool] {
    var buttons = [String: Bool]()
    for name in buttonNames {
        buttons[name] = false
    }
    return buttons
}

/// One JSON object, one line, on stdout, now.
///
/// FileHandle rather than `print` because stdout is a pipe here, and a pipe is
/// buffered: a `print` that sits in a 4KB buffer until the buffer fills is a
/// button press that arrives seconds late, or never if the process is idle.
private func emit(_ object: [String: Any]) {
    guard JSONSerialization.isValidJSONObject(object),
          let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])
    else {
        return
    }
    var line = data
    line.append(0x0A)
    FileHandle.standardOutput.write(line)
}

/// What one pad is doing right now, in the console's terms.
///
/// A `struct` and not just a dictionary so that "has anything changed" is a
/// value comparison rather than a hand-written field-by-field check that
/// somebody will eventually forget to extend.
private struct PadReading: Equatable {
    var index: Int
    var identifier: String
    var buttons: [String: Bool]
}

/// Reads every pad it can find, and reports the changes.
///
/// Every pad, not just the first: the console has two controller ports and two
/// players may each want a controller. The renderer decides which slot drives
/// which port; this only says what is there.
private final class PadReporter {
    private var last: [PadReading] = []
    /// Which slot each controller was given. Keyed by object identity because
    /// two identical pads have the same name and nothing else to tell them
    /// apart.
    private var slots: [ObjectIdentifier: Int] = [:]
    private var discoveryStarted = false

    func poll() {
        // Wireless discovery is what makes a pad that is switched on *after*
        // the application started show up. It is started once, lazily, rather
        // than at process start: with nothing to find there is no reason to
        // have the framework's radio scanning. Asking it repeatedly would
        // restart the scan every frame.
        if !discoveryStarted {
            discoveryStarted = true
            GCController.startWirelessControllerDiscovery(completionHandler: nil)
        }

        // Already-connected pads and newly-connected ones both appear here;
        // the framework keeps the list current.
        var present = Set<ObjectIdentifier>()
        var readings: [PadReading] = []

        for controller in GCController.controllers() {
            guard let buttons = read(controller) else {
                // A steering wheel or a flight stick has no mapping onto an
                // NES controller. Ignoring it is the honest answer.
                continue
            }
            let key = ObjectIdentifier(controller)
            present.insert(key)
            readings.append(PadReading(
                index: slot(for: key),
                identifier: describe(controller),
                buttons: buttons
            ))
        }

        // Slots are released when a pad goes, so the number can be reused and
        // the list does not grow a hole per unplugging.
        for key in Array(slots.keys) where !present.contains(key) {
            slots.removeValue(forKey: key)
        }

        readings.sort { $0.index < $1.index }
        guard readings != last else {
            return
        }
        last = readings
        emit([
            "type": "pads",
            "pads": readings.map { reading in
                [
                    "index": reading.index,
                    "id": reading.identifier,
                    "buttons": reading.buttons,
                ]
            },
        ])
    }

    /// The lowest free slot, kept for as long as the pad stays connected.
    private func slot(for key: ObjectIdentifier) -> Int {
        if let existing = slots[key] {
            return existing
        }
        let used = Set(slots.values)
        var candidate = 0
        while used.contains(candidate) {
            candidate += 1
        }
        slots[key] = candidate
        return candidate
    }

    /// What to call this pad in the log and on the settings screen.
    ///
    /// `vendorName` is "Xbox Wireless Controller"; `productCategory` is the
    /// class the framework put it in, such as "Xbox One" or "DualShock". Either
    /// can be empty, and a pad with no name at all is still a pad.
    private func describe(_ controller: GCController) -> String {
        var parts: [String] = []
        if let vendor = controller.vendorName, !vendor.isEmpty {
            parts.append(vendor)
        }
        if !controller.productCategory.isEmpty {
            parts.append(controller.productCategory)
        }
        return parts.isEmpty ? "Gamepad" : parts.joined(separator: " ")
    }

    /// The eight switches, from whichever shape the framework handed over.
    ///
    /// A modern pad is `GCExtendedGamepad`: a d-pad, two sticks, four face
    /// buttons, shoulders and a menu. The Siri Remote and other small pads are
    /// `GCMicroGamepad`: a d-pad, two buttons and a menu. Anything else --
    /// a steering wheel, a flight stick -- has no mapping onto an NES
    /// controller at all and is ignored rather than guessed at.
    private func read(_ controller: GCController) -> [String: Bool]? {
        if let pad = controller.extendedGamepad {
            // Most people reach for the stick even when there is a d-pad, so
            // the stick duplicates it. The deadzone is what keeps a worn stick
            // at rest from walking the player into a wall.
            let deadzone: Float = 0.5
            let x = pad.leftThumbstick.xAxis.value
            // Axis values are +1 up and -1 down, unlike the browser's Gamepad
            // API where +1 is down. The sign flip is here and nowhere else.
            let y = pad.leftThumbstick.yAxis.value

            var buttons = allReleased()
            buttons["LEFT"] = pad.dpad.left.isPressed || x < -deadzone
            buttons["RIGHT"] = pad.dpad.right.isPressed || x > deadzone
            buttons["UP"] = pad.dpad.up.isPressed || y > deadzone
            buttons["DOWN"] = pad.dpad.down.isPressed || y < -deadzone

            buttons["A"] = pad.buttonA.isPressed
            buttons["B"] = pad.buttonB.isPressed
            buttons["START"] = pad.buttonMenu.isPressed
            buttons["SELECT"] = pad.buttonOptions?.isPressed ?? false

            // The ones the game cannot see. A trigger is analog; half way down
            // is the honest point to call it pressed, because the alternative
            // is a command that fires on a brush against the shoulder.
            buttons["FACE_X"] = pad.buttonX.isPressed
            buttons["FACE_Y"] = pad.buttonY.isPressed
            buttons["L1"] = pad.leftShoulder.isPressed
            buttons["R1"] = pad.rightShoulder.isPressed
            buttons["L2"] = pad.leftTrigger.isPressed || pad.leftTrigger.value > 0.5
            buttons["R2"] = pad.rightTrigger.isPressed || pad.rightTrigger.value > 0.5
            buttons["L3"] = pad.leftThumbstickButton?.isPressed ?? false
            buttons["R3"] = pad.rightThumbstickButton?.isPressed ?? false
            // macOS keeps the home button for itself on every pad anybody has,
            // so this reads as up forever. It is reported anyway rather than
            // left out: a reading with a hole in it is a reading the renderer
            // has to branch on.
            buttons["GUIDE"] = pad.buttonHome?.isPressed ?? false
            return buttons
        }

        if let pad = controller.microGamepad {
            var buttons = allReleased()
            buttons["LEFT"] = pad.dpad.left.isPressed
            buttons["RIGHT"] = pad.dpad.right.isPressed
            buttons["UP"] = pad.dpad.up.isPressed
            buttons["DOWN"] = pad.dpad.down.isPressed
            buttons["A"] = pad.buttonA.isPressed
            buttons["B"] = pad.buttonX.isPressed
            buttons["START"] = pad.buttonMenu.isPressed
            // The micro pad has no Select. Left up is the honest answer.
            return buttons
        }

        return nil
    }
}

// ---------------------------------------------------------------------------
// Life
// ---------------------------------------------------------------------------

emit(["type": "hello", "version": 2, "pid": ProcessInfo.processInfo.processIdentifier])

// stdin is a watchdog. The parent keeps it open for as long as it wants the
// helper to live; when it exits, the pipe closes and this read returns nil.
// Breaking out of the loop is the clean exit, and it is worth having because
// the alternative -- an orphan process still holding the HID connection after
// the application is gone -- is exactly the kind of thing this whole file was
// written to avoid.
DispatchQueue.global(qos: .utility).async {
    while readLine(strippingNewline: true) != nil {}
    exit(0)
}

private let reporter = PadReporter()
let timer = Timer(timeInterval: 1.0 / 60.0, repeats: true) { _ in
    reporter.poll()
}
// `.common` rather than the default mode: the default is suspended while a
// menu or a window drag is being tracked, and a pad that stops reporting
// mid-drag is a pad that looks broken.
RunLoop.main.add(timer, forMode: .common)
RunLoop.main.run()
