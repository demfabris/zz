import AppKit
import CZZClient
import SwiftUI
import ZZNativeCore

struct TerminalSurface: View {
    let client: NativeClient
    let pane: NativePane
    let slot: TerminalSlot
    let generation: Int

    var body: some View {
        let padding = client.appearance.padding
        let insets = EdgeInsets(top: padding[0], leading: padding[3], bottom: padding[2], trailing: padding[1])
        let background = slot.frame?.background ?? 0x171719
        TerminalSurfaceView(client: client, pane: pane, slot: slot, generation: generation)
            .padding(insets)
            .background {
                GeometryReader { geometry in
                    Path { path in
                        path.addRect(CGRect(origin: .zero, size: geometry.size))
                        path.addRect(
                            CGRect(
                                x: insets.leading, y: insets.top,
                                width: max(0, geometry.size.width - insets.leading - insets.trailing),
                                height: max(0, geometry.size.height - insets.top - insets.bottom)))
                    }
                    .fill(
                        Color(
                            .sRGB, red: Double((background >> 16) & 0xff) / 255,
                            green: Double((background >> 8) & 0xff) / 255,
                            blue: Double(background & 0xff) / 255,
                            opacity: client.appearance.background_opacity), style: FillStyle(eoFill: true))
                }
            }
    }
}

private struct TerminalSurfaceView: NSViewRepresentable {
    let client: NativeClient
    let pane: NativePane
    let slot: TerminalSlot
    let generation: Int

    func makeNSView(context: Context) -> TerminalView {
        TerminalView(client: client, pane: pane.id)
    }

    func updateNSView(_ view: TerminalView, context: Context) {
        view.update(frame: slot.frame, active: pane.active, generation: generation, appearance: client.appearance)
    }
}

@MainActor
final class TerminalView: NSView, @preconcurrency NSTextInputClient {
    let client: NativeClient
    let pane: UInt64
    private var frameValue: TerminalFrame?
    private var active = false
    private var generation = -1
    private var lastSize = CGSize.zero
    private var lastScale: CGFloat = 0
    private var pendingKey: NSEvent?
    private var pressedKeys = Set<UInt16>()
    private var marked = NSAttributedString()
    private var markedSelection = NSRange(location: 0, length: 0)
    private var blinkTimer: Timer?
    private var blinkVisible = true
    private var wheelRemainder: CGFloat = 0
    private var terminalAppearance = NativeTerminalAppearance()
    private var font = NSFont.monospacedSystemFont(ofSize: 14, weight: .regular)
    private var cell: CGSize {
        let scale = window?.backingScaleFactor ?? 2
        return CGSize(
            width: ceil(("M" as NSString).size(withAttributes: [.font: font]).width * scale) / scale,
            height: ceil((font.ascender - font.descender + font.leading) * scale) / scale)
    }

    init(client: NativeClient, pane: UInt64) {
        self.client = client
        self.pane = pane
        super.init(frame: .zero)
        setAccessibilityElement(true)
        setAccessibilityRole(.textArea)
        setAccessibilityLabel("Terminal pane \(pane)")
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is unavailable") }
    override var isFlipped: Bool { true }
    override var isOpaque: Bool { terminalAppearance.background_opacity >= 1 }
    override var acceptsFirstResponder: Bool { true }
    override func accessibilityValue() -> Any? { frameValue?.text ?? "" }

    func update(frame: TerminalFrame?, active: Bool, generation: Int, appearance: NativeTerminalAppearance) {
        if self.terminalAppearance != appearance {
            self.terminalAppearance = appearance
            let weight: NSFont.Weight =
                appearance.font_weight >= 600 ? .bold : appearance.font_weight >= 500 ? .medium : .regular
            font =
                appearance.font_families.lazy.compactMap { NSFont(name: $0, size: appearance.font_size) }.first
                ?? NSFont.monospacedSystemFont(ofSize: appearance.font_size, weight: weight)
            if appearance.font_weight >= 600 { font = NSFontManager.shared.convert(font, toHaveTrait: .boldFontMask) }
            lastSize = .zero
            blinkTimer?.invalidate()
            blinkTimer = nil
            needsDisplay = true
            updateBlink()
        }
        if frameValue !== frame {
            frameValue = frame
            needsDisplay = true
            updateBlink()
        }
        if self.generation != generation {
            self.generation = generation
            lastSize = .zero
        }
        if self.active != active {
            self.active = active
            if active {
                lastSize = .zero
                window?.makeFirstResponder(self)
            }
            needsDisplay = true
        }
        resizeTerminal()
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if window != nil, active { window?.makeFirstResponder(self) }
        if window == nil {
            blinkTimer?.invalidate()
            blinkTimer = nil
        }
        resizeTerminal()
    }

    override func layout() {
        super.layout()
        resizeTerminal()
    }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        resizeTerminal()
    }

