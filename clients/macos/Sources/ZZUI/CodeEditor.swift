import AppKit
import SwiftUI

public enum ZZSyntaxKind: String, CaseIterable, Codable, Sendable {
    case keyword, string, number, comment, function, type, constant, property, punctuation

    public func color(dark: Bool) -> ZZColor {
        let hex: String
        switch self {
        case .keyword: hex = dark ? "c28b12" : "0433ff"
        case .string: hex = dark ? "62ba46" : "036a07"
        case .number, .constant: hex = dark ? "e1d797" : "0433ff"
        case .comment: hex = dark ? "9e9e9e" : "007fff"
        case .function: hex = dark ? "fdd888" : "0000a2"
        case .type: hex = dark ? "c75828" : "6f42c1"
        case .property, .punctuation: hex = dark ? "caccca" : "333333"
        }
        return ZZColor(hex: hex)!
    }
}

public struct ZZSyntaxSpan: Equatable, Sendable {
    public let range: NSRange
    public let kind: ZZSyntaxKind

    public init(range: NSRange, kind: ZZSyntaxKind) {
        self.range = range
        self.kind = kind
    }

    public init?(utf8Range: Range<Int>, in source: String, kind: ZZSyntaxKind) {
        guard utf8Range.lowerBound >= 0, utf8Range.upperBound <= source.utf8.count,
            let lower = source.utf8.index(source.utf8.startIndex, offsetBy: utf8Range.lowerBound).samePosition(
                in: source),
            let upper = source.utf8.index(source.utf8.startIndex, offsetBy: utf8Range.upperBound).samePosition(
                in: source)
        else { return nil }
        self.init(range: NSRange(lower..<upper, in: source), kind: kind)
    }
}

public enum ZZSyntaxHighlighter {
    public static func attributedString(_ source: String, spans: [ZZSyntaxSpan], dark: Bool) -> AttributedString {
        var result = AttributedString(source)
        for span in spans {
            guard let range = Range(span.range, in: source),
                let lower = AttributedString.Index(range.lowerBound, within: result),
                let upper = AttributedString.Index(range.upperBound, within: result)
            else { continue }
            result[lower..<upper].foregroundColor = span.kind.color(dark: dark).color
        }
        return result
    }
}

public enum ZZTextSelection {
    public static func clamped(_ range: NSRange, toUTF16Length length: Int) -> NSRange {
        let length = max(0, length)
        let start = min(max(0, range.location), length)
        return NSRange(location: start, length: min(max(0, range.length), length - start))
    }

    public static func lineStarts(in source: String) -> [Int] {
        var starts = [0]
        let text = source as NSString
        var position = 0
        while position < text.length {
            let end = NSMaxRange(text.lineRange(for: NSRange(location: position, length: 0)))
            guard end > position else { break }
            if end < text.length || text.character(at: end - 1) == 10 || text.character(at: end - 1) == 13 {
                starts.append(end)
            }
            position = end
        }
        return starts
    }
}

public struct ZZCodeEditor: NSViewRepresentable {
    @Environment(\.zzTheme) private var theme
    @Environment(\.isEnabled) private var isEnabled
    @Binding private var text: String
    private let selection: Binding<NSRange>?
    private let spans: [ZZSyntaxSpan]
    private let fontSize: CGFloat
    private let showsLineNumbers: Bool
    private let softWrap: Bool
    private let isEditable: Bool
    private let tabWidth: Int
    private let hardTabs: Bool
    private let placeholder: String
    private let relativeLineNumbers: Bool

    public init(
        text: Binding<String>, selection: Binding<NSRange>? = nil, spans: [ZZSyntaxSpan] = [],
        fontSize: CGFloat = 13, showsLineNumbers: Bool = true, softWrap: Bool = true,
        isEditable: Bool = true, tabWidth: Int = 2, hardTabs: Bool = false,
        placeholder: String = "", relativeLineNumbers: Bool = false
    ) {
        _text = text
        self.selection = selection
        self.spans = spans
        self.fontSize = fontSize
        self.showsLineNumbers = showsLineNumbers
        self.softWrap = softWrap
        self.isEditable = isEditable
        self.tabWidth = max(1, tabWidth)
        self.hardTabs = hardTabs
        self.placeholder = placeholder
        self.relativeLineNumbers = relativeLineNumbers
    }

