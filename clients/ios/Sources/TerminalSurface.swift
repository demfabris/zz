import SwiftUI
import UIKit

enum TerminalFontZoom {
    static let minimumStep = -4
    static let maximumStep = 10
    static let defaultPointSize: CGFloat = 13

    static func clamped(_ step: Int) -> Int {
        min(max(step, minimumStep), maximumStep)
    }

    static func pointSize(for step: Int) -> CGFloat {
        defaultPointSize + CGFloat(clamped(step))
    }

    static func targetStep(anchor: Int, scale: CGFloat) -> Int {
        let scaledPointSize = pointSize(for: anchor) * max(scale, 0.01)
        return clamped(Int((scaledPointSize - defaultPointSize).rounded()))
    }

    static func crossedSteps(from current: Int, to target: Int) -> [Int] {
        guard current != target else {
            return []
        }
        let direction = target > current ? 1 : -1
        return Array(stride(from: current + direction, through: target, by: direction))
    }
}

@MainActor
final class TerminalGridView: UIView, UIKeyInput {
    private static var softwareKeyboardVisible = false

    var pane: UInt64 = 0 {
        didSet {
            guard pane != oldValue else {
                return
            }
            lastResize = nil
            responderRevision &+= 1
        }
    }
    var onText: ((String) -> Void)?
    var onKey: ((UInt32, UInt32, UInt8, UInt8) -> Void)?
    var onResize: ((TerminalLayout, Bool) -> Void)?
    var onScroll: ((Int) -> Void)?
    var onFontSizeStep: ((Int) -> Void)?
    var onFocus: (() -> Void)?

    private var viewport: TerminalFrame?
    private var interactive = false
    private var preview = false
    private var fontSize: CGFloat = 0
    private var regularFont = UIFont.monospacedSystemFont(ofSize: 13, weight: .regular)
    private var boldFont = UIFont.monospacedSystemFont(ofSize: 13, weight: .bold)
    private var italicFont = UIFont.monospacedSystemFont(ofSize: 13, weight: .regular)
    private var boldItalicFont = UIFont.monospacedSystemFont(ofSize: 13, weight: .bold)
    private var measuredCell = CGSize(width: 8, height: 16)
    private var logicalCell = CGSize(width: 8, height: 16)
    private var lastResize: TerminalLayout?
    private var panRows = 0
    private var fontSizeStep = 0
    private var pinchAnchorStep = 0
    private var pinchReportedStep = 0
    private var cursorVisible = true
    private var blinkTimer: Timer?
    private var colors: [UInt32: UIColor] = [:]
    private let fontFeedback = UISelectionFeedbackGenerator()
    private var inputRequested = false
    private var sceneActive = true
    private var inputActivation: UInt64 = 0
    private var responderRevision: UInt64 = 0
    private var viewportIsStable = true

