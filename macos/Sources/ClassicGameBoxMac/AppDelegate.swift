// AppKit lifecycle: one window whose content view provides the CAMetalLayer
// the Rust app renders into.
//
// Frames are event-driven: any input schedules one, and while the app wants
// more (a running game, an animation) a `CADisplayLink` drives them at the
// display's refresh. Resizes are coalesced to one surface reconfigure per
// frame.
//
// Swift's only jobs are the window, the layer and native event forwarding —
// the UI and the emulator are Rust's.

import AppKit
import CGBNative
import Metal
import QuartzCore

final class AppDelegate: NSObject, NSApplicationDelegate {
    private let options: LaunchOptions
    private let gamepads = Gamepads()
    private var window: NSWindow?
    private var view: HostView?
    private var app: OpaquePointer?
    /// A pending one-shot frame (idle input), or nil.
    private var timer: Timer?
    /// Drives frames while the app wants them (macOS 14+).
    private var displayLink: CADisplayLink?
    /// The local event monitor that schedules a frame for any input.
    private var eventMonitor: Any?
    /// The geometry the surface was last configured for, so a live resize
    /// coalesces to one reconfigure per frame.
    private var lastPixelSize: (UInt32, UInt32)?
    private var lastScale: Double?

    init(options: LaunchOptions) {
        self.options = options
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        installMenu()

        let contentRect = NSRect(x: 0, y: 0, width: 1100, height: 760)
        let view = HostView(frame: contentRect)
        self.view = view

        // A transparent, full-size title bar: the UI runs under it and leaves
        // room for the traffic lights (`safe_area` in `cgb-app`).
        let window = NSWindow(
            contentRect: contentRect,
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        window.title = "Classic Game Box"
        window.titlebarAppearsTransparent = true
        window.titleVisibility = .hidden
        // The UI needs room to lay out; below this it would clip.
        window.contentMinSize = NSSize(width: 960, height: 600)
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
        view.onGeometryChange = { [weak self] in self?.requestFrame() }
        gamepads.app = handle
        gamepads.start()

        if #available(macOS 14.0, *) {
            let link = view.displayLink(target: self, selector: #selector(displayTick))
            link.add(to: .main, forMode: .common)
            link.isPaused = true
            displayLink = link
        }
        // Every input event passes through this monitor; it schedules a frame,
        // and `runFrame` keeps the display link running while the app wants
        // more frames (a running game, an animation, a download).
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
        displayLink?.invalidate()
        displayLink = nil
        timer?.invalidate()
        timer = nil
        // The wgpu surface borrows the CAMetalLayer, so the Rust app must be
        // torn down while `view` (and its layer) is still alive. `view` is
        // released only after this method returns.
        if let app {
            cgb_host_destroy(app)
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
                    cgb_host_start(layer, width, height, scale, lib, rom)
                }
            }
        case let (library?, nil):
            return library.withCString { lib in
                cgb_host_start(layer, width, height, scale, lib, nil)
            }
        case let (nil, rom?):
            return rom.withCString { rom in
                cgb_host_start(layer, width, height, scale, nil, rom)
            }
        case (nil, nil):
            return cgb_host_start(layer, width, height, scale, nil, nil)
        }
    }

    @objc private func displayTick() {
        runFrame()
    }

    private func timerTick() {
        timer = nil
        runFrame()
    }

    private func runFrame() {
        guard let app else { return }
        applyResizeIfNeeded()
        cgb_host_frame(app)
        view?.syncFrameState()
        syncFullscreen()
        setContinuous(cgb_host_needs_frame(app))
    }

    /// Ask for a frame as soon as the run loop is free (after the event that
    /// prompted it is dispatched). While the display link is running it will
    /// present at the next refresh anyway.
    private func requestFrame() {
        if let displayLink, !displayLink.isPaused {
            return
        }
        schedule(after: 0)
    }

    private func schedule(after delay: TimeInterval) {
        guard timer == nil else { return }
        let timer = Timer(timeInterval: delay, repeats: false) { [weak self] _ in
            self?.timerTick()
        }
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
    }

    /// Start or stop the continuous driver (the display link).
    private func setContinuous(_ on: Bool) {
        displayLink?.isPaused = !on
    }

    /// Reconfigure the surface for the current geometry, at most once a frame.
    private func applyResizeIfNeeded() {
        guard let app, let view else { return }
        let size = view.pixelSize
        let scale = view.scaleFactor
        if lastPixelSize?.0 != size.0 || lastPixelSize?.1 != size.1 || lastScale != scale {
            lastPixelSize = size
            lastScale = scale
            cgb_host_resize(app, size.0, size.1, scale)
        }
    }

    /// Apply a fullscreen request the Rust app parked for the window.
    private func syncFullscreen() {
        guard let app, let window else { return }
        switch cgb_host_take_fullscreen(app) {
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
