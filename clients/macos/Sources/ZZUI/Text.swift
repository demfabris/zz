import AppKit
import SwiftUI

public struct ZZMarkdownBlock: Identifiable {
    public let id: Int
    public var content: AttributedString
    public let intents: [PresentationIntent.IntentType]
}

public struct ZZMarkdownDocument {
    public let blocks: [ZZMarkdownBlock]

    public var imageURLs: Set<URL> {
        Set(blocks.flatMap { $0.content.runs.compactMap(\.imageURL) })
    }

    public init(_ source: String) {
        let parsed = (try? AttributedString(markdown: source)) ?? AttributedString(source)
        var blocks: [ZZMarkdownBlock] = []
        for run in parsed.runs {
            let components = run.presentationIntent?.components ?? []
            let id = components.first?.identity ?? 0
            let content = AttributedString(parsed[run.range])
            if blocks.last?.id == id {
                blocks[blocks.count - 1].content.append(content)
            } else {
                blocks.append(ZZMarkdownBlock(id: id, content: content, intents: components))
            }
        }
        self.blocks = blocks
    }

    @MainActor public func attributedString(
        theme: ZZTheme, fontSize: CGFloat = 13, images: [URL: NSImage] = [:]
    ) -> NSAttributedString {
        let result = NSMutableAttributedString(string: "")
        var seenListItems: Set<Int> = []
        var tables: [Int: NSTextTable] = [:]
        for block in blocks {
            var content = block.content
            let paragraph = NSMutableParagraphStyle()
            paragraph.paragraphSpacing = 16
            paragraph.lineSpacing = 3
            var size = fontSize
            var weight = NSFont.Weight.regular
            var monospaced = false
            var prefix = ""
            var foreground = theme.foreground.nsColor
            let listDepth = block.intents.filter {
                switch $0.kind {
                case .orderedList, .unorderedList: true
                default: false
                }
            }.count
            if listDepth > 0 {
                paragraph.headIndent = CGFloat(listDepth) * 20
                paragraph.firstLineHeadIndent = CGFloat(listDepth - 1) * 20
                paragraph.paragraphSpacing = 5
                paragraph.tabStops = [NSTextTab(textAlignment: .left, location: paragraph.headIndent)]
            }
            if let item = block.intents.first(where: {
                if case .listItem = $0.kind { return true }
                return false
            }), case .listItem(let ordinal) = item.kind {
                if seenListItems.insert(item.identity).inserted {
                    let nearestList = block.intents.first {
                        $0.kind == .orderedList || $0.kind == .unorderedList
                    }
                    if nearestList?.kind == .orderedList {
                        if listDepth == 1 {
                            prefix = "\(ordinal).\t"
                        } else {
                            let base = listDepth == 2 ? 65 : 97
                            let letter = UnicodeScalar(base + max(0, ordinal - 1) % 26)!
                            prefix = "\(letter).\t"
                        }
                    } else {
                        prefix = ["•", "◦", "▪", "‣", "⁃"][min(max(0, listDepth - 1), 4)] + "\t"
                    }
                    let start = String(content.characters.prefix(4))
                    if start == "[ ] " || start.lowercased() == "[x] " {
                        prefix = start == "[ ] " ? "☐\t" : "☑\t"
                        let end = content.characters.index(content.startIndex, offsetBy: 4)
                        content = AttributedString(content[end..<content.endIndex])
                    }
                } else {
                    paragraph.firstLineHeadIndent = paragraph.headIndent
                }
            }
            for intent in block.intents {
                switch intent.kind {
                case .header(let level):
                    size = [28, 21, 17.5, 15.75, 14, 14][min(max(0, level - 1), 5)]
                    weight = level == 1 ? .bold : (level == 6 ? .medium : .semibold)
                    paragraph.paragraphSpacing = 4.8
                case .codeBlock:
                    monospaced = true
                    size = fontSize - 1
                    let textBlock = NSTextBlock()
                    textBlock.backgroundColor = theme.background.raised(1).nsColor
                    textBlock.setWidth(10, type: .absoluteValueType, for: .padding)
                    textBlock.setWidth(0.5, type: .absoluteValueType, for: .border)
                    textBlock.setBorderColor(theme.border.nsColor)
                    paragraph.textBlocks = [textBlock]
                    paragraph.lineSpacing = 2
                case .blockQuote:
                    let quote = NSTextBlock()
                    quote.setWidth(3, type: .absoluteValueType, for: .border, edge: .minX)
                    quote.setWidth(16, type: .absoluteValueType, for: .padding, edge: .minX)
                    quote.setBorderColor(theme.background.raised(2).active().nsColor, for: .minX)
                    paragraph.textBlocks.append(quote)
                    foreground = theme.foreground.muted().nsColor
                case .thematicBreak:
                    let rule = NSTextBlock()
                    rule.setWidth(0.5, type: .absoluteValueType, for: .border, edge: .minY)
                    rule.setBorderColor(theme.border.nsColor)
                    paragraph.textBlocks = [rule]
                    content = AttributedString("\u{200b}")
                case .table(let columns):
                    if tables[intent.identity] == nil {
                        let table = NSTextTable()
                        table.numberOfColumns = columns.count
                        table.collapsesBorders = true
                        table.layoutAlgorithm = .automaticLayoutAlgorithm
                        table.setValue(100, type: .percentageValueType, for: .width)
                        tables[intent.identity] = table
                    }
                case .tableHeaderRow:
                    weight = .semibold
                default: break
                }
            }
            if let tableIntent = block.intents.first(where: {
                if case .table = $0.kind { return true }
                return false
            }), let table = tables[tableIntent.identity] {
                let row =
                    block.intents.compactMap { intent -> Int? in
                        if case .tableRow(let row) = intent.kind { return row }
                        return nil
                    }.first ?? 0
                let column =
                    block.intents.compactMap { intent -> Int? in
                        if case .tableCell(let column) = intent.kind { return column }
                        return nil
                    }.first ?? 0
                let cell = NSTextTableBlock(
                    table: table, startingRow: row, rowSpan: 1, startingColumn: column, columnSpan: 1)
                cell.setWidth(7, type: .absoluteValueType, for: .padding)
                cell.setWidth(0.5, type: .absoluteValueType, for: .border)
                cell.setBorderColor(theme.border.nsColor)
                if row == 0 { cell.backgroundColor = theme.background.raised(1).nsColor }
                paragraph.textBlocks = [cell]
                paragraph.paragraphSpacing = 0
            }
            let font = theme.nsFont(size: size, weight: weight, monospaced: monospaced)
            let attributes: [NSAttributedString.Key: Any] = [
                .font: font, .foregroundColor: foreground, .paragraphStyle: paragraph,
            ]
            result.append(NSAttributedString(string: prefix, attributes: attributes))
            for run in content.runs {
                var attrs = attributes
                let inline = run.inlinePresentationIntent ?? []
                var runFont = font
                if inline.contains(.code) {
                    runFont = theme.nsFont(size: size - 1, monospaced: true)
                    attrs[.backgroundColor] = theme.background.raised(1).nsColor
                }
                if inline.contains(.stronglyEmphasized) {
                    runFont = NSFontManager.shared.convert(runFont, toHaveTrait: .boldFontMask)
                }
                if inline.contains(.emphasized) {
                    runFont = NSFontManager.shared.convert(runFont, toHaveTrait: .italicFontMask)
                }
                if inline.contains(.strikethrough) { attrs[.strikethroughStyle] = NSUnderlineStyle.single.rawValue }
                if let link = run.link { attrs[.link] = link }
                attrs[.font] = runFont
                let text = String(content[run.range].characters)
                if let imageURL = run.imageURL {
                    let attachment = ZZMarkdownImageAttachment(image: images[imageURL], label: text)
                    attrs[.attachment] = attachment
                    attrs[.link] = imageURL
                    attrs[.toolTip] = text.isEmpty ? imageURL.lastPathComponent : text
                    result.append(NSAttributedString(string: "\u{fffc}", attributes: attrs))
                } else {
                    result.append(NSAttributedString(string: text, attributes: attrs))
                }
            }
            if !result.string.hasSuffix("\n") {
                result.append(NSAttributedString(string: "\n", attributes: attributes))
            }
        }
        return result
    }
}

