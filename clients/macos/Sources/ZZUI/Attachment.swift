import SwiftUI

public struct ZZAttachmentThumbnail: View {
    @Environment(\.zzTheme) private var theme
    private let image: Image
    private let title: String
    private let side: CGFloat
    private let open: () -> Void
    private let remove: (() -> Void)?

    public init(
        _ image: Image, title: String, side: CGFloat = ZZAgentMetrics.composerAttachment,
        open: @escaping () -> Void, remove: (() -> Void)? = nil
    ) {
        self.image = image
        self.title = title
        self.side = side
        self.open = open
        self.remove = remove
    }

    public var body: some View {
        Button(action: open) {
            image.resizable().scaledToFit().frame(width: side, height: side)
                .background(theme.background.raised(2).color)
                .clipShape(ZZRoundedRectangle(radius: theme.radius))
                .overlay { ZZRoundedRectangle(radius: theme.radius).stroke(theme.border.color, lineWidth: 1) }
        }
        .buttonStyle(.plain).accessibilityLabel("Preview \(title)").help(title)
        .overlay(alignment: .topTrailing) {
            if let remove {
                ZZButton(
                    "Remove \(title)", icon: "xmark", variant: .secondary, size: .xSmall,
                    iconOnly: true, action: remove
                ).offset(x: 5, y: -5)
            }
        }
    }
}

public struct ZZAttachmentStrip<Content: View>: View {
    private let content: Content
    public init(@ViewBuilder content: () -> Content) { self.content = content() }
    public var body: some View {
        ScrollView(.horizontal) { HStack(spacing: 8) { content }.padding(12) }.scrollIndicators(.hidden)
    }
}

public struct ZZAttachmentPreview: View {
    @Environment(\.zzTheme) private var theme
    @State private var zoom: CGFloat = 1
    @GestureState private var magnification: CGFloat = 1
    private let image: Image
    private let title: String
    private let close: () -> Void

    public init(_ image: Image, title: String, close: @escaping () -> Void) {
        self.image = image
        self.title = title
        self.close = close
    }

    public var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Text(title).font(theme.font(size: 13, weight: .medium)).lineLimit(1)
                Spacer()
                ZZIconButton("Zoom out", systemName: "minus.magnifyingglass") { zoom = max(0.25, zoom / 1.25) }
                ZZButton("\(Int(zoom * 100))%", variant: .ghost, size: .small) { zoom = 1 }
                ZZIconButton("Zoom in", systemName: "plus.magnifyingglass") { zoom = min(4, zoom * 1.25) }
                ZZIconButton("Close preview", systemName: "xmark", action: close)
            }.padding(12)
            ZZSeparator()
            GeometryReader { geometry in
                ScrollView([.horizontal, .vertical]) {
                    image.resizable().scaledToFit()
                        .frame(
                            width: geometry.size.width * zoom * magnification,
                            height: geometry.size.height * zoom * magnification
                        )
                        .accessibilityLabel(title)
                }
                .gesture(
                    MagnifyGesture()
                        .updating($magnification) { value, state, _ in state = value.magnification }
                        .onEnded { zoom = min(4, max(0.25, zoom * $0.magnification)) })
            }
        }
        .frame(minWidth: 440, idealWidth: 880, maxWidth: 880, minHeight: 320, idealHeight: 560, maxHeight: 560)
        .onExitCommand(perform: close)
    }
}
