import AppKit
import SwiftUI

public enum ZZDiagramPhase: Equatable, Sendable {
    case pending
    case ready
    case failed(String)
}

public struct ZZDiagram<Content: View>: View {
    @Environment(\.zzTheme) private var theme
    @State private var copied = false
    @State private var showingPreview = false
    private let source: String
    private let title: String
    private let phase: ZZDiagramPhase
    private let content: Content
    private let previewImage: Image?
    private let preview: (() -> Void)?

    public init(
        _ source: String, title: String = "Mermaid", phase: ZZDiagramPhase,
        preview: (() -> Void)? = nil, @ViewBuilder content: () -> Content
    ) {
        self.source = source
        self.title = title
        self.phase = phase
        self.preview = preview
        self.content = content()
        previewImage = nil
    }

    private init(_ source: String, title: String, phase: ZZDiagramPhase, content: Content, previewImage: Image?) {
        self.source = source
        self.title = title
        self.phase = phase
        self.content = content
        self.previewImage = previewImage
        preview = nil
    }

    public var body: some View {
        Group {
            switch phase {
            case .pending:
                HStack(spacing: 8) {
                    ZZSpinner(size: 14)
                    Text("Rendering \(title)…")
                }
                .foregroundStyle(theme.foreground.muted().color)
                .frame(maxWidth: .infinity, minHeight: 120)
            case .ready:
                content.scaledToFit()
                    .frame(maxWidth: .infinity, maxHeight: 560)
                    .clipped()
                    .accessibilityLabel("\(title) diagram")
            case .failed(let error):
                VStack(alignment: .leading, spacing: 8) {
                    Label("\(title) could not be rendered", systemImage: "exclamationmark.triangle")
                        .foregroundStyle(theme.danger.color)
                    Text(error).foregroundStyle(theme.foreground.muted().color)
                        .textSelection(.enabled)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.trailing, 28)
            }
        }
        .font(theme.font(size: 11))
        .padding(12)
        .background(theme.background.color)
        .clipShape(ZZRoundedRectangle(radius: theme.radius))
        .overlay {
            ZZRoundedRectangle(radius: theme.radius).strokeBorder(theme.border.color, lineWidth: 1)
        }
        .overlay(alignment: .topTrailing) {
            HStack(spacing: 4) {
                ZZIconButton(copied ? "Copied source" : "Copy source", systemName: copied ? "checkmark" : "doc.on.doc")
                {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(source, forType: .string)
                    copied = true
                }
                if phase == .ready, previewImage != nil || preview != nil {
                    ZZIconButton("Open full diagram", systemName: "arrow.up.left.and.arrow.down.right") {
                        if let preview { preview() } else { showingPreview = true }
                    }
                }
            }
            .padding(4)
        }
        .sheet(isPresented: $showingPreview) {
            if let previewImage {
                ZZAttachmentPreview(previewImage, title: "\(title) diagram") { showingPreview = false }
            }
        }
        .task(id: copied) {
            guard copied else { return }
            do { try await Task.sleep(for: .seconds(2)) } catch { return }
            copied = false
        }
    }
}

extension ZZDiagram where Content == Image {
    public init(_ source: String, title: String = "Mermaid", image: Image?, error: String? = nil) {
        let phase: ZZDiagramPhase = error.map(ZZDiagramPhase.failed) ?? (image == nil ? .pending : .ready)
        self.init(
            source, title: title, phase: phase,
            content: (image ?? Image(systemName: "photo")).resizable(), previewImage: image
        )
    }
}
