import AppKit
import CZZClient
import CoreImage
import MetalKit
import SwiftUI
import ZZNativeCore

struct NativeBrowserSurface: NSViewRepresentable {
    let client: NativeClient
    let pane: UInt64
    let tab: NativeBrowserTab
    let active: Bool
    let frame: NativeBrowserFrame?

    func makeNSView(context: Context) -> BrowserView { BrowserView(client: client, pane: pane, tab: tab) }
    func updateNSView(_ view: BrowserView, context: Context) { view.update(tab: tab, active: active, frame: frame) }
}

@MainActor
final class BrowserView: MTKView, @preconcurrency NSTextInputClient {
    let client: NativeClient
    let pane: UInt64
    private var tab: NativeBrowserTab
    private var currentFrame: NativeBrowserFrame?
    private var active = false
    private var context: CIContext?
    private var commands: (any MTLCommandQueue)?
    private var trackedArea: NSTrackingArea?
    private var marked = NSAttributedString()
    private var markedSelection = NSRange(location: 0, length: 0)
    private var pendingKey: NSEvent?
    private var menuVersion = 0
    private var lastViewport: CGRect = .zero
    private var lastScale: CGFloat = 0
    private var retryScheduled = false
    private var lastSession: UInt64 = 0
    private lazy var browserInputContext = NSTextInputContext(client: self)
    override var inputContext: NSTextInputContext? { browserInputContext }

    init(client: NativeClient, pane: UInt64, tab: NativeBrowserTab) {
        self.client = client
        self.pane = pane
        self.tab = tab
        let device = MTLCreateSystemDefaultDevice()
        super.init(frame: .zero, device: device)
        if let device { context = CIContext(mtlDevice: device); commands = device.makeCommandQueue() }
        isPaused = true
        enableSetNeedsDisplay = true
        framebufferOnly = false
        colorPixelFormat = .bgra8Unorm
        colorspace = CGColorSpace(name: CGColorSpace.sRGB)
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityLabel("Browser page")
    }

    required init(coder: NSCoder) { fatalError("init(coder:) is unavailable") }
    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    override var mouseDownCanMoveWindow: Bool { false }

    func update(tab: NativeBrowserTab, active: Bool, frame: NativeBrowserFrame?) {
        if self.tab !== tab {
            self.tab = tab
            lastSession = 0
            unmarkText()
        }
        if self.active != active {
            self.active = active
            if active { window?.makeFirstResponder(self) }
        }
        if currentFrame !== frame {
            currentFrame = frame
            needsDisplay = true
        }
        setAccessibilityLabel(tab.title)
        if menuVersion != tab.contextMenuVersion, let request = tab.contextMenu {
            menuVersion = tab.contextMenuVersion
            DispatchQueue.main.async { [weak self] in self?.showMenu(request) }
        }
        window?.invalidateCursorRects(for: self)
        updateViewport()
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let frame = currentFrame, let image = frame.image, let context,
            drawableSize.width > 0, drawableSize.height > 0
        else { return }
        guard let drawable = currentDrawable, let command = commands?.makeCommandBuffer() else {
            retryDisplay()
            return
        }
        let bounds = CGRect(origin: .zero, size: drawableSize)
        let scaled = image.transformed(
            by: CGAffineTransform(scaleX: bounds.width / frame.size.width, y: bounds.height / frame.size.height))
        context.render(
            scaled, to: drawable.texture, commandBuffer: command, bounds: bounds,
            colorSpace: CGColorSpace(name: CGColorSpace.sRGB)!)
        command.addCompletedHandler { [weak self] _ in
            DispatchQueue.main.async { [weak self] in
                if let self, self.currentFrame !== frame { self.needsDisplay = true }
            }
        }
        command.present(drawable)
        command.commit()
        client.recordFrame()
    }