    override init(frame: CGRect) {
        super.init(frame: frame)
        isOpaque = true
        clipsToBounds = true
        contentMode = .redraw
        let tap = UITapGestureRecognizer(target: self, action: #selector(focusInput))
        addGestureRecognizer(tap)
        let pan = UIPanGestureRecognizer(target: self, action: #selector(scroll(_:)))
        pan.maximumNumberOfTouches = 1
        addGestureRecognizer(pan)
        let pinch = UIPinchGestureRecognizer(target: self, action: #selector(zoom(_:)))
        addGestureRecognizer(pinch)
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(keyboardWillShow(_:)),
            name: UIResponder.keyboardWillShowNotification,
            object: nil
        )
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(keyboardDidHide(_:)),
            name: UIResponder.keyboardDidHideNotification,
            object: nil
        )
    }

    required init?(coder: NSCoder) {
        nil
    }

    deinit {
        NotificationCenter.default.removeObserver(self)
    }

    override var canBecomeFirstResponder: Bool {
        interactive && inputRequested && sceneActive
    }

    var hasText: Bool {
        true
    }

    func insertText(_ text: String) {
        onText?(text)
    }

    func deleteBackward() {
        onKey?(
            UInt32(ZZ_KEY_BACKSPACE.rawValue),
            0,
            0,
            UInt8(ZZ_KEY_PRESS.rawValue)
        )
    }

    func configure(
        frame: TerminalFrame?,
        interactive: Bool,
        preview: Bool,
        fontSizeStep: Int,
        inputRequested: Bool,
        sceneActive: Bool,
        inputActivation: UInt64
    ) {
        let generationChanged = viewport?.generation != frame?.generation ||
            viewport?.viewGeneration != frame?.viewGeneration
        let inputChanged = self.interactive != interactive ||
            self.inputRequested != inputRequested ||
            self.sceneActive != sceneActive ||
            self.inputActivation != inputActivation
        viewport = frame
        self.interactive = interactive
        self.preview = preview
        self.inputRequested = inputRequested
        self.sceneActive = sceneActive
        self.inputActivation = inputActivation
        if !sceneActive {
            Self.softwareKeyboardVisible = false
            viewportIsStable = true
        } else if Self.softwareKeyboardVisible {
            viewportIsStable = false
        }
        let nextFontSizeStep = TerminalFontZoom.clamped(fontSizeStep)
        if self.fontSizeStep != nextFontSizeStep {
            self.fontSizeStep = nextFontSizeStep
            lastResize = nil
        }
        isUserInteractionEnabled = interactive
        if inputChanged {
            reconcileInput()
        }
        if let frame {
            backgroundColor = color(frame.background)
        }
        updateMetrics()
        updateBlinkTimer()
        if generationChanged, let frame, !frame.damage.all {
            let first = CGFloat(frame.damage.firstRow) * measuredCell.height
            let last = CGFloat(frame.damage.lastRow + 1) * measuredCell.height
            setNeedsDisplay(CGRect(x: 0, y: first, width: bounds.width, height: last - first))
        } else if generationChanged {
            setNeedsDisplay()
        }
        resizeIfNeeded()
    }

    func deactivateInput() {
        inputRequested = false
        reconcileInput()
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        if window == nil {
            blinkTimer?.invalidate()
            blinkTimer = nil
        } else {
            updateBlinkTimer()
            resizeIfNeeded()
        }
        reconcileInput()
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        updateMetrics()
        resizeIfNeeded()
        setNeedsDisplay()
    }

    override func draw(_ rect: CGRect) {
        guard
            let viewport,
            let context = UIGraphicsGetCurrentContext(),
            viewport.columns > 0,
            viewport.rows > 0
        else {
            return
        }
        context.setFillColor(color(viewport.background).cgColor)
        context.fill(rect)

        let firstRow = max(0, Int(floor(rect.minY / measuredCell.height)))
        let lastRow = min(viewport.rows - 1, Int(floor(rect.maxY / measuredCell.height)))
        guard firstRow <= lastRow else {
            return
        }
        for row in firstRow...lastRow {
            draw(row: row, viewport: viewport, context: context)
        }
        drawCursor(viewport, context: context)
    }

    override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        var forwarded = false
        for press in presses {
            guard let key = press.key, let mapped = map(key) else {
                continue
            }
            forwarded = true
            onKey?(mapped.code, mapped.scalar, modifierBits(key.modifierFlags), UInt8(ZZ_KEY_PRESS.rawValue))
        }
        if !forwarded {
            super.pressesBegan(presses, with: event)
        }
    }

    override func pressesEnded(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        var forwarded = false
        for press in presses {
            guard let key = press.key, let mapped = map(key) else {
                continue
            }
            forwarded = true
            onKey?(mapped.code, mapped.scalar, modifierBits(key.modifierFlags), UInt8(ZZ_KEY_RELEASE.rawValue))
        }
        if !forwarded {
            super.pressesEnded(presses, with: event)
        }
    }

    @objc private func focusInput() {
        onFocus?()
    }