public struct ZZMarkdown: NSViewRepresentable {
    @Environment(\.zzTheme) private var theme
    private let source: String
    private let fontSize: CGFloat
    private let suppliedImages: [URL: NSImage]
    private let attributedText: NSAttributedString?

    public init(_ source: String, fontSize: CGFloat = 13, images: [URL: NSImage] = [:]) {
        self.source = source
        self.fontSize = fontSize
        suppliedImages = images
        attributedText = nil
    }

    public init(attributedText: NSAttributedString) {
        source = ""
        fontSize = 13
        suppliedImages = [:]
        self.attributedText = attributedText
    }

    public func makeNSView(context: Context) -> NSTextView {
        let view = NSTextView()
        view.isEditable = false
        view.isSelectable = true
        view.drawsBackground = false
        view.isHorizontallyResizable = false
        view.isVerticallyResizable = false
        view.textContainerInset = NSSize(width: 0, height: 4)
        view.textContainer?.lineFragmentPadding = 0
        view.textContainer?.containerSize = NSSize(width: 0, height: CGFloat.greatestFiniteMagnitude)
        view.textContainer?.widthTracksTextView = true
        view.setContentHuggingPriority(.defaultLow, for: .horizontal)
        view.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        view.setAccessibilityLabel("Markdown content")
        return view
    }