    private func resizeTerminal() {
        guard window != nil, bounds.width > 0, bounds.height > 0, client.connected else { return }
        let scale = window?.backingScaleFactor ?? 2
        let size = CGSize(
            width: max(1, floor(bounds.width / cell.width)), height: max(1, floor(bounds.height / cell.height)))
        guard size != lastSize || scale != lastScale else { return }
        lastSize = size
        lastScale = scale
        client.resize(
            pane, columns: UInt16(clamping: Int(size.width)), rows: UInt16(clamping: Int(size.height)),
            cell: CGSize(width: cell.width * scale, height: cell.height * scale))
    }

    override func becomeFirstResponder() -> Bool {
        client.focusTerminal(pane, focused: true)
        needsDisplay = true
        return true
    }

    override func resignFirstResponder() -> Bool {
        unmarkText()
        client.focusTerminal(pane, focused: false)
        needsDisplay = true
        return true
    }

    private func updateBlink() {
        let animate =
            frameValue?.cursor?.blinking != 0
            || frameValue?.styles.contains(where: { $0.attributes & UInt16(ZZ_ATTR_BLINK) != 0 }) == true
        guard animate, window != nil else {
            blinkTimer?.invalidate()
            blinkTimer = nil
            blinkVisible = true
            return
        }
        guard blinkTimer == nil else { return }
        let timer = Timer(timeInterval: max(0.05, Double(terminalAppearance.cursor_blink_ms) / 1000), repeats: true) {
            [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                self.blinkVisible.toggle()
                self.needsDisplay = true
            }
        }
        RunLoop.main.add(timer, forMode: .common)
        blinkTimer = timer
    }

    override func draw(_ dirtyRect: NSRect) {
        client.recordFrame()
        let background = color(frameValue?.background ?? 0x171719).withAlphaComponent(
            terminalAppearance.background_opacity)
        background.setFill()
        dirtyRect.fill()
        guard let frame = frameValue else { return }
        let size = cell
        let start = max(0, Int(floor(dirtyRect.minY / size.height)))
        let end = min(frame.rows, Int(ceil(dirtyRect.maxY / size.height)))
        guard start < end else { return }
        for row in start..<end {
            for column in 0..<frame.columns {
                let cellValue = frame.cells[row * frame.columns + column]
                let style =
                    frame.styles.indices.contains(Int(cellValue.style)) ? frame.styles[Int(cellValue.style)] : nil
                let cellBackground = style?.background ?? frame.background
                guard cellBackground != frame.background else { continue }
                color(cellBackground).setFill()
                CGRect(
                    x: CGFloat(column) * size.width, y: CGFloat(row) * size.height, width: size.width,
                    height: size.height
                ).fill()
            }
            for column in 0..<frame.columns {
                let index = row * frame.columns + column
                let cellValue = frame.cells[index]
                let width = cellValue.flags & UInt16(ZZ_CELL_WIDTH_MASK)
                let style =
                    frame.styles.indices.contains(Int(cellValue.style)) ? frame.styles[Int(cellValue.style)] : nil
                let attributes = style?.attributes ?? 0
                guard width < 2, attributes & UInt16(ZZ_ATTR_INVISIBLE) == 0,
                    attributes & UInt16(ZZ_ATTR_BLINK) == 0 || blinkVisible
                else { continue }
                let glyph = frame.glyph(at: index)
                guard !glyph.isEmpty else { continue }
                var glyphFont = font
                if attributes & UInt16(ZZ_ATTR_BOLD) != 0 {
                    glyphFont = NSFontManager.shared.convert(glyphFont, toHaveTrait: .boldFontMask)
                }
                if attributes & UInt16(ZZ_ATTR_ITALIC) != 0 {
                    glyphFont = NSFontManager.shared.convert(glyphFont, toHaveTrait: .italicFontMask)
                }
                let foreground = color(style?.foreground ?? frame.foreground).withAlphaComponent(
                    attributes & UInt16(ZZ_ATTR_FAINT) != 0 ? 0.55 : 1)
                let rect = CGRect(
                    x: CGFloat(column) * size.width, y: CGFloat(row) * size.height,
                    width: size.width * (width == 1 ? 2 : 1), height: size.height)
                (glyph as NSString).draw(
                    at: CGPoint(x: rect.minX, y: rect.minY),
                    withAttributes: [.font: glyphFont, .foregroundColor: foreground])
                if let style, style.underline_kind > 0 {
                    (style.underline_color == UInt32(ZZ_NO_COLOR) ? foreground : color(style.underline_color)).setFill()
                    CGRect(x: rect.minX, y: rect.maxY - 2, width: rect.width, height: 1).fill()
                    if style.underline_kind == 2 {
                        CGRect(x: rect.minX, y: rect.maxY - 4, width: rect.width, height: 1).fill()
                    }
                }
                foreground.setFill()
                if attributes & UInt16(ZZ_ATTR_STRIKETHROUGH) != 0 {
                    CGRect(x: rect.minX, y: rect.midY, width: rect.width, height: 1).fill()
                }
                if attributes & UInt16(ZZ_ATTR_OVERLINE) != 0 {
                    CGRect(x: rect.minX, y: rect.minY, width: rect.width, height: 1).fill()
                }
            }
        }
        if let cursor = frame.cursor, cursor.visible != 0, cursor.blinking == 0 || blinkVisible || !active {
            var rect = CGRect(
                x: CGFloat(cursor.column) * size.width, y: CGFloat(cursor.row) * size.height, width: size.width,
                height: size.height)
            let foreground = color(cursor.color)
            if !active || window?.firstResponder !== self {
                foreground.setStroke()
                NSBezierPath(rect: rect.insetBy(dx: 0.5, dy: 0.5)).stroke()
            } else {
                if cursor.style == 2 {
                    rect.origin.y = rect.maxY - 2
                    rect.size.height = 2
                }
                if cursor.style == 0 { rect.size.width = 2 }
                if cursor.style == 3 {
                    foreground.setStroke()
                    NSBezierPath(rect: rect.insetBy(dx: 0.5, dy: 0.5)).stroke()
                } else {
                    foreground.withAlphaComponent(cursor.style == 1 ? 0.45 : 1).setFill()
                    rect.fill()
                }
            }
        }
        if marked.length > 0 {
            let cursor = frame.cursor
            marked.draw(
                at: CGPoint(x: CGFloat(cursor?.column ?? 0) * size.width, y: CGFloat(cursor?.row ?? 0) * size.height))
        }
    }

