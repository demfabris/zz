import SwiftUI

public struct ZZPickerOverlay<Content: View>: View {
    @Environment(\.zzTheme) private var theme
    private let dismiss: () -> Void
    private let content: Content

    public init(dismiss: @escaping () -> Void, @ViewBuilder content: () -> Content) {
        self.dismiss = dismiss
        self.content = content()
    }

    public var body: some View {
        ZStack {
            theme.scrim.color.contentShape(.rect).onTapGesture(perform: dismiss)
            content.padding(16)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .onExitCommand(perform: dismiss)
    }
}

public struct ZZPickerModal<Header: View, Rows: View, Footer: View>: View {
    @Environment(\.zzTheme) private var theme
    private let emptyMessage: String?
    private let header: Header
    private let rows: Rows
    private let footer: Footer

    public init(
        emptyMessage: String? = nil, @ViewBuilder header: () -> Header,
        @ViewBuilder rows: () -> Rows, @ViewBuilder footer: () -> Footer
    ) {
        self.emptyMessage = emptyMessage
        self.header = header()
        self.rows = rows()
        self.footer = footer()
    }

    public var body: some View {
        GeometryReader { geometry in
            VStack(spacing: 0) {
                VStack(alignment: .leading, spacing: 6) { header }
                    .padding(6).padding(.bottom, 1)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .overlay(alignment: .bottom) { ZZSeparator() }
                ZStack {
                    if let emptyMessage {
                        ZZPickerEmpty(emptyMessage)
                    } else {
                        ScrollView { LazyVStack(spacing: 0) { rows } }
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .clipped().padding(6)
                HStack(spacing: 6) { footer }
                    .font(theme.font(size: 10)).foregroundStyle(theme.foreground.muted().color)
                    .padding(.vertical, 6).padding(.leading, 16).padding(.trailing, 6).padding(.top, 1)
                    .frame(maxWidth: .infinity, minHeight: 40, alignment: .leading)
                    .overlay(alignment: .top) { ZZSeparator() }
            }
            .padding(1)
            .frame(
                width: min(660, geometry.size.width * 0.92),
                height: min(720, max(240, geometry.size.height * 0.82))
            )
            .foregroundStyle(theme.foreground.color)
            .background(theme.background.raised(1).color)
            .clipShape(ZZRoundedRectangle(radius: theme.radius))
            .overlay {
                ZZRoundedRectangle(radius: theme.radius).strokeBorder(theme.border.color, lineWidth: 1)
                    .allowsHitTesting(false)
            }
            .shadow(
                color: theme.scrim.color.opacity(theme.shadows ? theme.shadowStrength : 0), radius: 16, y: 12
            )
            .contentShape(.rect).onTapGesture {}
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}

public struct ZZPickerSearch: View {
    @Environment(\.zzTheme) private var theme
    @Binding private var query: String
    @FocusState private var focused: Bool
    private let placeholder: String
    private let submit: () -> Void
    private let move: (Int) -> Void
    private let cancel: () -> Void

    public init(
        _ placeholder: String, query: Binding<String>, submit: @escaping () -> Void = {},
        move: @escaping (Int) -> Void = { _ in }, cancel: @escaping () -> Void = {}
    ) {
        self.placeholder = placeholder
        self._query = query
        self.submit = submit
        self.move = move
        self.cancel = cancel
    }

    public var body: some View {
        HStack(spacing: 0) {
            Image(systemName: "magnifyingglass").font(.system(size: 12))
                .frame(width: 12, height: 12).foregroundStyle(theme.foreground.muted().color)
                .accessibilityHidden(true)
            TextField(placeholder, text: $query).textFieldStyle(.plain)
                .font(theme.font(size: 12)).foregroundStyle(theme.foreground.color)
                .padding(.horizontal, 8).frame(height: 28)
                .focused($focused).focusEffectDisabled().onSubmit(submit)
                .onKeyPress(.upArrow) {
                    move(-1); return .handled
                }
                .onKeyPress(.downArrow) {
                    move(1); return .handled
                }
                .onExitCommand(perform: cancel)
        }
        .padding(.horizontal, 10).frame(height: 32)
        .background(theme.background.raised(1).color, in: ZZRoundedRectangle(radius: theme.radius))
        .overlay {
            ZZRoundedRectangle(radius: theme.radius).strokeBorder(theme.border.color, lineWidth: 1)
                .allowsHitTesting(false)
        }
        .onAppear { focused = true }
    }
}

public struct ZZPickerEmpty: View {
    @Environment(\.zzTheme) private var theme
    private let label: String

    public init(_ label: String) { self.label = label }

    public var body: some View {
        Text(label).font(theme.font(size: 11)).foregroundStyle(theme.foreground.muted().color)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

public struct ZZPathRow: View {
    @Environment(\.zzTheme) private var theme
    private let label: String
    private let icon: String
    private let selected: Bool
    private let action: () -> Void
    private let hover: () -> Void

    public init(
        _ label: String, icon: String, selected: Bool = false,
        action: @escaping () -> Void, hover: @escaping () -> Void = {}
    ) {
        self.label = label
        self.icon = icon
        self.selected = selected
        self.action = action
        self.hover = hover
    }

    public var body: some View {
        ZZPickerRow(selected: selected, selectedFill: theme.background.raised(2).wash(), action: action, hover: hover) {
            Image(systemName: icon).font(.system(size: 12)).frame(width: 12, height: 12)
                .foregroundStyle(theme.foreground.muted().color).accessibilityHidden(true)
            Text(label).font(theme.font(size: 12)).lineLimit(1)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .frame(height: 26)
    }
}

public struct ZZDirectoryRow: View {
    private let label: String
    private let selected: Bool
    private let action: () -> Void
    private let hover: () -> Void

    public init(
        _ label: String, selected: Bool = false, action: @escaping () -> Void,
        hover: @escaping () -> Void = {}
    ) {
        self.label = label
        self.selected = selected
        self.action = action
        self.hover = hover
    }

    public var body: some View {
        ZZPathRow(label, icon: "folder", selected: selected, action: action, hover: hover)
    }
}

public struct ZZHistoryRow: View {
    @Environment(\.zzTheme) private var theme
    private let title: String
    private let directory: String
    private let updatedAt: String?
    private let selected: Bool
    private let current: Bool
    private let action: () -> Void
    private let hover: () -> Void

    public init(
        _ title: String, directory: String, updatedAt: String? = nil, selected: Bool = false,
        current: Bool = false, action: @escaping () -> Void, hover: @escaping () -> Void = {}
    ) {
        self.title = title
        self.directory = directory
        self.updatedAt = updatedAt
        self.selected = selected
        self.current = current
        self.action = action
        self.hover = hover
    }

    public var body: some View {
        ZZPickerRow(selected: selected, action: action, hover: hover) {
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(theme.font(size: 12, weight: .medium)).lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
                HStack(spacing: 4) {
                    Text(directory).font(theme.font(size: 9)).lineLimit(1)
                        .padding(.horizontal, 6).padding(.vertical, 2)
                        .background(theme.background.raised(2).color, in: ZZRoundedRectangle(radius: theme.radius))
                        .overlay {
                            ZZRoundedRectangle(radius: theme.radius).strokeBorder(theme.border.color, lineWidth: 1)
                                .allowsHitTesting(false)
                        }
                    if current {
                        Text("CURRENT").font(theme.font(size: 8)).foregroundStyle(theme.success.color)
                            .padding(.horizontal, 4).background(theme.success.fill().color, in: Capsule())
                            .fixedSize()
                    }
                    Spacer(minLength: 0)
                    if let updatedAt {
                        Text(updatedAt).font(theme.font(size: 9)).lineLimit(1)
                            .frame(maxWidth: 112, alignment: .trailing)
                    }
                }
                .foregroundStyle(theme.foreground.muted().color)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .frame(height: 52)
    }
}

private struct ZZPickerRow<Content: View>: View {
    @Environment(\.zzTheme) private var theme
    @State private var hovered = false
    let selected: Bool
    var selectedFill: ZZColor?
    let action: () -> Void
    let hover: () -> Void
    @ViewBuilder let content: Content

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) { content }
                .padding(.horizontal, 10).frame(maxWidth: .infinity, maxHeight: .infinity)
                .foregroundStyle(theme.foreground.color)
                .background(
                    selected
                        ? (selectedFill ?? theme.background.hover()).color
                        : hovered ? theme.background.hover().color : .clear,
                    in: ZZRoundedRectangle(radius: theme.radius)
                )
                .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .onHover {
            hovered = $0
            if $0 { hover() }
        }
        .accessibilityAddTraits(selected ? [.isSelected] : [])
    }
}
