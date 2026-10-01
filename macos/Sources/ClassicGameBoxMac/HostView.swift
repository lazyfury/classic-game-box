// The window's content view: a plain NSView whose backing layer is the
// `CAMetalLayer` Rust renders into, plus the native event forwarding that
// replaces winit's input plugins.
//
// Swift only translates AppKit events into the C ABI calls; the meaning of a
// click or a key is the UI's business, in Rust.

import AppKit
import CGBNative
import Metal
import QuartzCore

final class HostView: NSView {
    /// The Rust app, set by `AppDelegate` once it has started.
    var app: OpaquePointer?

    /// Handed to Rust as the wgpu surface target.
    let metalLayer = CAMetalLayer()

    /// Called whenever the drawable size or backing scale changed, so the app
    /// can reconfigure its surface.
    var onGeometryChange: (() -> Void)?

    /// The cursor last applied, so a steady pointer does not re-set it.
    private var lastCursorCode: UInt32 = 0
    /// The focused text caret (logical, origin top-left), for the IME.
    private var caretRect: NSRect?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        metalLayer.device = MTLCreateSystemDefaultDevice()
        metalLayer.pixelFormat = .bgra8Unorm
        metalLayer.framebufferOnly = true
        layer = metalLayer
        registerForDraggedTypes([.fileURL])
        updateLayerGeometry()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("HostView is created programmatically")
    }

    override var acceptsFirstResponder: Bool { true }

    override func layout() {
        super.layout()
        updateLayerGeometry()
    }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        updateLayerGeometry()
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        for area in trackingAreas {
            removeTrackingArea(area)
        }
        addTrackingArea(
            NSTrackingArea(
                rect: .zero,
                options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect],
                owner: self,
                userInfo: nil
            )
        )
    }

    /// The drawable size in physical pixels.
    var pixelSize: (width: UInt32, height: UInt32) {
        let size = metalLayer.drawableSize
        return (
            UInt32(max(size.width, 1)),
            UInt32(max(size.height, 1))
        )
    }

    /// The backing scale (2.0 on Retina).
    var scaleFactor: Double {
        let scale = metalLayer.contentsScale
        return scale > 0 ? Double(scale) : 1.0
    }

    private func updateLayerGeometry() {
        let scale = window?.backingScaleFactor ?? 1.0
        metalLayer.contentsScale = scale
        metalLayer.frame = bounds
        metalLayer.drawableSize = CGSize(
            width: bounds.width * scale,
            height: bounds.height * scale
        )
        onGeometryChange?()
    }

    // MARK: - Per-frame host state

    /// Apply what the app wants of the native side after a frame: the cursor
    /// and the IME caret position.
    func syncFrameState() {
        guard let app else { return }

        let code = cgb_mac_cursor(app)
        if code != lastCursorCode {
            lastCursorCode = code
            Self.cursor(for: code).set()
        }

        var x: Float = 0
        var y: Float = 0
        var width: Float = 0
        var height: Float = 0
        if cgb_mac_caret(app, &x, &y, &width, &height) {
            caretRect = NSRect(x: CGFloat(x), y: CGFloat(y), width: CGFloat(width), height: CGFloat(height))
        } else {
            caretRect = nil
        }
    }

    private static func cursor(for code: UInt32) -> NSCursor {
        switch code {
        case 1: return .pointingHand
        case 2: return .iBeam
        case 3: return .resizeLeftRight
        case 4: return .resizeUpDown
        case 5: return .openHand
        case 6: return .closedHand
        default: return .arrow
        }
    }

    // MARK: - Drag & drop

    override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation {
        .copy
    }

    override func draggingUpdated(_ sender: NSDraggingInfo) -> NSDragOperation {
        .copy
    }

    override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
        guard let app else { return false }
        let options: [NSPasteboard.ReadingOptionKey: Any] = [.urlReadingFileURLsOnly: true]
        guard let urls = sender.draggingPasteboard.readObjects(
            forClasses: [NSURL.self],
            options: options
        ) as? [URL] else {
            return false
        }
        for url in urls {
            url.path.withCString { cgb_mac_dropped_file(app, $0) }
        }
        return !urls.isEmpty
    }

    // MARK: - Geometry helpers

    /// AppKit's bottom-left coordinates → the logical top-left the UI uses.
    private func logicalPoint(_ event: NSEvent) -> (Float, Float) {
        let point = convert(event.locationInWindow, from: nil)
        return (Float(point.x), Float(bounds.height - point.y))
    }

    /// AppKit modifier flags → the ABI's bit mask.
    private func modifierBits(_ flags: NSEvent.ModifierFlags) -> UInt32 {
        var bits: UInt32 = 0
        if flags.contains(.shift) { bits |= 1 }
        if flags.contains(.control) { bits |= 2 }
        if flags.contains(.option) { bits |= 4 }
        if flags.contains(.command) { bits |= 8 }
        return bits
    }

    // MARK: - Pointer

    override func mouseDown(with event: NSEvent) {
        pointerDown(event, button: 0)
    }

    override func mouseUp(with event: NSEvent) {
        pointerUp(event, button: 0)
    }

    override func rightMouseDown(with event: NSEvent) {
        pointerDown(event, button: 1)
    }

    override func rightMouseUp(with event: NSEvent) {
        pointerUp(event, button: 1)
    }

    override func otherMouseDown(with event: NSEvent) {
        pointerDown(event, button: 2)
    }

    override func otherMouseUp(with event: NSEvent) {
        pointerUp(event, button: 2)
    }

    override func mouseMoved(with event: NSEvent) {
        pointerMove(event)
    }

    override func mouseDragged(with event: NSEvent) {
        pointerMove(event)
    }

    override func rightMouseDragged(with event: NSEvent) {
        pointerMove(event)
    }

    override func otherMouseDragged(with event: NSEvent) {
        pointerMove(event)
    }

    override func mouseEntered(with event: NSEvent) {
        pointerMove(event)
    }

    override func mouseExited(with event: NSEvent) {
        guard let app else { return }
        cgb_mac_pointer_leave(app)
    }

    override func scrollWheel(with event: NSEvent) {
        guard let app else { return }
        let (x, y) = logicalPoint(event)
        var dx = Float(event.scrollingDeltaX)
        var dy = -Float(event.scrollingDeltaY)
        if !event.hasPreciseScrollingDeltas {
            // A mouse wheel reports lines; a notch is about three text lines.
            dx *= 10
            dy *= 10
        }
        cgb_mac_scroll(app, x, y, dx, dy)
    }

    private func pointerDown(_ event: NSEvent, button: UInt32) {
        guard let app else { return }
        let (x, y) = logicalPoint(event)
        cgb_mac_pointer_down(app, x, y, button, UInt32(event.clickCount))
    }

    private func pointerUp(_ event: NSEvent, button: UInt32) {
        guard let app else { return }
        let (x, y) = logicalPoint(event)
        cgb_mac_pointer_up(app, x, y, button)
    }

    private func pointerMove(_ event: NSEvent) {
        guard let app else { return }
        let (x, y) = logicalPoint(event)
        cgb_mac_pointer_move(app, x, y)
    }

    // MARK: - Keyboard

    override func keyDown(with event: NSEvent) {
        guard let app else {
            super.keyDown(with: event)
            return
        }
        withCharacters(event.charactersIgnoringModifiers) { characters in
            cgb_mac_key_down(app, UInt32(event.keyCode), characters, modifierBits(event.modifierFlags))
        }
        // Let AppKit run the input method (and call `insertText:` for plain
        // typing); we never insert text ourselves.
        interpretKeyEvents([event])
    }

    override func keyUp(with event: NSEvent) {
        guard let app else {
            super.keyUp(with: event)
            return
        }
        withCharacters(event.charactersIgnoringModifiers) { characters in
            cgb_mac_key_up(app, UInt32(event.keyCode), characters)
        }
    }

    override func flagsChanged(with event: NSEvent) {
        guard let app else {
            super.flagsChanged(with: event)
            return
        }
        cgb_mac_modifiers(app, modifierBits(event.modifierFlags))
    }

    /// Navigation commands (arrows, Enter, Tab, Backspace) already reached Rust
    /// as key events; swallowing them keeps AppKit from beeping.
    override func doCommand(by selector: Selector) {}

    private func withCharacters(_ characters: String?, _ body: (UnsafePointer<CChar>?) -> Void) {
        if let characters {
            characters.withCString { body($0) }
        } else {
            body(nil)
        }
    }
}

