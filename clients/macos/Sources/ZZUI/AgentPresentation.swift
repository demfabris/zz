import SwiftUI

public struct ZZAgentError: View {
    @Environment(\.zzTheme) private var theme
    private let message: String

    public init(_ message: String) { self.message = message }

    public var body: some View {
        Text(message).font(theme.font(size: 11)).textSelection(.enabled)
            .frame(maxWidth: .infinity, alignment: .leading).padding(12)
            .background(theme.danger.fill().color, in: ZZRoundedRectangle(radius: theme.radius))
            .overlay {
                ZZRoundedRectangle(radius: theme.radius).strokeBorder(theme.danger.outline().color, lineWidth: 1)
            }
    }
}

public struct ZZAgentEmptyState: View {
    @Environment(\.zzTheme) private var theme
    private let message: String
    private let busy: Bool

    public init(_ message: String, busy: Bool = false) {
        self.message = message
        self.busy = busy
    }

    public var body: some View {
        VStack(spacing: 8) {
            if busy { ZZSpinner(size: 14) }
            Text(message).font(theme.font(size: 12)).multilineTextAlignment(.center)
        }.foregroundStyle(theme.foreground.muted().color)
            .frame(maxWidth: .infinity).padding(.vertical, 48)
    }
}

public struct ZZAgentPermissionOption<Content: View>: View {
    @Environment(\.zzTheme) private var theme
    private let index: Int
    private let highlighted: Bool
    private let content: Content

    public init(index: Int, highlighted: Bool = false, @ViewBuilder content: () -> Content) {
        self.index = index
        self.highlighted = highlighted
        self.content = content()
    }

    public var body: some View {
        HStack(spacing: 8) {
            if (0..<9).contains(index) {
                Text("\(index + 1)").font(theme.font(size: 9))
                    .foregroundStyle(theme.foreground.muted().color)
                    .padding(.horizontal, 8).padding(.vertical, 2)
                    .background(theme.background.raised(2).color, in: ZZRoundedRectangle(radius: theme.radius))
            }
            content
        }.frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 4).padding(.vertical, 2)
            .background(
                highlighted ? theme.background.hover().color : .clear,
                in: ZZRoundedRectangle(radius: theme.radius))
    }
}

public struct ZZAgentPermissionCard<Options: View, Cancel: View>: View {
    @Environment(\.zzTheme) private var theme
    private let title: String
    private let counter: String?
    private let options: Options
    private let cancel: Cancel

    public init(
        _ title: String, counter: String? = nil,
        @ViewBuilder options: () -> Options, @ViewBuilder cancel: () -> Cancel
    ) {
        self.title = title
        self.counter = counter
        self.options = options()
        self.cancel = cancel()
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Image(systemName: "exclamationmark.triangle").font(.system(size: 14)).foregroundStyle(
                    theme.warning.color)
                Text(title).font(theme.font(size: 12)).frame(maxWidth: .infinity, alignment: .leading)
                if let counter {
                    Text(counter).font(theme.font(size: 9)).foregroundStyle(theme.foreground.muted().color)
                        .padding(.horizontal, 8).padding(.vertical, 2)
                        .background(theme.background.raised(2).color, in: ZZRoundedRectangle(radius: theme.radius))
                }
            }
            VStack(spacing: 4) { options }
            HStack(spacing: 8) {
                Text("1-9 picks · enter confirms · esc cancels").font(theme.font(size: 9))
                    .foregroundStyle(theme.foreground.muted().color)
                Spacer(minLength: 0)
                cancel
            }
        }.padding(12)
            .background(theme.warning.fill().color, in: ZZRoundedRectangle(radius: theme.radius))
            .overlay {
                ZZRoundedRectangle(radius: theme.radius).strokeBorder(theme.warning.outline().color, lineWidth: 1)
            }
    }
}

public struct ZZAgentSuggestionRow: View {
    @Environment(\.zzTheme) private var theme
    @State private var hovered = false
    private let name: String
    private let description: String?
    private let selected: Bool
    private let action: () -> Void

    public init(_ name: String, description: String? = nil, selected: Bool = false, action: @escaping () -> Void) {
        self.name = name
        self.description = description
        self.selected = selected
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            VStack(alignment: .leading, spacing: 2) {
                Text("/" + name).font(theme.font(size: 12, weight: .medium)).lineLimit(1)
                if let description {
                    Text(description).font(theme.font(size: 10)).foregroundStyle(theme.foreground.muted().color)
                        .lineLimit(1)
                }
            }.frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 12).frame(height: 52)
                .background(
                    selected || hovered ? theme.background.hover().color : .clear,
                    in: ZZRoundedRectangle(radius: theme.radius)
                )
                .contentShape(Rectangle())
        }.buttonStyle(.plain).onHover { hovered = $0 }.accessibilityAddTraits(selected ? [.isSelected] : [])
    }
}

public struct ZZAgentSuggestionList<Rows: View>: View {
    @Environment(\.zzTheme) private var theme
    private let rowCount: Int
    private let selection: String?
    private let rows: Rows

    public init(rowCount: Int, selection: String? = nil, @ViewBuilder rows: () -> Rows) {
        self.rowCount = max(0, rowCount)
        self.selection = selection
        self.rows = rows()
    }

    public var body: some View {
        ScrollViewReader { proxy in
            ScrollView { LazyVStack(spacing: 0) { rows } }
                .onChange(of: selection) { if let selection { proxy.scrollTo(selection) } }
        }.frame(height: CGFloat(min(rowCount, 6)) * 52)
            .padding(4).background(theme.background.raised(1).color)
            .clipShape(ZZRoundedRectangle(radius: theme.radius))
            .overlay { ZZRoundedRectangle(radius: theme.radius).strokeBorder(theme.border.color, lineWidth: 1) }
            .shadow(color: theme.scrim.color.opacity(theme.shadows ? theme.shadowStrength : 0), radius: 6, y: 4)
    }
}