    public func updateNSView(_ view: NSTextView, context: Context) {
        let coordinator = context.coordinator
        guard
            coordinator.source != source || coordinator.theme != theme || coordinator.fontSize != fontSize
                || coordinator.suppliedImages != suppliedImages || coordinator.attributedText != attributedText
        else { return }
        if coordinator.source != source {
            coordinator.cancelLoads()
            coordinator.document = ZZMarkdownDocument(source)
            coordinator.images = coordinator.images.filter { coordinator.document?.imageURLs.contains($0.key) == true }
        }
        coordinator.source = source
        coordinator.theme = theme
        coordinator.fontSize = fontSize
        coordinator.suppliedImages = suppliedImages
        coordinator.attributedText = attributedText
        coordinator.render(in: view)
        view.linkTextAttributes = [
            .foregroundColor: theme.foreground.nsColor, .underlineStyle: NSUnderlineStyle.single.rawValue,
        ]
        if attributedText == nil { coordinator.loadImages(in: view) }
    }

    public func sizeThatFits(_ proposal: ProposedViewSize, nsView: NSTextView, context: Context) -> CGSize? {
        let fallback = nsView.bounds.width > 0 ? nsView.bounds.width : 680
        let proposedWidth = proposal.width ?? fallback
        return context.coordinator.fittingSize(width: proposedWidth.isFinite ? max(0, proposedWidth) : fallback)
    }

    public func makeCoordinator() -> Coordinator { Coordinator() }

    public static func dismantleNSView(_ view: NSTextView, coordinator: Coordinator) {
        coordinator.cancelLoads()
    }

    @MainActor public final class Coordinator {
        var source: String?
        var theme: ZZTheme?
        var fontSize: CGFloat?
        var document: ZZMarkdownDocument?
        var attributedText: NSAttributedString?
        var suppliedImages: [URL: NSImage] = [:]
        var images: [URL: NSImage] = [:]
        private var loads: [URL: Task<Void, Never>] = [:]
        private let measuringStorage = NSTextStorage()
        private let measuringLayout = NSLayoutManager()
        private let measuringContainer = NSTextContainer(
            containerSize: NSSize(width: 680, height: CGFloat.greatestFiniteMagnitude))

        init() {
            measuringStorage.addLayoutManager(measuringLayout)
            measuringLayout.addTextContainer(measuringContainer)
            measuringContainer.lineFragmentPadding = 0
        }

        func fittingSize(width: CGFloat) -> CGSize {
            guard width > 0 else { return CGSize(width: 0, height: 8) }
            measuringContainer.containerSize.width = width
            measuringLayout.ensureLayout(for: measuringContainer)
            return CGSize(width: width, height: ceil(measuringLayout.usedRect(for: measuringContainer).height + 8))
        }

        func cancelLoads() {
            for task in loads.values { task.cancel() }
            loads.removeAll()
        }