    private func color(_ value: UInt32) -> NSColor {
        NSColor(
            srgbRed: CGFloat((value >> 16) & 255) / 255, green: CGFloat((value >> 8) & 255) / 255,
            blue: CGFloat(value & 255) / 255, alpha: 1)
    }

    override func keyDown(with event: NSEvent) {
        guard client.connected else { return }
        blinkVisible = true
        pendingKey = event
        defer { pendingKey = nil }
        if hasMarkedText() {
            interpretKeyEvents([event])
        } else if event.modifierFlags.contains(.control) || TerminalKey.code(event) != ZZ_KEY_CHARACTER.rawValue {
            send(event)
        } else {
            interpretKeyEvents([event])
        }
    }

    override func keyUp(with event: NSEvent) {
        if pressedKeys.remove(event.keyCode) != nil { send(event, release: true) }
    }

    private func send(_ event: NSEvent, release: Bool = false, committed: String? = nil) {
        if !release { pressedKeys.insert(event.keyCode) }
        let scalar = event.charactersIgnoringModifiers?.unicodeScalars.first?.value ?? 0
        client.key(
            pane, code: TerminalKey.code(event), scalar: scalar, function: TerminalKey.function(event),
            action: release ? 2 : event.isARepeat ? 1 : 0, modifiers: TerminalKey.modifiers(event.modifierFlags),
            text: committed ?? event.characters ?? "")
    }

    func insertText(_ string: Any, replacementRange: NSRange) {
        let text = (string as? NSAttributedString)?.string ?? (string as? String ?? "")
        let composing = hasMarkedText()
        unmarkText()
        guard !text.isEmpty else { return }
        if let pendingKey, !composing, text.unicodeScalars.count == 1 {
            send(pendingKey, committed: text)
        } else {
            client.text(text, pane: pane)
        }
    }