    public func makeNSView(context: Context) -> NSScrollView {
        let storage = NSTextStorage()
        let layout = NSLayoutManager()
        let container = NSTextContainer(containerSize: NSSize(width: 0, height: CGFloat.greatestFiniteMagnitude))
        storage.addLayoutManager(layout)
        layout.addTextContainer(container)
        let view = ZZNativeCodeTextView(frame: .zero, textContainer: container)
        view.isRichText = false
        view.importsGraphics = false
        view.allowsUndo = true
        view.isAutomaticQuoteSubstitutionEnabled = false
        view.isAutomaticDashSubstitutionEnabled = false
        view.isAutomaticTextReplacementEnabled = false
        view.isAutomaticSpellingCorrectionEnabled = false
        view.isContinuousSpellCheckingEnabled = false
        view.isAutomaticLinkDetectionEnabled = false
        view.isGrammarCheckingEnabled = false
        view.usesFindBar = true
        view.isIncrementalSearchingEnabled = true
        view.isVerticallyResizable = true
        view.minSize = NSSize(width: 0, height: 0)
        view.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        view.textContainerInset = NSSize(width: 8, height: 10)
        view.clipsToBounds = true
        view.autoresizingMask = [.width]
        view.setAccessibilityLabel("Code editor")
        view.delegate = context.coordinator
        let scroll = ZZCodeScrollView()
        scroll.clipsToBounds = true
        scroll.documentView = view
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.drawsBackground = false
        let ruler = ZZLineNumberRuler(scrollView: scroll, orientation: .verticalRuler)
        ruler.clipsToBounds = true
        ruler.clientView = view
        scroll.verticalRulerView = ruler
        context.coordinator.ruler = ruler
        context.coordinator.textView = view
        return scroll
    }

    public func updateNSView(_ scroll: NSScrollView, context: Context) {
        guard let view = scroll.documentView as? ZZNativeCodeTextView else { return }
        let coordinator = context.coordinator
        coordinator.parent = self
        coordinator.applying = true
        defer { coordinator.applying = false }
        let changedText = view.string != text
        if changedText && !view.hasMarkedText() {
            let previous = view.selectedRange()
            view.breakUndoCoalescing()
            view.undoManager?.removeAllActions(withTarget: view)
            if let storage = view.textStorage { view.undoManager?.removeAllActions(withTarget: storage) }
            view.string = text
            view.setSelectedRange(ZZTextSelection.clamped(previous, toUTF16Length: text.utf16.count))
        }
        if let selection, !view.hasMarkedText() {
            let desired = ZZTextSelection.clamped(selection.wrappedValue, toUTF16Length: view.string.utf16.count)
            if view.selectedRange() != desired { view.setSelectedRange(desired) }
        }
        view.isEditable = isEditable && isEnabled
        view.isSelectable = true
        view.backgroundColor = theme.background.nsColor
        view.insertionPointColor = theme.foreground.nsColor
        if view.textColor != theme.foreground.nsColor { view.textColor = theme.foreground.nsColor }
        let font = theme.nsFont(size: fontSize, monospaced: true)
        if view.font != font { view.font = font }
        view.tabWidth = tabWidth
        view.hardTabs = hardTabs
        view.placeholder = placeholder
        view.placeholderColor = theme.foreground.muted().nsColor
        let paragraph = NSMutableParagraphStyle()
        paragraph.defaultTabInterval =
            (" " as NSString).size(withAttributes: [.font: view.font!]).width * CGFloat(tabWidth)
        paragraph.tabStops = []
        view.defaultParagraphStyle = paragraph
        view.typingAttributes = [
            .font: view.font!, .foregroundColor: theme.foreground.nsColor, .paragraphStyle: paragraph,
        ]
        view.isHorizontallyResizable = !softWrap
        view.textContainer?.widthTracksTextView = softWrap
        if !softWrap { view.textContainer?.containerSize.width = .greatestFiniteMagnitude }
        scroll.hasHorizontalScroller = !softWrap
        scroll.hasVerticalRuler = showsLineNumbers
        scroll.rulersVisible = showsLineNumbers
        coordinator.ruler?.foreground = theme.foreground.muted().nsColor
        coordinator.ruler?.background = theme.background.nsColor
        coordinator.ruler?.font = theme.nsFont(size: fontSize - 2, monospaced: true)
        coordinator.ruler?.relativeLineNumbers = relativeLineNumbers
        coordinator.ruler?.updateLines()
        coordinator.applyHighlights()
    }