        func render(in view: NSTextView) {
            guard let theme else { return }
            let selected = view.selectedRanges
            let value =
                attributedText ?? document?.attributedString(
                    theme: theme, fontSize: fontSize ?? 13,
                    images: images.merging(suppliedImages) { _, supplied in supplied }
                ) ?? NSAttributedString(string: "")
            view.textStorage?.setAttributedString(value)
            measuringStorage.setAttributedString(value)
            view.selectedRanges = selected.map {
                NSValue(range: ZZTextSelection.clamped($0.rangeValue, toUTF16Length: value.length))
            }
            view.invalidateIntrinsicContentSize()
        }

        func loadImages(in view: NSTextView) {
            for url in document?.imageURLs ?? []
            where suppliedImages[url] == nil && images[url] == nil && loads[url] == nil {
                guard url.scheme == "https" || url.scheme == "http" else { continue }
                loads[url] = Task { [weak self, weak view] in
                    do {
                        var request = URLRequest(url: url)
                        request.timeoutInterval = 20
                        let (data, response) = try await URLSession.shared.data(for: request)
                        try Task.checkCancellation()
                        guard let response = response as? HTTPURLResponse, (200..<300).contains(response.statusCode),
                            let image = NSImage(data: data), let self, let view
                        else { return }
                        self.images[url] = image
                        self.render(in: view)
                    } catch {}
                }
            }
        }
    }
}

private final class ZZMarkdownImageAttachment: NSTextAttachment {
    private let sourceImage: NSImage?

    @MainActor init(image: NSImage?, label: String) {
        sourceImage = image
        super.init(data: nil, ofType: nil)
        if let image = image?.copy() as? NSImage {
            image.accessibilityDescription = label
            self.image = image
            attachmentCell = NSTextAttachmentCell(imageCell: image)
        } else {
            attachmentCell = NSTextAttachmentCell(textCell: label.isEmpty ? "Image" : "Image: \(label)")
        }
    }

    required init?(coder: NSCoder) { nil }

    override func attachmentBounds(
        for textContainer: NSTextContainer?, proposedLineFragment lineFrag: NSRect,
        glyphPosition position: NSPoint, characterIndex charIndex: Int
    ) -> NSRect {
        guard let sourceImage, sourceImage.size.width > 0, sourceImage.size.height > 0 else {
            var size = attachmentCell?.cellSize() ?? NSSize(width: 80, height: 20)
            size.width = min(size.width, max(1, lineFrag.width - position.x))
            return NSRect(origin: .zero, size: size)
        }
        let width = min(600, max(1, lineFrag.width - position.x))
        let scale = min(1, min(width / sourceImage.size.width, 400 / sourceImage.size.height))
        let size = NSSize(width: sourceImage.size.width * scale, height: sourceImage.size.height * scale)
        image?.size = size
        return NSRect(origin: .zero, size: size)
    }
}

public struct ZZCodeBlock: View {
    @Environment(\.zzTheme) private var theme
    @State private var copied = false
    private let source: String
    private let language: String
    private let spans: [ZZSyntaxSpan]

    public init(_ source: String, language: String = "text", spans: [ZZSyntaxSpan] = []) {
        self.source = source
        self.language = language
        self.spans = spans
    }

    public var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text(language).font(theme.font(size: 11, weight: .medium))
                    .foregroundStyle(theme.foreground.muted().color)
                Spacer()
                ZZIconButton(copied ? "Copied" : "Copy code", systemName: copied ? "checkmark" : "doc.on.doc") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(source, forType: .string)
                    copied = true
                }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 4)
            Divider().overlay(theme.border.color)
            ScrollView(.horizontal) {
                Text(ZZSyntaxHighlighter.attributedString(source, spans: spans, dark: theme.background.lightness < 0.5))
                    .font(theme.font(size: 12, monospaced: true))
                    .foregroundStyle(theme.foreground.color)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(10)
            }
        }
        .zzSurface(elevation: 1)
        .task(id: copied) {
            guard copied else { return }
            do { try await Task.sleep(for: .seconds(2)) } catch { return }
            copied = false
        }
    }
}