    @objc private func scroll(_ gesture: UIPanGestureRecognizer) {
        guard interactive else {
            return
        }
        switch gesture.state {
        case .began:
            panRows = 0
        case .changed:
            let rows = Int(gesture.translation(in: self).y / max(measuredCell.height, 1))
            let delta = rows - panRows
            if delta != 0 {
                onScroll?(-delta)
                panRows = rows
            }
        default:
            panRows = 0
        }
    }

    @objc private func zoom(_ gesture: UIPinchGestureRecognizer) {
        guard interactive else {
            return
        }
        switch gesture.state {
        case .began:
            pinchAnchorStep = fontSizeStep
            pinchReportedStep = fontSizeStep
            fontFeedback.prepare()
        case .changed:
            let target = TerminalFontZoom.targetStep(anchor: pinchAnchorStep, scale: gesture.scale)
            for step in TerminalFontZoom.crossedSteps(from: pinchReportedStep, to: target) {
                pinchReportedStep = step
                fontSizeStep = step
                lastResize = nil
                updateMetrics()
                resizeIfNeeded()
                setNeedsDisplay()
                onFontSizeStep?(fontSizeStep)
                fontFeedback.selectionChanged()
                fontFeedback.prepare()
            }
        default:
            pinchAnchorStep = fontSizeStep
            pinchReportedStep = fontSizeStep
        }
    }

    private func updateMetrics() {
        guard let viewport, viewport.columns > 0, viewport.rows > 0 else {
            return
        }
        let baseSize = TerminalFontZoom.pointSize(for: fontSizeStep)
        let baseFont = UIFont.monospacedSystemFont(ofSize: baseSize, weight: .regular)
        let baseWidth = ceil(("M" as NSString).size(withAttributes: [.font: baseFont]).width)
        let baseHeight = ceil(baseFont.lineHeight * 1.08)
        logicalCell = CGSize(width: baseWidth, height: baseHeight)
        let nextSize: CGFloat
        if preview {
            let scale = min(
                bounds.width / (CGFloat(viewport.columns) * baseWidth),
                bounds.height / (CGFloat(viewport.rows) * baseHeight)
            )
            nextSize = max(2, baseSize * min(scale, 0.72))
        } else {
            nextSize = baseSize
        }
        guard abs(nextSize - fontSize) > 0.01 else {
            return
        }
        fontSize = nextSize
        regularFont = UIFont.monospacedSystemFont(ofSize: nextSize, weight: .regular)
        boldFont = UIFont.monospacedSystemFont(ofSize: nextSize, weight: .bold)
        let descriptor = regularFont.fontDescriptor.withSymbolicTraits(.traitItalic)
        italicFont = descriptor.map { UIFont(descriptor: $0, size: nextSize) } ?? regularFont
        let boldDescriptor = boldFont.fontDescriptor.withSymbolicTraits([.traitBold, .traitItalic])
        boldItalicFont = boldDescriptor.map { UIFont(descriptor: $0, size: nextSize) } ?? boldFont
        measuredCell = CGSize(
            width: ceil(("M" as NSString).size(withAttributes: [.font: regularFont]).width),
            height: ceil(regularFont.lineHeight * 1.08)
        )
    }

    private func resizeIfNeeded() {
        guard interactive, window != nil, viewport != nil,
              let layout = TerminalLayout(bounds: bounds.size, cell: logicalCell) else {
            return
        }
        guard layout != lastResize else {
            return
        }
        lastResize = layout
        onResize?(layout, viewportIsStable)
    }

    private func reconcileInput() {
        responderRevision &+= 1
        let revision = responderRevision
        Task { @MainActor [weak self] in
            await Task.yield()
            guard let self, self.responderRevision == revision else {
                return
            }
            let shouldOwnInput = self.interactive &&
                self.inputRequested &&
                self.sceneActive &&
                self.window != nil
            if shouldOwnInput {
                if !self.isFirstResponder {
                    self.becomeFirstResponder()
                }
            } else if self.isFirstResponder {
                self.resignFirstResponder()
            }
        }
    }