    public func makeCoordinator() -> Coordinator { Coordinator(self) }

    @MainActor public final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: ZZCodeEditor
        var applying = false
        weak var textView: ZZNativeCodeTextView?
        weak var ruler: ZZLineNumberRuler?
        private var highlightedText: String?
        private var highlightedSpans: [ZZSyntaxSpan] = []
        private var highlightedDark: Bool?

        init(_ parent: ZZCodeEditor) { self.parent = parent }

        public func textDidChange(_ notification: Notification) {
            guard !applying, let view = notification.object as? NSTextView else { return }
            parent.text = view.string
            parent.selection?.wrappedValue = view.selectedRange()
            ruler?.updateLines()
            if !view.hasMarkedText() { applyHighlights() }
        }

        public func textViewDidChangeSelection(_ notification: Notification) {
            guard !applying, let view = notification.object as? NSTextView else { return }
            if parent.selection?.wrappedValue != view.selectedRange() {
                parent.selection?.wrappedValue = view.selectedRange()
            }
            ruler?.needsDisplay = true
        }

        func applyHighlights() {
            guard let view = textView, !view.hasMarkedText(), let layout = view.layoutManager else { return }
            let dark = parent.theme.background.lightness < 0.5
            guard highlightedText != view.string || highlightedSpans != parent.spans || highlightedDark != dark else {
                return
            }
            let range = NSRange(location: 0, length: view.string.utf16.count)
            layout.removeTemporaryAttribute(.foregroundColor, forCharacterRange: range)
            for span in parent.spans where Range(span.range, in: view.string) != nil {
                layout.addTemporaryAttribute(
                    .foregroundColor, value: span.kind.color(dark: dark).nsColor, forCharacterRange: span.range)
            }
            highlightedText = view.string
            highlightedSpans = parent.spans
            highlightedDark = dark
        }
    }
}

final class ZZNativeCodeTextView: NSTextView {
    var tabWidth = 2
    var hardTabs = false
    var placeholder = "" { didSet { if placeholder != oldValue { needsDisplay = true } } }
    var placeholderColor = NSColor.secondaryLabelColor

    override func insertTab(_ sender: Any?) {
        guard isEditable else { return }
        guard !hasMarkedText() else {
            super.insertTab(sender)
            return
        }
        if selectedRange().length == 0 {
            insertText(hardTabs ? "\t" : String(repeating: " ", count: tabWidth), replacementRange: selectedRange())
        } else {
            indentLines(outdent: false)
        }
    }

    override func insertBacktab(_ sender: Any?) {
        guard isEditable else { return }
        guard !hasMarkedText() else {
            super.insertBacktab(sender)
            return
        }
        indentLines(outdent: true)
    }

    private func indentLines(outdent: Bool) {
        let selection = selectedRange()
        let range = (string as NSString).lineRange(
            for: NSRange(location: selection.location, length: max(0, selection.length - 1)))
        let source = (string as NSString).substring(with: range)
        let lines = source.components(separatedBy: "\n")
        let indent = hardTabs ? "\t" : String(repeating: " ", count: tabWidth)
        let replacement = lines.enumerated().map { index, line in
            if index == lines.count - 1 && line.isEmpty && source.hasSuffix("\n") { return line }
            if !outdent { return indent + line }
            if line.hasPrefix("\t") { return String(line.dropFirst()) }
            let count = min(tabWidth, line.prefix { $0 == " " }.count)
            return String(line.dropFirst(count))
        }.joined(separator: "\n")
        guard replacement != source else { return }
        insertText(replacement, replacementRange: range)
        if outdent {
            let removed = source.utf16.count - replacement.utf16.count
            setSelectedRange(NSRange(location: max(range.location, selection.location - removed), length: 0))
        } else {
            setSelectedRange(NSRange(location: range.location, length: replacement.utf16.count))
        }
    }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        guard string.isEmpty, !hasMarkedText(), !placeholder.isEmpty, let font else { return }
        let rect = bounds.insetBy(
            dx: textContainerInset.width + (textContainer?.lineFragmentPadding ?? 0), dy: textContainerInset.height)
        (placeholder as NSString).draw(in: rect, withAttributes: [.font: font, .foregroundColor: placeholderColor])
    }

    override func insertNewline(_ sender: Any?) {
        guard isEditable, !hasMarkedText() else {
            super.insertNewline(sender)
            return
        }
        let string = self.string as NSString
        let range = string.lineRange(for: selectedRange())
        let line = string.substring(with: range)
        let indentation = String(line.prefix { $0 == " " || $0 == "\t" })
        insertText("\n" + indentation, replacementRange: selectedRange())
    }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if window?.firstResponder === self, isEditable, !hasMarkedText(),
            event.modifierFlags.intersection(.deviceIndependentFlagsMask) == .command
        {
            if event.charactersIgnoringModifiers == "]" {
                indentLines(outdent: false)
                return true
            }
            if event.charactersIgnoringModifiers == "[" {
                indentLines(outdent: true)
                return true
            }
        }
        if window?.firstResponder === self,
            event.modifierFlags.intersection(.deviceIndependentFlagsMask) == .command,
            event.charactersIgnoringModifiers == "f"
        {
            let item = NSMenuItem()
            item.tag = Int(NSFindPanelAction.showFindPanel.rawValue)
            performFindPanelAction(item)
            return true
        }
        return super.performKeyEquivalent(with: event)
    }
}