// MARK: - NSTextInputClient

extension HostView: NSTextInputClient {
    func insertText(_ string: Any, replacementRange: NSRange) {
        guard let app else { return }
        let text = Self.plainString(string)
        text.withCString { cgb_mac_text(app, $0) }
    }

    func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        guard let app else { return }
        let text = Self.plainString(string)
        text.withCString {
            cgb_mac_ime(
                app,
                2,
                $0,
                Int32(selectedRange.location),
                Int32(selectedRange.location + selectedRange.length)
            )
        }
    }

    func unmarkText() {
        guard let app else { return }
        cgb_mac_ime(app, 1, nil, -1, -1)
    }

    func selectedRange() -> NSRange {
        NSRange(location: NSNotFound, length: 0)
    }

    func markedRange() -> NSRange {
        NSRange(location: NSNotFound, length: 0)
    }

    func hasMarkedText() -> Bool {
        false
    }

    func attributedSubstring(
        forProposedRange range: NSRange,
        actualRange: NSRangePointer?
    ) -> NSAttributedString? {
        nil
    }

    func validAttributesForMarkedText() -> [NSAttributedString.Key] {
        []
    }

    func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect {
        guard let caretRect, let window, caretRect.height > 0 else { return .zero }
        // The caret is logical with the origin at the top-left; AppKit view
        // coordinates put it at the bottom-left.
        let viewRect = NSRect(
            x: caretRect.minX,
            y: bounds.height - caretRect.maxY,
            width: max(caretRect.width, 1),
            height: caretRect.height
        )
        return window.convertToScreen(convert(viewRect, to: nil))
    }

    func characterIndex(for point: NSPoint) -> Int {
        0
    }

    private static func plainString(_ value: Any) -> String {
        if let text = value as? String {
            return text
        }
        if let attributed = value as? NSAttributedString {
            return attributed.string
        }
        return ""
    }
}
