// AppKit lifecycle: one window whose content view provides the CAMetalLayer
// the Rust app renders into. Frames are event-driven: any input schedules one,
// and while the app wants more (a running game, an animation) it reschedules
// itself at 60 Hz.
//
// Swift's only jobs are the window, the layer and native event forwarding —
// the UI and the emulator are Rust's.

import AppKit
import Metal
import CGBNative

final class AppDelegate: NSObject, NSApplicationDelegate {
    private let options: LaunchOptions
    private let gamepads = Gamepads()
    private var window: NSWindow?
    private var view: HostView?
    private var app: OpaquePointer?
    /// The pending one-shot frame timer, or nil when idle.
    private var timer: Timer?
    /// The local event monitor that schedules a frame for any input.
    private var eventMonitor: Any?

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
            let alert = NSAlert()
            alert.messageText = "启动失败"
            alert.informativeText = "无法创建 Metal / wgpu 渲染后端。"
            alert.alertStyle = .critical
            alert.runModal()
            NSApp.terminate(nil)
            return
        }
        self.app = handle
        view.app = handle
        view.onGeometryChange = { [weak self] in self?.resize() }
        gamepads.app = handle
        gamepads.start()

        // Frames are event-driven. The local monitor is the one place every
        // input event passes through; it schedules a frame, and `tick` keeps
        // rescheduling while the app still wants frames (a running game, an
        // animation, a download).
        eventMonitor = NSEvent.addLocalMonitorForEvents(matching: .any) { [weak self] event in
            self?.requestFrame()
            return event
        }
        requestFrame()

        NSApp.activate(ignoringOtherApps: true)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }

    func applicationWillTerminate(_ notification: Notification) {
        if let eventMonitor {
            NSEvent.removeMonitor(eventMonitor)
            self.eventMonitor = nil
        }
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
        timer = nil
        guard let app else { return }
        cgb_mac_frame(app)
        view?.syncFrameState()
        syncFullscreen()
        if cgb_mac_needs_frame(app) {
            schedule(after: 1.0 / 60.0)
        }
    }

    /// Ask for a frame as soon as the run loop is free (after the event that
    /// prompted it is dispatched).
    private func requestFrame() {
        schedule(after: 0)
    }

    private func schedule(after delay: TimeInterval) {
        guard timer == nil else { return }
        let timer = Timer(timeInterval: delay, repeats: false) { [weak self] _ in
            self?.tick()
        }
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
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
