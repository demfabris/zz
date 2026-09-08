import AppKit
import Foundation
import SwiftUI
import ZZUI

struct TextGallery: View {
    @Environment(\.zzTheme) private var theme
    @State private var source = Self.sampleCode
    @State private var selection = NSRange(location: 0, length: 0)
    @State private var lineNumbers = true
    @State private var softWrap = true
    @State private var editable = true
    @State private var diagramState = "Ready"
    @State private var relativeLineNumbers = false
    @State private var tabWidth = 2
    @State private var hardTabs = false

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            GallerySection(
                "Markdown",
                detail: "Select text across paragraphs, lists and tables. Links open with the Mac’s default browser."
            ) {
                ZZMarkdown(Self.sampleMarkdown)
            }
            GallerySection("Code blocks", detail: "Monospaced text, supplied syntax colors and a copy action.") {
                ZZCodeBlock(Self.sampleCode, language: "rust", spans: PreviewSyntax.spans(Self.sampleCode))
            }
            GallerySection(
                "Images", detail: "Native attachments scale to the available width and keep text selection intact."
            ) {
                ZZMarkdown(
                    "![Workspace](zz-gallery://workspace)\n\nImage attachments keep their alternative text for accessibility.",
                    images: [URL(string: "zz-gallery://workspace")!: Self.previewImage]
                )
            }
            GallerySection(
                "Mermaid diagrams",
                detail:
                    "Rendered diagrams include source copy and a full preview. Loading and errors keep their place in the conversation."
            ) {
                Picker("Diagram state", selection: $diagramState) {
                    Text("Ready").tag("Ready")
                    Text("Rendering").tag("Rendering")
                    Text("Error").tag("Error")
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(width: 270)
                ZZDiagram(
                    Self.sampleDiagram,
                    image: diagramState == "Ready" ? Image(nsImage: DiagramFixture.image(theme: theme)) : nil,
                    error: diagramState == "Error" ? "Expected a closing bracket on line 3." : nil
                )
            }
            GallerySection("Code editor", detail: "Native selection, input methods, undo, indentation and ⌘F search.") {
                HStack(spacing: 16) {
                    Toggle("Line numbers", isOn: $lineNumbers)
                    Toggle("Wrap lines", isOn: $softWrap)
                    Toggle("Editable", isOn: $editable)
                    Menu("Options") {
                        Toggle("Relative line numbers", isOn: $relativeLineNumbers)
                        Toggle("Hard tabs", isOn: $hardTabs)
                        Picker("Indentation width", selection: $tabWidth) {
                            Text("2 spaces").tag(2)
                            Text("4 spaces").tag(4)
                            Text("8 spaces").tag(8)
                        }
                    }.fixedSize()
                    Spacer()
                    ZZButton("Clear", variant: .ghost) { source = "" }
                    ZZButton("Reset", variant: .ghost) { source = Self.sampleCode }
                }
                .font(.system(size: 12))
                ZZCodeEditor(
                    text: $source, selection: $selection, spans: PreviewSyntax.spans(source),
                    showsLineNumbers: lineNumbers, softWrap: softWrap, isEditable: editable,
                    tabWidth: tabWidth, hardTabs: hardTabs, placeholder: "Write some code…",
                    relativeLineNumbers: relativeLineNumbers
                )
                .frame(height: 280)
                .zzControlSurface()
                HStack {
                    Text("\(source.utf8.count) bytes")
                    Spacer()
                    Text("Selection \(selection.location):\(selection.length)")
                }
                .font(.system(size: 11, design: .monospaced)).foregroundStyle(.secondary)
            }
        }
    }

    private static let sampleCode = """
        use zz_client::ClientCore;

        fn main() {
            let workspace = "Native on the outside 🦀";
            let panes = 8;
            println!("{workspace}: {panes} panes");
        }
        """

    private static let sampleDiagram = """
        flowchart LR
            A[Swift client] --> B[ClientCore]
            B --> C[zz daemon]
        """

    private static var previewImage: NSImage {
        let image = NSImage(systemSymbolName: "rectangle.split.2x2.fill", accessibilityDescription: "Workspace")!
        return image.withSymbolConfiguration(.init(pointSize: 96, weight: .regular)) ?? image
    }

    private static let sampleMarkdown = """
        ## A place for your work

        Keep **terminals**, *browsers* and `agents` together. Select this paragraph and keep dragging through the list below.

        - Attach to an existing session
        - Keep your panes where you left them
            - A terminal for the server
            - A browser for the app

        1. Open your workspace
        2. Pick up where you stopped

        - [x] Connect to the session
        - [ ] Open another pane

        > Sessions stay alive while you move between clients.

        | Surface | Use |
        | --- | --- |
        | Terminal | Run your tools |
        | Browser | Inspect your app |
        | Agent | Work through a task |

        Visit [zzmux.sh](https://zzmux.sh) for more. ~~Forgotten sessions~~, remembered workspaces.

        ---

        ```toml
        [workspace]
        name = "Focus time"
        ```
        """
}

private enum DiagramFixture {
    @MainActor static func image(theme: ZZTheme) -> NSImage {
        let content = HStack(spacing: 0) {
            node("Swift client", theme: theme)
            arrow(theme: theme)
            node("ClientCore", theme: theme)
            arrow(theme: theme)
            node("zz daemon", theme: theme)
        }
        .frame(width: 720, height: 180)
        .background(theme.background.color)
        let renderer = ImageRenderer(content: content)
        renderer.scale = 2
        let image = renderer.nsImage ?? NSImage(size: NSSize(width: 720, height: 180))
        image.accessibilityDescription = "Swift client connects to ClientCore, which connects to the zz daemon."
        return image
    }

    @MainActor private static func node(_ title: String, theme: ZZTheme) -> some View {
        Text(title)
            .font(theme.font(size: 14, weight: .medium))
            .foregroundStyle(theme.foreground.color)
            .frame(width: 180, height: 64)
            .background(theme.background.raised(1).color, in: ZZRoundedRectangle(radius: theme.radius))
            .overlay { ZZRoundedRectangle(radius: theme.radius).strokeBorder(theme.border.color, lineWidth: 1) }
    }

    @MainActor private static func arrow(theme: ZZTheme) -> some View {
        Image(systemName: "arrow.right")
            .font(.system(size: 16))
            .foregroundStyle(theme.foreground.muted().color)
            .frame(width: 60)
    }
}

private enum PreviewSyntax {
    static func spans(_ source: String) -> [ZZSyntaxSpan] {
        let rules: [(String, ZZSyntaxKind)] = [
            (#"\b(use|fn|let|mut|pub|struct|impl|self|return|if|else)\b"#, .keyword),
            (#"\b[0-9]+\b"#, .number),
            (#"\b[A-Z][A-Za-z0-9_]*\b"#, .type),
            (#"\b[a-z_][A-Za-z0-9_]*(?=!?\()"#, .function),
            (#""(?:\\.|[^"\\])*""#, .string),
        ]
        let range = NSRange(source.startIndex..<source.endIndex, in: source)
        return rules.flatMap { pattern, kind in
            guard let regex = try? NSRegularExpression(pattern: pattern) else { return [ZZSyntaxSpan]() }
            return regex.matches(in: source, range: range).map { ZZSyntaxSpan(range: $0.range, kind: kind) }
        }
    }
}
