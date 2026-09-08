import SwiftUI

private extension EnvironmentValues {
    @Entry var zzChooserFocus: FocusState<Bool>.Binding? = nil
}

public struct ZZChooserModal<Rows: View, Footer: View>: View {
    @Environment(\.zzTheme) private var theme
    @FocusState private var focused: Bool
    private let title: String
    private let subtitle: String
    private let maxWidth: CGFloat
    private let rowCount: Int
    private let prompt: String?
    private let help: String?
    private let close: () -> Void
    private let move: ((Int) -> Void)?
    private let accept: (() -> Void)?
    private let toggleTag: (() -> Void)?
    private let rows: Rows
    private let footer: Footer

    public init(
        _ title: String, subtitle: String, maxWidth: CGFloat = 600, rowCount: Int,
        prompt: String? = nil, help: String? = nil, close: @escaping () -> Void,
        move: ((Int) -> Void)? = nil, accept: (() -> Void)? = nil, toggleTag: (() -> Void)? = nil,
        @ViewBuilder rows: () -> Rows, @ViewBuilder footer: () -> Footer
    ) {
        self.title = title
        self.subtitle = subtitle
        self.maxWidth = maxWidth
        self.rowCount = max(0, rowCount)
        self.prompt = prompt
        self.help = help
        self.close = close
        self.move = move
        self.accept = accept
        self.toggleTag = toggleTag
        self.rows = rows()
        self.footer = footer()
    }

    public var body: some View {
        VStack(spacing: 0) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(theme.font(size: 13, weight: .medium))
                    Text(subtitle).font(theme.font(size: 11)).foregroundStyle(theme.foreground.muted().color).lineLimit(
                        1)
                }
                Spacer(minLength: 8)
                ZZIconButton("Close chooser", systemName: "xmark", action: close)
            }.padding(.horizontal, 12).frame(height: 56)
            if let help { notice(help) }
            if let prompt { notice(prompt) }
            ScrollView { LazyVStack(spacing: 0) { rows }.environment(\.zzChooserFocus, $focused) }
                .frame(height: CGFloat(min(rowCount, 10)) * 40).padding(4)
            footer.frame(height: 34)
        }
        .frame(maxWidth: maxWidth).background(theme.background.raised(1).opaque().color)
        .clipShape(ZZRoundedRectangle(radius: theme.radius + 4))
        .overlay {
            ZZRoundedRectangle(radius: theme.radius + 4).strokeBorder(
                theme.foreground.opacity(0.1).color, lineWidth: 0.5)
        }
        .overlay {
            ZZRoundedRectangle(radius: theme.radius + 4).stroke(theme.border.subtle().color, lineWidth: 1).padding(-0.5)
        }
        .shadow(
            color: theme.scrim.color.opacity(theme.shadows ? theme.shadowStrength : 0), radius: 18, y: 14
        ).onExitCommand {
            focused = true
            close()
        }
        .focusable().focused($focused).focusEffectDisabled()
        .onChange(of: help) { if help != nil { focused = true } }
        .onChange(of: prompt) { if prompt != nil { focused = true } }
        .onKeyPress(.upArrow) {
            guard let move else { return .ignored }
            move(-1)
            return .handled
        }
        .onKeyPress(.downArrow) {
            guard let move else { return .ignored }
            move(1)
            return .handled
        }
        .onKeyPress(.return) {
            guard let accept else { return .ignored }
            accept()
            return .handled
        }
        .onKeyPress(.space) {
            guard let toggleTag else { return .ignored }
            toggleTag()
            return .handled
        }
    }

    private func notice(_ value: String) -> some View {
        Text(value).font(theme.font(size: 11, monospaced: true))
            .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 12).padding(.vertical, 9)
            .background(theme.background.raised(2).color)
            .overlay(alignment: .bottom) { theme.border.color.frame(height: 1) }
    }
}

public struct ZZChooserFooter: View {
    @Environment(\.zzTheme) private var theme
    @FocusState private var searchFocused: Bool
    private let search: Binding<String>?
    private let prefix: String
    private let hints: [ZZShortcutHint]
    private let submit: () -> Void

    public init(
        search: Binding<String>? = nil, prefix: String = "Search:",
        hints: [ZZShortcutHint] = [.init("↵", "accept"), .init("esc", "cancel")], submit: @escaping () -> Void = {}
    ) {
        self.search = search
        self.prefix = prefix
        self.hints = hints
        self.submit = submit
    }

    public var body: some View {
        HStack(spacing: 12) {
            if let search {
                HStack(spacing: 7) {
                    Text(prefix).font(theme.font(size: 11, monospaced: true))
                    TextField("Filter", text: search).textFieldStyle(.plain)
                        .font(theme.font(size: 11, monospaced: true)).onSubmit(submit)
                        .focused($searchFocused).onAppear { searchFocused = true }
                }
            }
            Spacer(minLength: 0)
            ZZShortcutHints(hints, spacing: 10, keySpacing: 4)
        }
        .padding(.horizontal, 12).frame(height: 34).foregroundStyle(theme.foreground.color)
        .overlay(alignment: .top) { theme.foreground.opacity(0.1).color.frame(height: 0.5) }
    }
}

