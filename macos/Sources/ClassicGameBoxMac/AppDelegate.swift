// AppKit lifecycle: one window whose content view provides the CAMetalLayer
// the Rust app renders into, plus a 60 Hz tick that asks Rust for frames.
//
// Swift's only jobs are the window, the layer and (next) native event
// forwarding — the UI and the emulator are Rust's.

import AppKit
import Metal
import CGBNative

final class AppDelegate: NSObject, NSApplicationDelegate {
    private let options: LaunchOptions
    private let gamepads = Gamepads()
    private var window: NSWindow?
    private var view: HostView?
    private var app: OpaquePointer?
    private var timer: Timer?

    init(options: LaunchOptions) {
        self.options = options
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        installMenu()

        let contentRect = NSRect(x: 0, y: 0, width: 1100, height: 760)
        let view = HostView(frame: contentRect)
        self.view = view

        let window = NSWindow(
            contentRect: contentRect,
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.title = "Classic Game Box (Swift host)"
        window.contentView = view
        window.acceptsMouseMovedEvents = true
        window.center()
        window.makeKeyAndOrderFront(nil)
        window.makeFirstResponder(view)
        self.window = window

        guard let handle = startRustApp(in: view) else {
            NSApp.terminate(nil)
            return
        }
        self.app = handle
        view.app = handle
        view.onGeometryChange = { [weak self] in self?.resize() }
        gamepads.app = handle
        gamepads.start()

        // A main-thread timer drives the frames. `cgb_mac_needs_frame` can
        // later let an idle app skip work; for now every tick presents.
        let timer = Timer(timeInterval: 1.0 / 60.0, repeats: true) { [weak self] _ in
            self?.tick()
        }
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer

        NSApp.activate(ignoringOtherApps: true)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }

    func applicationWillTerminate(_ notification: Notification) {
        timer?.invalidate()
        timer = nil
        if let app {
            cgb_mac_destroy(app)
            self.app = nil
        }
    }

    private func startRustApp(in view: HostView) -> OpaquePointer? {
        let layer = Unmanaged.passUnretained(view.metalLayer).toOpaque()
        let (width, height) = view.pixelSize
        let scale = view.scaleFactor
        switch (options.libraryDir, options.rom) {
        case let (library?, rom?):
            return library.withCString { lib in
                rom.withCString { rom in
                    cgb_mac_start(layer, width, height, scale, lib, rom)
                }
            }
        case let (library?, nil):
            return library.withCString { lib in
                cgb_mac_start(layer, width, height, scale, lib, nil)
            }
        case let (nil, rom?):
            return rom.withCString { rom in
                cgb_mac_start(layer, width, height, scale, nil, rom)
            }
        case (nil, nil):
            return cgb_mac_start(layer, width, height, scale, nil, nil)
        }
    }

    private func resize() {
        guard let app, let view else { return }
        let (width, height) = view.pixelSize
        cgb_mac_resize(app, width, height, view.scaleFactor)
    }

    private func tick() {
        guard let app else { return }
        cgb_mac_frame(app)
        view?.syncFrameState()
        syncFullscreen()
    }

    /// Apply a fullscreen request the Rust app parked for the window.
    private func syncFullscreen() {
        guard let app, let window else { return }
        switch cgb_mac_take_fullscreen(app) {
        case 1 where !window.styleMask.contains(.fullScreen):
            window.toggleFullScreen(nil)
        case 0 where window.styleMask.contains(.fullScreen):
            window.toggleFullScreen(nil)
        default:
            break
        }
    }

    private func installMenu() {
        let mainMenu = NSMenu()
        let appItem = NSMenuItem()
        mainMenu.addItem(appItem)
        let appMenu = NSMenu()
        appMenu.addItem(
            withTitle: "Quit Classic Game Box",
            action: #selector(NSApplication.terminate(_:)),
            keyEquivalent: "q"
        )
        appItem.submenu = appMenu
        NSApp.mainMenu = mainMenu
    }
}