    @objc private func keyboardWillShow(_ notification: Notification) {
        guard keyboardIsLocal(notification) else {
            return
        }
        Self.softwareKeyboardVisible = true
        viewportIsStable = false
    }

    @objc private func keyboardDidHide(_ notification: Notification) {
        guard keyboardIsLocal(notification) else {
            return
        }
        Self.softwareKeyboardVisible = false
        viewportIsStable = true
        lastResize = nil
        resizeIfNeeded()
    }

    private func keyboardIsLocal(_ notification: Notification) -> Bool {
        notification.userInfo?[UIResponder.keyboardIsLocalUserInfoKey] as? Bool ?? true
    }

    private func updateBlinkTimer() {
        blinkTimer?.invalidate()
        blinkTimer = nil
        cursorVisible = true
        let blinkingText = viewport?.styles.contains {
            $0.attributes & UInt16(ZZ_ATTR_BLINK) != 0
        } ?? false
        guard interactive, viewport?.cursor?.blinking != 0 || blinkingText else {
            return
        }
        blinkTimer = Timer.scheduledTimer(
            timeInterval: 0.5,
            target: self,
            selector: #selector(blinkCursor),
            userInfo: nil,
            repeats: true
        )
    }

    @objc private func blinkCursor() {
        cursorVisible.toggle()
        setNeedsDisplay()
    }

    private func draw(row: Int, viewport: TerminalFrame, context: CGContext) {
        for column in 0..<viewport.columns {
            let index = row * viewport.columns + column
            guard viewport.cells.indices.contains(index) else {
                continue
            }
            let cell = viewport.cells[index]
            let style = viewport.style(for: cell)
            let attributes = style?.attributes ?? 0
            let cellRect = CGRect(
                x: CGFloat(column) * measuredCell.width,
                y: CGFloat(row) * measuredCell.height,
                width: measuredCell.width,
                height: measuredCell.height
            )
            context.setFillColor(color(style?.background ?? viewport.background).cgColor)
            context.fill(cellRect)

            let width = cell.flags & UInt16(ZZ_CELL_WIDTH_MASK)
            guard
                width < 2,
                attributes & UInt16(ZZ_ATTR_INVISIBLE) == 0,
                attributes & UInt16(ZZ_ATTR_BLINK) == 0 || cursorVisible
            else {
                continue
            }
            let glyph = viewport.glyph(at: index)
            guard !glyph.isEmpty else {
                continue
            }
            let font = font(attributes)
            let foreground = color(style?.foreground ?? viewport.foreground)
                .withAlphaComponent(attributes & UInt16(ZZ_ATTR_FAINT) != 0 ? 0.55 : 1)
            let glyphRect = CGRect(
                x: cellRect.minX,
                y: cellRect.minY + (measuredCell.height - font.lineHeight) * 0.5,
                width: measuredCell.width * (width == 1 ? 2 : 1),
                height: font.lineHeight
            )
            (glyph as NSString).draw(
                in: glyphRect,
                withAttributes: [.font: font, .foregroundColor: foreground]
            )
            let decorationRect = CGRect(
                x: cellRect.minX,
                y: cellRect.minY,
                width: measuredCell.width * (width == 1 ? 2 : 1),
                height: cellRect.height
            )
            let underlineColor = style.map {
                $0.underline_color == UInt32(ZZ_NO_COLOR) ? foreground : color($0.underline_color)
            } ?? foreground
            drawDecorations(
                attributes,
                underline: style?.underline_kind ?? 0,
                rect: decorationRect,
                foreground: foreground,
                underlineColor: underlineColor,
                context: context
            )
        }
    }