private final class ZZCodeScrollView: NSScrollView {
    override func tile() {
        super.tile()
        guard let view = documentView as? NSTextView else { return }
        view.minSize = NSSize(width: contentSize.width, height: contentSize.height)
        if view.textContainer?.widthTracksTextView == true, view.frame.width != contentSize.width {
            view.setFrameSize(NSSize(width: contentSize.width, height: max(contentSize.height, view.frame.height)))
        }
    }
}

final class ZZLineNumberRuler: NSRulerView {
    var foreground = NSColor.secondaryLabelColor
    var background = NSColor.textBackgroundColor
    var font = NSFont.monospacedDigitSystemFont(ofSize: 11, weight: .regular)
    var relativeLineNumbers = false
    private var starts = [0]
    private var source = ""

    func updateLines() {
        guard let view = clientView as? NSTextView else { return }
        if source != view.string {
            source = view.string
            starts = ZZTextSelection.lineStarts(in: source)
        }
        let width = ("\(starts.count)" as NSString).size(withAttributes: [.font: font]).width + 22
        ruleThickness = max(38, width)
        needsDisplay = true
    }

    override func drawHashMarksAndLabels(in rect: NSRect) {
        background.setFill()
        bounds.intersection(rect).fill()
        guard let view = clientView as? NSTextView, let layout = view.layoutManager,
            let container = view.textContainer
        else { return }
        let visible = view.visibleRect
        let visibleGlyphs = layout.glyphRange(
            forBoundingRect: visible.offsetBy(dx: -view.textContainerOrigin.x, dy: -view.textContainerOrigin.y),
            in: container)
        let visibleCharacters = layout.characterRange(forGlyphRange: visibleGlyphs, actualGlyphRange: nil)
        var first = starts.partitioningIndex { $0 >= visibleCharacters.location }
        if first > 0 { first -= 1 }
        let current = starts.partitioningIndex { $0 > view.selectedRange().location } - 1
        for index in first..<starts.count {
            let start = starts[index]
            if start > NSMaxRange(visibleCharacters) { break }
            let lineRect: NSRect
            if start == (source as NSString).length {
                lineRect = layout.extraLineFragmentRect
            } else {
                let glyph = layout.glyphIndexForCharacter(at: start)
                lineRect = layout.lineFragmentRect(forGlyphAt: glyph, effectiveRange: nil)
            }
            let point = convert(NSPoint(x: 0, y: lineRect.minY + view.textContainerOrigin.y), from: view)
            let number = relativeLineNumbers && index != current ? abs(index - current) : index + 1
            let label = "\(number)" as NSString
            let attrs: [NSAttributedString.Key: Any] = [
                .font: font, .foregroundColor: index == current ? (view.insertionPointColor ?? foreground) : foreground,
            ]
            let size = label.size(withAttributes: attrs)
            label.draw(
                at: NSPoint(
                    x: ruleThickness - size.width - 10, y: point.y + max(0, (lineRect.height - size.height) / 2)),
                withAttributes: attrs)
        }
    }
}

extension Array where Element == Int {
    fileprivate func partitioningIndex(where predicate: (Int) -> Bool) -> Int {
        var lower = 0
        var upper = count
        while lower < upper {
            let middle = (lower + upper) / 2
            if predicate(self[middle]) { upper = middle } else { lower = middle + 1 }
        }
        return lower
    }
}
