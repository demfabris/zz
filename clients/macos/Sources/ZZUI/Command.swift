import SwiftUI

public struct ZZShortcutHint: Identifiable, Hashable, Sendable {
    public var id: String { key + label }
    public let key: String
    public let label: String
    public init(_ key: String, _ label: String) {
        self.key = key
        self.label = label
    }
}

public struct ZZShortcutHints: View {
    @Environment(\.zzTheme) private var theme
    private let hints: [ZZShortcutHint]
    private let spacing: CGFloat
    private let keySpacing: CGFloat

    public init(_ hints: [ZZShortcutHint], spacing: CGFloat = 12, keySpacing: CGFloat = 6) {
        self.hints = hints
        self.spacing = spacing
        self.keySpacing = keySpacing
    }
    public var body: some View {
        HStack(spacing: spacing) {
            ForEach(hints) { hint in
                HStack(spacing: keySpacing) {
                    ZZKbd(hint.key)
                    Text(hint.label).font(theme.font(size: 9))
                }
            }
        }.foregroundStyle(theme.foreground.muted().color)
    }
}

public struct ZZCommandPaletteRow: View {
    @Environment(\.zzTheme) private var theme
    @State private var hovered = false
    private let label: String
    private let detail: String
    private let kind: String
    private let selected: Bool
    private let action: () -> Void

    public init(
        _ label: String, detail: String = "", kind: String = "", selected: Bool = false,
        action: @escaping () -> Void
    ) {
        self.label = label
        self.detail = detail
        self.kind = kind
        self.selected = selected
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                Text(label).font(theme.font(size: 12, monospaced: true))
                    .frame(maxWidth: .infinity, alignment: .leading).lineLimit(1)
                Text(detail).font(theme.font(size: 10)).foregroundStyle(theme.foreground.muted().color).lineLimit(1)
                HStack(spacing: 0) {
                    if !kind.isEmpty { ZZTag(kind, fontSize: 9) }
                }.frame(width: 64, alignment: .trailing)
            }
            .padding(.horizontal, 12).frame(height: 40)
            .background(
                selected || hovered ? theme.background.hover().color : .clear,
                in: ZZRoundedRectangle(radius: theme.radius))
        }.buttonStyle(.plain).onHover { hovered = $0 }.accessibilityAddTraits(selected ? [.isSelected] : [])
    }
}

public struct ZZCommandPalette<Rows: View>: View {
    @Environment(\.zzTheme) private var theme
    @Binding private var query: String
    @FocusState private var focused: Bool
    private let prompt: String
    private let hints: [ZZShortcutHint]
    private let rowCount: Int?
    private let submit: () -> Void
    private let cancel: () -> Void
    private let move: (Int) -> Void
    private let rows: Rows

    public init(
        query: Binding<String>, prompt: String = ":",
        hints: [ZZShortcutHint] = [.init("↑ ↓", "navigate"), .init("↵", "run"), .init("esc", "close")],
        rowCount: Int? = nil,
        submit: @escaping () -> Void, cancel: @escaping () -> Void, move: @escaping (Int) -> Void = { _ in },
        @ViewBuilder rows: () -> Rows
    ) {
        _query = query
        self.prompt = prompt
        self.hints = hints
        self.rowCount = rowCount.map { max(0, $0) }
        self.submit = submit
        self.cancel = cancel
        self.move = move
        self.rows = rows()
    }

    public var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 6) {
                Text(prompt).font(theme.font(size: 11, monospaced: true))
                TextField("Type a command…", text: $query).textFieldStyle(.plain)
                    .font(theme.font(size: 12, monospaced: true)).focused($focused).onSubmit(submit)
                    .onKeyPress(.upArrow) {
                        move(-1)
                        return .handled
                    }
                    .onKeyPress(.downArrow) {
                        move(1)
                        return .handled
                    }
            }.padding(.horizontal, 13).frame(height: 36).padding(4)
            if rowCount != 0 {
                theme.border.color.frame(height: 1)
                ScrollView { LazyVStack(spacing: 0) { rows } }
                    .frame(height: rowCount.map { CGFloat(min($0, 8)) * 40 })
                    .frame(maxHeight: 320).padding(.horizontal, 5).padding(.vertical, 4)
            }
            ZZShortcutHints(hints).frame(maxWidth: .infinity, alignment: .trailing)
                .padding(.horizontal, 10).frame(height: 34)
                .overlay(alignment: .top) { theme.border.color.frame(height: 1) }
        }
        .frame(maxWidth: 560).background(theme.background.raised(1).opaque().color)
        .clipShape(ZZRoundedRectangle(radius: theme.radius + 4))
        .overlay {
            ZZRoundedRectangle(radius: theme.radius + 4).stroke(theme.border.subtle().color, lineWidth: 1)
        }
        .shadow(
            color: theme.scrim.color.opacity(theme.shadows ? theme.shadowStrength : 0), radius: 16, y: 12
        )
        .onExitCommand(perform: cancel).onAppear { focused = true }
    }
}
