import SwiftUI
import UIKit

enum TerminalFontZoom {
    static let minimumPointSize: CGFloat = 9
    static let maximumPointSize: CGFloat = 23
    static let defaultPointSize: CGFloat = 13

    static func clampedPointSize(_ pointSize: CGFloat) -> CGFloat {
        min(max(pointSize, minimumPointSize), maximumPointSize)
    }

    static func clamped(
        _ step: Int,
        basePointSize: CGFloat = defaultPointSize
    ) -> Int {
        let basePointSize = clampedPointSize(basePointSize)
        let minimumStep = Int(ceil(minimumPointSize - basePointSize))
        let maximumStep = Int(floor(maximumPointSize - basePointSize))
        return min(max(step, minimumStep), maximumStep)
    }

    static func pointSize(
        for step: Int,
        basePointSize: CGFloat = defaultPointSize
    ) -> CGFloat {
        let basePointSize = clampedPointSize(basePointSize)
        return basePointSize + CGFloat(clamped(step, basePointSize: basePointSize))
    }

    static func targetStep(
        anchor: Int,
        scale: CGFloat,
        basePointSize: CGFloat = defaultPointSize
    ) -> Int {
        let basePointSize = clampedPointSize(basePointSize)
        let scaledPointSize = pointSize(for: anchor, basePointSize: basePointSize)
            * max(scale, 0.01)
        return clamped(
            Int((scaledPointSize - basePointSize).rounded()),
            basePointSize: basePointSize
        )
    }

    static func crossedSteps(from current: Int, to target: Int) -> [Int] {
        guard current != target else {
            return []
        }
        let direction = target > current ? 1 : -1
        return Array(stride(from: current + direction, through: target, by: direction))
    }
}

enum TerminalBlinkPolicy {
    static func cursorShouldAnimate(
        cursorActive: Bool,
        frameRequestsBlink: Bool,
        cursorBlinking: Bool
    ) -> Bool {
        cursorActive && frameRequestsBlink && cursorBlinking
    }

    static func shouldRunTimer(
        interactive: Bool,
        cursorActive: Bool,
        cursorRequestsBlink: Bool,
        blinkingText: Bool,
        cursorBlinking: Bool
    ) -> Bool {
        interactive && (
            blinkingText || cursorShouldAnimate(
                cursorActive: cursorActive,
                frameRequestsBlink: cursorRequestsBlink,
                cursorBlinking: cursorBlinking
            )
        )
    }

    /// Sorted rows containing at least one cell whose style blinks.
    static func blinkingRows(
        cellStyleIndices: [Int],
        columns: Int,
        rowCount: Int,
        styleAttributes: [UInt16]
    ) -> [Int] {
        guard columns > 0, rowCount > 0 else {
            return []
        }
        var rows: [Int] = []
        for row in 0..<rowCount {
            var found = false
            for column in 0..<columns {
                let index = row * columns + column
                guard cellStyleIndices.indices.contains(index) else {
                    break
                }
                let styleIndex = cellStyleIndices[index]
                guard styleAttributes.indices.contains(styleIndex) else {
                    continue
                }
                if styleAttributes[styleIndex] & UInt16(ZZ_ATTR_BLINK) != 0 {
                    found = true
                    break
                }
            }
            if found {
                rows.append(row)
            }
        }
        return rows
    }

