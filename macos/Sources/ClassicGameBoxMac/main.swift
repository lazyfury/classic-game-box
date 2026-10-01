// Experimental Swift/macOS host for Classic Game Box.
//
// Swift owns the window and its CAMetalLayer, and forwards native events; the
// igui UI, the wgpu renderer and the emulator are all Rust (`cgb-mac`), linked
// through the `cgb_mac_*` C ABI. Swift replaces `winit` and nothing else.

import AppKit

let options = LaunchOptions.parse(CommandLine.arguments)

let application = NSApplication.shared
application.setActivationPolicy(.regular)
let delegate = AppDelegate(options: options)
application.delegate = delegate
application.run()