    private func retryDisplay() {
        guard !retryScheduled else { return }
        retryScheduled = true
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.0 / 60.0) { [weak self] in
            guard let self else { return }
            self.retryScheduled = false
            if self.window != nil { self.needsDisplay = true }
        }
    }

    override func layout() { super.layout(); updateViewport() }
    override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); updateViewport() }
    override func viewDidChangeBackingProperties() { super.viewDidChangeBackingProperties(); updateViewport() }
    private func updateViewport(force: Bool = false, focused: Bool? = nil) {
        guard let window, bounds.width > 0, bounds.height > 0 else { return }
        let scale = window.backingScaleFactor
        let screen = window.convertToScreen(convert(bounds, to: nil))
        guard force || lastViewport != screen || lastScale != scale || lastSession != tab.session else { return }
        lastViewport = screen
        lastScale = scale
        lastSession = tab.session
        client.browsers.viewport(
            tab, size: bounds.size, scale: scale, screen: screen.origin,
            focused: focused ?? (window.firstResponder === self))
    }
    override func becomeFirstResponder() -> Bool { updateViewport(force: true, focused: true); return true }
    override func resignFirstResponder() -> Bool {
        unmarkText(); updateViewport(force: true, focused: false); return true
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let trackedArea { removeTrackingArea(trackedArea) }
        let area = NSTrackingArea(
            rect: .zero, options: [.activeInKeyWindow, .inVisibleRect, .mouseMoved, .mouseEnteredAndExited], owner: self
        )
        addTrackingArea(area)
        trackedArea = area
    }
    override func resetCursorRects() {
        let cursor: NSCursor
        switch tab.cursor {
        case "IBeam": cursor = .iBeam
        case "PointingHand": cursor = .pointingHand
        case "Crosshair": cursor = .crosshair
        case "ResizeHorizontal": cursor = .resizeLeftRight
        case "ResizeVertical": cursor = .resizeUpDown
        case "NotAllowed": cursor = .operationNotAllowed
        case "Grab": cursor = .openHand
        case "Grabbing": cursor = .closedHand
        default: cursor = .arrow
        }
        addCursorRect(bounds, cursor: cursor)
    }

    override func mouseDown(with event: NSEvent) {
        if !active { client.selectPane(pane) }
        window?.makeFirstResponder(self)
        pointer(event, phase: 2, button: 1)
    }
    override func mouseUp(with event: NSEvent) { pointer(event, phase: 3, button: 1) }
    override func mouseDragged(with event: NSEvent) { pointer(event, phase: 0, button: 1) }
    override func rightMouseDown(with event: NSEvent) {
        if !active { client.selectPane(pane) }
        window?.makeFirstResponder(self)
        pointer(event, phase: 2, button: 3)
    }
    override func rightMouseDragged(with event: NSEvent) { pointer(event, phase: 0, button: 3) }
    override func rightMouseUp(with event: NSEvent) { pointer(event, phase: 3, button: 3) }
    override func otherMouseDown(with event: NSEvent) {
        if !active { client.selectPane(pane) }
        window?.makeFirstResponder(self)
        pointer(event, phase: 2, button: 2)
    }
    override func otherMouseDragged(with event: NSEvent) { pointer(event, phase: 0, button: 2) }
    override func otherMouseUp(with event: NSEvent) { pointer(event, phase: 3, button: 2) }
    override func mouseMoved(with event: NSEvent) { pointer(event, phase: 0, button: 0) }
    override func mouseExited(with event: NSEvent) { pointer(event, phase: 1, button: 0) }
    private func pointer(_ event: NSEvent, phase: UInt32, button: UInt32) {
        var flags = TerminalKey.modifiers(event.modifierFlags)
        if phase == 2 || phase == 0 {
            if button == 1 { flags |= 16 } else if button == 2 { flags |= 32 } else if button == 3 { flags |= 64 }
        }
        let clicks = phase == 2 || phase == 3 ? event.clickCount : 0
        client.browsers.pointer(
            tab, point: convert(event.locationInWindow, from: nil), phase: phase, button: button, clicks: clicks,
            flags: flags)
    }
    override func scrollWheel(with event: NSEvent) {
        client.browsers.wheel(
            tab, point: convert(event.locationInWindow, from: nil),
            delta: CGPoint(x: event.scrollingDeltaX, y: event.scrollingDeltaY),
            precise: event.hasPreciseScrollingDeltas,
            flags: TerminalKey.modifiers(event.modifierFlags))
    }

    override func keyDown(with event: NSEvent) {
        pendingKey = event
        defer { pendingKey = nil }
        if hasMarkedText() { interpretKeyEvents([event]); return }
        let textual =
            TerminalKey.code(event) == ZZ_KEY_CHARACTER.rawValue && !event.modifierFlags.contains(.control)
            && !event.modifierFlags.contains(.command)
        send(event, follows: textual)
        if textual { interpretKeyEvents([event]) }
    }
    override func keyUp(with event: NSEvent) { send(event, release: true) }
    private func send(_ event: NSEvent, release: Bool = false, follows: Bool = false) {
        client.browserKey(
            pane, code: TerminalKey.code(event),
            scalar: event.charactersIgnoringModifiers?.unicodeScalars.first?.value ?? 0,
            function: TerminalKey.function(event), action: release ? 2 : event.isARepeat ? 1 : 0,
            modifiers: TerminalKey.modifiers(event.modifierFlags), text: event.characters ?? "", textFollows: follows)
    }
    func insertText(_ value: Any, replacementRange: NSRange) {
        let text = (value as? NSAttributedString)?.string ?? (value as? String ?? "")
        let composing = hasMarkedText()
        marked = NSAttributedString()
        if composing {
            client.browsers.action(tab, "commit", ["text": text])
        } else {
            client.browserText(text, pane: pane)
        }
    }
    override func doCommand(by selector: Selector) {}
    func setMarkedText(_ value: Any, selectedRange: NSRange, replacementRange: NSRange) {
        let text = (value as? NSAttributedString)?.string ?? (value as? String ?? "")
        marked = NSAttributedString(string: text)
        markedSelection = selectedRange
        client.browsers.action(
            tab, "composition", ["text": text, "start": selectedRange.location, "end": NSMaxRange(selectedRange)])
    }
    func unmarkText() {
        if hasMarkedText() { client.browsers.action(tab, "cancel-composition") }
        marked = NSAttributedString()
    }
    func selectedRange() -> NSRange { markedSelection }
    func markedRange() -> NSRange {
        hasMarkedText() ? NSRange(location: 0, length: marked.length) : NSRange(location: NSNotFound, length: 0)
    }
    func hasMarkedText() -> Bool { marked.length != 0 }
    func attributedSubstring(forProposedRange range: NSRange, actualRange: NSRangePointer?) -> NSAttributedString? {
        nil
    }
    func validAttributesForMarkedText() -> [NSAttributedString.Key] { [] }
    func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect {
        window?.convertToScreen(convert(CGRect(x: 0, y: 0, width: 1, height: 20), to: nil)) ?? .zero
    }
    func characterIndex(for point: NSPoint) -> Int { NSNotFound }

    private func showMenu(_ request: [String: Any]) {
        let menu = NSMenu()
        func add(_ title: String, _ action: String, _ fields: [String: Any] = [:], enabled: Bool = true) {
            let item = NSMenuItem(title: title, action: #selector(menuAction(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = fields.merging(["action": action]) { _, new in new }
            item.isEnabled = enabled
            menu.addItem(item)
        }
        menu.autoenablesItems = false
        if let url = request["url"] as? String {
            add("Open link in new tab", "new-tab", ["url": url])
            add("Copy link", "copy-link", ["url": url])
            menu.addItem(.separator())
        }
        add("Back", "back", enabled: tab.canGoBack)
        add("Forward", "forward", enabled: tab.canGoForward)
        add("Reload", "reload")
        menu.addItem(.separator())
        for (title, flag) in [("Cut", "cut"), ("Copy", "copy"), ("Paste", "paste"), ("Select all", "selectAll")] {
            add(
                title, "edit", ["command": title == "Select all" ? "SelectAll" : title],
                enabled: request[flag] as? Bool == true)
        }
        menu.addItem(.separator())
        add("Inspect element", "inspect", ["x": request["x"] ?? 0, "y": request["y"] ?? 0])
        menu.popUp(positioning: nil, at: CGPoint(x: request["x"] as? Int ?? 0, y: request["y"] as? Int ?? 0), in: self)
    }
    @objc private func menuAction(_ sender: NSMenuItem) {
        guard var fields = sender.representedObject as? [String: Any],
            let action = fields.removeValue(forKey: "action") as? String
        else { return }
        if action == "new-tab", let pane = client.browsers.panes[pane], let url = fields["url"] as? String {
            client.browsers.newTab(pane, url: url)
        } else if action == "copy-link", let url = fields["url"] as? String {
            NSPasteboard.general.clearContents(); NSPasteboard.general.setString(url, forType: .string)
        } else {
            client.browsers.action(tab, action, fields)
        }
    }
    @objc func copy(_ sender: Any?) { client.browsers.action(tab, "edit", ["command": "Copy"]) }
    @objc func paste(_ sender: Any?) { client.browsers.action(tab, "edit", ["command": "Paste"]) }
}