    /// Display rects covering one blink tick: full-width bands for contiguous
    /// runs of blinking rows plus the animating cursor cell. The cursor rect is
    /// omitted when a band already covers it.
    static func blinkDirtyRects(
        cursorColumn: Int?,
        cursorRow: Int?,
        cursorAnimates: Bool,
        blinkingRows: [Int],
        columns: Int,
        rowCount: Int,
        cellSize: CGSize,
        boundsWidth: CGFloat
    ) -> [CGRect] {
        guard cellSize.width > 0, cellSize.height > 0, columns > 0, rowCount > 0 else {
            return []
        }
        let rows = Array(Set(blinkingRows.filter { $0 >= 0 && $0 < rowCount })).sorted()
        var rects: [CGRect] = []
        var runStart: Int?
        var runEnd = 0
        for row in rows {
            if runStart != nil, row == runEnd + 1 {
                runEnd = row
            } else {
                if let start = runStart {
                    rects.append(CGRect(
                        x: 0,
                        y: CGFloat(start) * cellSize.height,
                        width: boundsWidth,
                        height: CGFloat(runEnd - start + 1) * cellSize.height
                    ))
                }
                runStart = row
                runEnd = row
            }
        }
        if let start = runStart {
            rects.append(CGRect(
                x: 0,
                y: CGFloat(start) * cellSize.height,
                width: boundsWidth,
                height: CGFloat(runEnd - start + 1) * cellSize.height
            ))
        }
        if cursorAnimates,
           let cursorColumn,
           let cursorRow,
           cursorColumn >= 0, cursorColumn < columns,
           cursorRow >= 0, cursorRow < rowCount
        {
            let cursorRect = CGRect(
                x: CGFloat(cursorColumn) * cellSize.width,
                y: CGFloat(cursorRow) * cellSize.height,
                width: cellSize.width,
                height: cellSize.height
            )
            if !rects.contains(where: { $0.contains(cursorRect) }) {
                rects.append(cursorRect)
            }
        }
        return rects
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
    var onSelection: ((UInt32, UInt16, UInt16, Bool) -> Void)?

    private var viewport: TerminalFrame?
    private var interactive = false
    private var preview = false
    private var fontSize: CGFloat = 0
    private var terminalFont: ZZTerminalFont?
    private var basePointSize = TerminalFontZoom.defaultPointSize
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
    private var cursorActive = false
    private var cursorBlinking = true
    private var blinkTimer: Timer?
    private var blinkingRows: [Int] = []
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
        let selection = UILongPressGestureRecognizer(target: self, action: #selector(selectText(_:)))
        selection.minimumPressDuration = 0.35
        pan.require(toFail: selection)
        addGestureRecognizer(selection)
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
        terminalFont: ZZTerminalFont,
        basePointSize: CGFloat,
        cursorActive: Bool,
        cursorBlinking: Bool,
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
        let basePointSize = TerminalFontZoom.clampedPointSize(basePointSize)
        let fontChanged = self.terminalFont != terminalFont ||
            abs(self.basePointSize - basePointSize) > 0.01
        let previewChanged = self.preview != preview
        let cursorActiveChanged = self.cursorActive != cursorActive
        let cursorBlinkingChanged = self.cursorBlinking != cursorBlinking
        viewport = frame
        if generationChanged {
            blinkingRows = frame.map {
                TerminalBlinkPolicy.blinkingRows(
                    cellStyleIndices: $0.cells.map { Int($0.style) },
                    columns: $0.columns,
                    rowCount: $0.rows,
                    styleAttributes: $0.styles.map { $0.attributes }
                )
            } ?? []
        }
        self.interactive = interactive
        self.preview = preview
        self.cursorActive = cursorActive
        self.cursorBlinking = cursorBlinking
        self.inputRequested = inputRequested
        self.sceneActive = sceneActive
        self.inputActivation = inputActivation
        if !sceneActive {
            Self.softwareKeyboardVisible = false
            viewportIsStable = true
        } else if Self.softwareKeyboardVisible {
            viewportIsStable = false
        }
        if fontChanged {
            self.terminalFont = terminalFont
            self.basePointSize = basePointSize
            fontSize = 0
            lastResize = nil
        }
        if previewChanged {
            fontSize = 0
        }
        let nextFontSizeStep = TerminalFontZoom.clamped(
            fontSizeStep,
            basePointSize: basePointSize
        )
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
        if fontChanged || previewChanged || cursorActiveChanged || cursorBlinkingChanged {
            setNeedsDisplay()
        } else if generationChanged, let frame, !frame.damage.all {
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

    override func pressesChanged(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        var forwarded = false
        for press in presses {
            guard let key = press.key, let mapped = map(key) else {
                continue
            }
            forwarded = true
            onKey?(mapped.code, mapped.scalar, modifierBits(key.modifierFlags), UInt8(ZZ_KEY_REPEAT.rawValue))
        }
        if !forwarded {
            super.pressesChanged(presses, with: event)
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

    @objc private func selectText(_ gesture: UILongPressGestureRecognizer) {
        guard interactive, let viewport, viewport.columns > 0, viewport.rows > 0 else {
            return
        }
        let location = gesture.location(in: self)
        let column = min(
            max(Int(floor(location.x / max(measuredCell.width, 1))), 0),
            viewport.columns - 1
        )
        let row = min(
            max(Int(floor(location.y / max(measuredCell.height, 1))), 0),
            viewport.rows - 1
        )
        let phase: UInt32
        switch gesture.state {
        case .began:
            phase = 0
            UISelectionFeedbackGenerator().selectionChanged()
        case .changed:
            phase = 1
        case .ended, .cancelled:
            phase = 2
        default:
            return
        }
        onSelection?(
            phase,
            UInt16(clamping: column),
            UInt16(clamping: row),
            false
        )
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
            let target = TerminalFontZoom.targetStep(
                anchor: pinchAnchorStep,
                scale: gesture.scale,
                basePointSize: basePointSize
            )
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
        guard let terminalFont else {
            return
        }
        let baseSize = TerminalFontZoom.pointSize(
            for: fontSizeStep,
            basePointSize: basePointSize
        )
        let baseFont = terminalFont.uiFont(size: baseSize, bold: false, italic: false)
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
        regularFont = terminalFont.uiFont(size: nextSize, bold: false, italic: false)
        boldFont = terminalFont.uiFont(size: nextSize, bold: true, italic: false)
        italicFont = terminalFont.uiFont(size: nextSize, bold: false, italic: true)
        boldItalicFont = terminalFont.uiFont(size: nextSize, bold: true, italic: true)
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
        let cursorRequestsBlink = viewport?.cursor.map {
            $0.visible != 0 && $0.blinking != 0
        } ?? false
        let shouldRun = TerminalBlinkPolicy.shouldRunTimer(
            interactive: interactive,
            cursorActive: cursorActive,
            cursorRequestsBlink: cursorRequestsBlink,
            blinkingText: !blinkingRows.isEmpty,
            cursorBlinking: cursorBlinking
        )
        guard shouldRun, window != nil else {
            let needsRedraw = !cursorVisible
            blinkTimer?.invalidate()
            blinkTimer = nil
            cursorVisible = true
            if needsRedraw {
                setNeedsDisplay()
            }
            return
        }
        guard blinkTimer == nil else {
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
        let rects = currentBlinkDirtyRects()
        if rects.isEmpty {
            setNeedsDisplay()
        } else {
            for rect in rects {
                setNeedsDisplay(rect)
            }
        }
    }

    /// Dirty region for one blink tick: the animating cursor cell plus rows
    /// holding ANSI-blinking text. Narrower than a full-surface redraw; the
    /// draw path already clips to the invalidated rect.
    private func currentBlinkDirtyRects() -> [CGRect] {
        guard let viewport, viewport.columns > 0, viewport.rows > 0 else {
            return []
        }
        let cursorAnimates = viewport.cursor.map {
            TerminalBlinkPolicy.cursorShouldAnimate(
                cursorActive: cursorActive,
                frameRequestsBlink: $0.blinking != 0,
                cursorBlinking: cursorBlinking
            )
        } ?? false
        return TerminalBlinkPolicy.blinkDirtyRects(
            cursorColumn: viewport.cursor.map { Int($0.column) },
            cursorRow: viewport.cursor.map { Int($0.row) },
            cursorAnimates: cursorAnimates,
            blinkingRows: blinkingRows,
            columns: viewport.columns,
            rowCount: viewport.rows,
            cellSize: measuredCell,
            boundsWidth: bounds.width
        )
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
        guard let cursor = viewport.cursor else {
            return
        }
        let shouldAnimate = TerminalBlinkPolicy.cursorShouldAnimate(
            cursorActive: cursorActive,
            frameRequestsBlink: cursor.blinking != 0,
            cursorBlinking: cursorBlinking
        )
        guard
            cursor.visible != 0,
            !shouldAnimate || cursorVisible
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
        Self.map(
            keyCode: key.keyCode,
            charactersIgnoringModifiers: key.charactersIgnoringModifiers,
            modifierFlags: key.modifierFlags
        )
    }

    static func map(
        keyCode: UIKeyboardHIDUsage,
        charactersIgnoringModifiers: String,
        modifierFlags: UIKeyModifierFlags
    ) -> (code: UInt32, scalar: UInt32)? {
        let code: UInt32?
        switch keyCode {
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
        let hasTerminalModifier = modifierFlags.contains(.control) ||
            modifierFlags.contains(.alternate) ||
            modifierFlags.contains(.command)
        guard hasTerminalModifier,
              charactersIgnoringModifiers.unicodeScalars.count == 1,
              let scalar = charactersIgnoringModifiers.unicodeScalars.first else {
            return nil
        }
        return (UInt32(ZZ_KEY_CHARACTER.rawValue), scalar.value)
    }
}

struct LiveTerminalSurface: View {
    @ObservedObject private var frameSlot: TerminalFrameSlot
    private let store: ZZStore
    private let pane: UInt64
    private let interactive: Bool
    private let preview: Bool

    init(
        store: ZZStore,
        pane: UInt64,
        interactive: Bool,
        preview: Bool
    ) {
        self.store = store
        self.pane = pane
        self.interactive = interactive
        self.preview = preview
        _frameSlot = ObservedObject(wrappedValue: store.frameSlot(for: pane))
    }

    var body: some View {
        TerminalSurface(
            store: store,
            pane: pane,
            frame: frameSlot.frame,
            interactive: interactive,
            preview: preview
        )
    }
}

struct TerminalSurface: UIViewRepresentable {
    @Environment(\.zzTerminalPresentation) private var terminalPresentation
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
        view.onSelection = { phase, column, row, rectangle in
            store.updateSelection(
                pane: pane,
                phase: phase,
                column: column,
                row: row,
                rectangle: rectangle
            )
        }
        view.configure(
            frame: frame,
            interactive: interactive,
            preview: preview,
            fontSizeStep: store.terminalFontSizeStep(for: pane),
            terminalFont: terminalPresentation.font,
            basePointSize: terminalPresentation.pointSize,
            cursorActive: cursorActive,
            cursorBlinking: terminalPresentation.cursorBlinking,
            inputRequested: interactive && store.terminalInput.owner.owns(pane),
            sceneActive: store.sceneIsActive,
            inputActivation: store.terminalInput.activation
        )
    }

    private var cursorActive: Bool {
        guard interactive else {
            return false
        }
        if let selectedPaneID = store.selectedPaneID {
            return selectedPaneID == pane
        }
        return store.selectedSession?.activeWindow?.activePane == pane
    }
}