public struct ZZTreeChooserRow: View {
    @Environment(\.zzTheme) private var theme
    @Environment(\.zzChooserFocus) private var chooserFocus
    @State private var hovered = false
    private let title: String
    private let detail: String
    private let target: String
    private let icon: String
    private let depth: Int
    private let expanded: Bool?
    private let key: String?
    private let active: Bool
    private let tagged: Bool
    private let selected: Bool
    private let action: () -> Void

    public init(
        _ title: String, detail: String = "", target: String, icon: String = "terminal", depth: Int = 0,
        expanded: Bool? = nil,
        key: String? = nil, active: Bool = false, tagged: Bool = false, selected: Bool = false,
        action: @escaping () -> Void
    ) {
        self.title = title
        self.detail = detail
        self.target = target
        self.icon = icon
        self.depth = max(0, depth)
        self.expanded = expanded
        self.key = key
        self.active = active
        self.tagged = tagged
        self.selected = selected
        self.action = action
    }

    public var body: some View {
        Button {
            chooserFocus?.wrappedValue = true
            action()
        } label: {
            HStack(spacing: 0) {
                if let key {
                    Text(key.isEmpty ? "" : "(\(key))").font(theme.font(size: 10, monospaced: true)).frame(
                        width: 30, alignment: .leading
                    ).foregroundStyle(theme.foreground.muted().color)
                }
                Image(systemName: expanded == true ? "chevron.down" : "chevron.right")
                    .font(.system(size: 12)).opacity(expanded == nil ? 0 : 1)
                    .frame(width: 16).padding(.leading, CGFloat(depth) * 16)
                    .foregroundStyle(theme.foreground.muted().color)
                    .accessibilityHidden(true)
                Image(systemName: icon).font(.system(size: 14)).frame(width: 14)
                    .foregroundStyle((active ? theme.foreground : theme.foreground.muted()).color)
                Text(title).font(theme.font(size: 13, weight: active ? .medium : .regular)).lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading).padding(.leading, 8)
                Text(detail).font(theme.font(size: 11)).foregroundStyle(theme.foreground.muted().color).lineLimit(1)
                    .padding(.leading, 12)
                Text(target).font(theme.font(size: 10, monospaced: true)).foregroundStyle(
                    theme.foreground.muted().color
                ).padding(.leading, 12)
                if tagged { ZZTag("Tagged", fontSize: 9).padding(.leading, 8) }
                Image(systemName: "checkmark").font(.system(size: 12)).opacity(active ? 1 : 0).frame(
                    width: 22, alignment: .trailing)
            }
            .padding(.horizontal, 8).frame(height: 40)
            .background(
                hovered ? theme.background.hover().color : selected ? theme.background.washed(2).color : .clear,
                in: ZZRoundedRectangle(radius: theme.radius))
        }.buttonStyle(.plain).onHover { hovered = $0 }.accessibilityAddTraits(selected || active ? [.isSelected] : [])
    }
}

public struct ZZBufferChooserRow: View {
    @Environment(\.zzTheme) private var theme
    @Environment(\.zzChooserFocus) private var chooserFocus
    @State private var hovered = false
    private let name: String
    private let preview: String
    private let size: String
    private let age: String
    private let key: String?
    private let selected: Bool
    private let tagged: Bool
    private let action: () -> Void

    public init(
        _ name: String, preview: String, size: String, age: String, key: String? = nil,
        selected: Bool = false, tagged: Bool = false, action: @escaping () -> Void
    ) {
        self.name = name
        self.preview = preview
        self.size = size
        self.age = age
        self.key = key
        self.selected = selected
        self.tagged = tagged
        self.action = action
    }

    public var body: some View {
        Button {
            chooserFocus?.wrappedValue = true
            action()
        } label: {
            HStack(spacing: 12) {
                if let key {
                    Text(key.isEmpty ? "" : "(\(key))").font(theme.font(size: 10, monospaced: true))
                        .frame(width: 30, alignment: .leading).foregroundStyle(theme.foreground.muted().color)
                }
                Text(name).font(theme.font(size: 11, monospaced: true)).frame(width: 142, alignment: .leading)
                    .lineLimit(1)
                Text(preview).font(theme.font(size: 11)).frame(maxWidth: .infinity, alignment: .leading).lineLimit(1)
                Text(size).font(theme.font(size: 9, monospaced: true)).frame(width: 76, alignment: .trailing)
                    .foregroundStyle(theme.foreground.muted().color)
                Text(age).font(theme.font(size: 9, monospaced: true)).frame(width: 54, alignment: .trailing)
                    .foregroundStyle(theme.foreground.muted().color)
                if tagged { ZZTag("TAGGED", variant: .primary, size: .small, fontSize: 9).padding(.leading, 8) }
            }
            .padding(.horizontal, 10).frame(height: 40)
            .background(
                hovered ? theme.background.hover().color : selected ? theme.background.washed(2).color : .clear,
                in: ZZRoundedRectangle(radius: theme.radius))
        }.buttonStyle(.plain).onHover { hovered = $0 }.accessibilityAddTraits(selected ? [.isSelected] : [])
    }
}