    private func drawDecorations(
        _ attributes: UInt16,
        underline: UInt8,
        rect: CGRect,
        foreground: UIColor,
        underlineColor: UIColor,
        context: CGContext
    ) {
        let thickness = max(1, fontSize / 14)
        if underline > 0 {
            context.saveGState()
            context.setStrokeColor(underlineColor.cgColor)
            context.setLineWidth(thickness)
            let y = rect.maxY - max(1, measuredCell.height * 0.08)
            if underline == 3 {
                let wavelength = max(3, fontSize * 0.35)
                let amplitude = max(1, thickness)
                context.move(to: CGPoint(x: rect.minX, y: y))
                var x = rect.minX
                var rising = true
                while x < rect.maxX {
                    let end = min(x + wavelength * 0.5, rect.maxX)
                    let midpoint = x + (end - x) * 0.5
                    context.addQuadCurve(
                        to: CGPoint(x: end, y: y),
                        control: CGPoint(x: midpoint, y: y + (rising ? -amplitude : amplitude))
                    )
                    x = end
                    rising.toggle()
                }
            } else {
                if underline == 4 {
                    context.setLineDash(phase: 0, lengths: [thickness, thickness * 2])
                } else if underline == 5 {
                    context.setLineDash(phase: 0, lengths: [thickness * 4, thickness * 2])
                }
                context.move(to: CGPoint(x: rect.minX, y: y))
                context.addLine(to: CGPoint(x: rect.maxX, y: y))
                if underline == 2 {
                    context.move(to: CGPoint(x: rect.minX, y: y - 2))
                    context.addLine(to: CGPoint(x: rect.maxX, y: y - 2))
                }
            }
            context.strokePath()
            context.restoreGState()
        }
        context.setStrokeColor(foreground.cgColor)
        context.setLineWidth(thickness)
        if attributes & UInt16(ZZ_ATTR_STRIKETHROUGH) != 0 {
            let y = rect.midY
            context.move(to: CGPoint(x: rect.minX, y: y))
            context.addLine(to: CGPoint(x: rect.maxX, y: y))
        }
        if attributes & UInt16(ZZ_ATTR_OVERLINE) != 0 {
            context.move(to: CGPoint(x: rect.minX, y: rect.minY + 1))
            context.addLine(to: CGPoint(x: rect.maxX, y: rect.minY + 1))
        }
        context.strokePath()
    }

    private func drawCursor(_ viewport: TerminalFrame, context: CGContext) {
        guard
            let cursor = viewport.cursor,
            cursor.visible != 0,
            cursor.blinking == 0 || cursorVisible
        else {
            return
        }
        let rect = CGRect(
            x: CGFloat(cursor.column) * measuredCell.width,
            y: CGFloat(cursor.row) * measuredCell.height,
            width: measuredCell.width,
            height: measuredCell.height
        )
        context.setFillColor(color(cursor.color).cgColor)
        context.setStrokeColor(color(cursor.color).cgColor)
        switch cursor.style {
        case 0:
            context.fill(CGRect(x: rect.minX, y: rect.minY, width: max(2, measuredCell.width * 0.16), height: rect.height))
        case 2:
            context.fill(CGRect(x: rect.minX, y: rect.maxY - max(2, measuredCell.height * 0.12), width: rect.width, height: max(2, measuredCell.height * 0.12)))
        case 3:
            context.setLineWidth(max(1, fontSize / 12))
            context.stroke(rect.insetBy(dx: 0.5, dy: 0.5))
        default:
            context.setAlpha(0.62)
            context.fill(rect)
            context.setAlpha(1)
        }
    }

    private func font(_ attributes: UInt16) -> UIFont {
        let bold = attributes & UInt16(ZZ_ATTR_BOLD) != 0
        let italic = attributes & UInt16(ZZ_ATTR_ITALIC) != 0
        return switch (bold, italic) {
        case (true, true): boldItalicFont
        case (true, false): boldFont
        case (false, true): italicFont
        case (false, false): regularFont
        }
    }

    private func color(_ packed: UInt32) -> UIColor {
        if let cached = colors[packed] {
            return cached
        }
        let value = UIColor(
            red: CGFloat((packed >> 16) & 0xff) / 255,
            green: CGFloat((packed >> 8) & 0xff) / 255,
            blue: CGFloat(packed & 0xff) / 255,
            alpha: 1
        )
        colors[packed] = value
        return value
    }