    override func doCommand(by selector: Selector) {
        if let pendingKey { send(pendingKey) }
    }
    func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        let text = (string as? NSAttributedString)?.string ?? (string as? String ?? "")
        marked = NSAttributedString(
            string: text,
            attributes: [
                .font: font, .foregroundColor: NSColor.textColor, .backgroundColor: NSColor.textBackgroundColor,
                .underlineStyle: NSUnderlineStyle.single.rawValue,
            ])
        markedSelection = selectedRange
        needsDisplay = true
    }
    func unmarkText() {
        marked = NSAttributedString()
        needsDisplay = true
    }
    func selectedRange() -> NSRange { markedSelection }
    func markedRange() -> NSRange {
        hasMarkedText() ? NSRange(location: 0, length: marked.length) : NSRange(location: NSNotFound, length: 0)
    }
    func hasMarkedText() -> Bool { marked.length > 0 }
    func attributedSubstring(forProposedRange range: NSRange, actualRange: NSRangePointer?) -> NSAttributedString? {
        nil
    }
    func validAttributesForMarkedText() -> [NSAttributedString.Key] {
        [.font, .foregroundColor, .backgroundColor, .underlineStyle]
    }
    func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect {
        actualRange?.pointee = range
        let cursor = frameValue?.cursor
        let rect = CGRect(
            x: CGFloat(cursor?.column ?? 0) * cell.width, y: CGFloat(cursor?.row ?? 0) * cell.height, width: cell.width,
            height: cell.height)
        return window?.convertToScreen(convert(rect, to: nil)) ?? rect
    }
    func characterIndex(for point: NSPoint) -> Int { NSNotFound }

    override func mouseDown(with event: NSEvent) {
        if !active { client.selectPane(pane) }
        window?.makeFirstResponder(self)
        pointer(event, phase: 0)
    }
    override func mouseDragged(with event: NSEvent) { pointer(event, phase: 1) }
    override func mouseUp(with event: NSEvent) { pointer(event, phase: 2) }
    private func pointer(_ event: NSEvent, phase: UInt32) {
        guard let frame = frameValue, frame.columns > 0, frame.rows > 0 else { return }
        let point = convert(event.locationInWindow, from: nil)
        client.selection(
            pane, phase: phase, column: UInt16(clamping: min(frame.columns - 1, max(0, Int(point.x / cell.width)))),
            row: UInt16(clamping: min(frame.rows - 1, max(0, Int(point.y / cell.height)))),
            clicks: UInt8(clamping: event.clickCount), rectangle: event.modifierFlags.contains(.option))
    }
    override func scrollWheel(with event: NSEvent) {
        wheelRemainder += event.scrollingDeltaY / (event.hasPreciseScrollingDeltas ? cell.height : 1)
        let lines = Int32(wheelRemainder.rounded(.towardZero))
        guard lines != 0 else { return }
        wheelRemainder -= CGFloat(lines)
        client.scroll(pane, lines: lines)
    }
    @objc func copy(_ sender: Any?) { client.copy(pane) }
    @objc func paste(_ sender: Any?) {
        if let text = NSPasteboard.general.string(forType: .string) { client.text(text, pane: pane, paste: true) }
    }
}

enum TerminalKey {
    static func modifiers(_ flags: NSEvent.ModifierFlags) -> UInt8 {
        (flags.contains(.shift) ? 1 : 0) | (flags.contains(.control) ? 2 : 0) | (flags.contains(.option) ? 4 : 0)
            | (flags.contains(.command) ? 8 : 0)
    }
    static func function(_ event: NSEvent) -> UInt8 {
        guard let scalar = event.charactersIgnoringModifiers?.unicodeScalars.first?.value, scalar >= 0xf704,
            scalar <= 0xf726
        else { return 0 }
        return UInt8(scalar - 0xf704 + 1)
    }
    static func code(_ event: NSEvent) -> UInt32 {
        if function(event) > 0 { return ZZ_KEY_FUNCTION.rawValue }
        switch event.keyCode {
        case 36, 76: return ZZ_KEY_ENTER.rawValue
        case 48: return ZZ_KEY_TAB.rawValue
        case 51: return ZZ_KEY_BACKSPACE.rawValue
        case 53: return ZZ_KEY_ESCAPE.rawValue
        case 117: return ZZ_KEY_DELETE.rawValue
        case 114: return ZZ_KEY_INSERT.rawValue
        case 115: return ZZ_KEY_HOME.rawValue
        case 119: return ZZ_KEY_END.rawValue
        case 116: return ZZ_KEY_PAGE_UP.rawValue
        case 121: return ZZ_KEY_PAGE_DOWN.rawValue
        case 123: return ZZ_KEY_ARROW_LEFT.rawValue
        case 124: return ZZ_KEY_ARROW_RIGHT.rawValue
        case 125: return ZZ_KEY_ARROW_DOWN.rawValue
        case 126: return ZZ_KEY_ARROW_UP.rawValue
        default: return ZZ_KEY_CHARACTER.rawValue
        }
    }
}