    private func modifierBits(_ flags: UIKeyModifierFlags) -> UInt8 {
        var value: UInt8 = 0
        if flags.contains(.shift) { value |= 1 << 0 }
        if flags.contains(.control) { value |= 1 << 1 }
        if flags.contains(.alternate) { value |= 1 << 2 }
        if flags.contains(.command) { value |= 1 << 3 }
        return value
    }

    private func map(_ key: UIKey) -> (code: UInt32, scalar: UInt32)? {
        let code: UInt32?
        switch key.keyCode {
        case .keyboardDeleteOrBackspace: code = UInt32(ZZ_KEY_BACKSPACE.rawValue)
        case .keyboardReturnOrEnter: code = UInt32(ZZ_KEY_ENTER.rawValue)
        case .keyboardTab: code = UInt32(ZZ_KEY_TAB.rawValue)
        case .keyboardEscape: code = UInt32(ZZ_KEY_ESCAPE.rawValue)
        case .keyboardDeleteForward: code = UInt32(ZZ_KEY_DELETE.rawValue)
        case .keyboardHome: code = UInt32(ZZ_KEY_HOME.rawValue)
        case .keyboardEnd: code = UInt32(ZZ_KEY_END.rawValue)
        case .keyboardPageUp: code = UInt32(ZZ_KEY_PAGE_UP.rawValue)
        case .keyboardPageDown: code = UInt32(ZZ_KEY_PAGE_DOWN.rawValue)
        case .keyboardUpArrow: code = UInt32(ZZ_KEY_ARROW_UP.rawValue)
        case .keyboardDownArrow: code = UInt32(ZZ_KEY_ARROW_DOWN.rawValue)
        case .keyboardLeftArrow: code = UInt32(ZZ_KEY_ARROW_LEFT.rawValue)
        case .keyboardRightArrow: code = UInt32(ZZ_KEY_ARROW_RIGHT.rawValue)
        default: code = nil
        }
        if let code {
            return (code, 0)
        }
        let hasTerminalModifier = key.modifierFlags.contains(.control) ||
            key.modifierFlags.contains(.alternate) ||
            key.modifierFlags.contains(.command)
        guard hasTerminalModifier,
              key.charactersIgnoringModifiers.unicodeScalars.count == 1,
              let scalar = key.charactersIgnoringModifiers.unicodeScalars.first else {
            return nil
        }
        return (UInt32(ZZ_KEY_CHARACTER.rawValue), scalar.value)
    }
}

struct TerminalSurface: UIViewRepresentable {
    @ObservedObject var store: ZZStore
    let pane: UInt64
    let frame: TerminalFrame?
    let interactive: Bool
    let preview: Bool

    func makeUIView(context: Context) -> TerminalGridView {
        let view = TerminalGridView()
        configure(view)
        return view
    }

    func updateUIView(_ view: TerminalGridView, context: Context) {
        configure(view)
    }

    static func dismantleUIView(_ view: TerminalGridView, coordinator: ()) {
        view.deactivateInput()
    }

    private func configure(_ view: TerminalGridView) {
        view.pane = pane
        view.onText = { text in store.sendText(text, to: pane) }
        view.onKey = { code, scalar, modifiers, action in
            store.sendKey(code, to: pane, codepoint: scalar, action: UInt32(action), modifiers: modifiers)
        }
        view.onResize = { layout, stable in
            store.resize(pane: pane, layout: layout, stable: stable)
        }
        view.onScroll = { lines in store.scroll(pane: pane, lines: lines) }
        view.onFontSizeStep = { step in store.setTerminalFontSizeStep(step, for: pane) }
        view.onFocus = { store.requestKeyboard(for: pane) }
        view.configure(
            frame: frame,
            interactive: interactive,
            preview: preview,
            fontSizeStep: store.terminalFontSizeStep(for: pane),
            inputRequested: interactive && store.terminalInput.owner.owns(pane),
            sceneActive: store.sceneIsActive,
            inputActivation: store.terminalInput.activation
        )
    }
}
