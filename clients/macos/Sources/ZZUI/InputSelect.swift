import SwiftUI

public struct ZZSelectOption: Identifiable, Hashable, Sendable {
    public let id: String
    public var title: String
    public var group: String?
    public var detail: String?
    public var systemImage: String?
    public var disabled: Bool

    public init(
        _ id: String, title: String? = nil, group: String? = nil, detail: String? = nil, systemImage: String? = nil,
        disabled: Bool = false
    ) {
        self.id = id
        self.title = title ?? id
        self.group = group
        self.detail = detail
        self.systemImage = systemImage
        self.disabled = disabled
    }

    public static func matching(_ query: String, in options: [Self]) -> [Self] {
        let words = query.split(whereSeparator: \.isWhitespace)
        return options.filter { option in
            let text = [option.title, option.group ?? "", option.detail ?? ""].joined(separator: " ")
            return words.allSatisfy { text.localizedStandardContains(String($0)) }
        }
    }

    public static func next(after cursor: String?, forward: Bool, in options: [Self]) -> String? {
        let enabled = options.filter { !$0.disabled }
        guard !enabled.isEmpty else { return nil }
        guard let cursor, let index = enabled.firstIndex(where: { $0.id == cursor }) else {
            return forward ? enabled.first?.id : enabled.last?.id
        }
        let next = (index + (forward ? 1 : enabled.count - 1)) % enabled.count
        return enabled[next].id
    }
}

public struct ZZSelect: View {
    @Environment(\.zzTheme) private var theme
    @Environment(\.isEnabled) private var isEnabled
    @Binding private var selection: String?
    @State private var presented = false
    @State private var query = ""
    @State private var cursor: String?
    @State private var selectedOption: ZZSelectOption?
    @State private var triggerWidth: CGFloat = 180
    @FocusState private var triggerFocused: Bool
    @FocusState private var searchFocused: Bool
    @FocusState private var menuFocused: Bool
    private let title: String
    private let options: [ZZSelectOption]
    private let placeholder: String
    private let size: ZZControlSize
    private let searchable: Bool
    private let clearable: Bool
    private let menuMaxHeight: CGFloat

    public init(
        _ title: String, selection: Binding<String?>, options: [ZZSelectOption], placeholder: String = "Select",
        size: ZZControlSize = .small, searchable: Bool = false, clearable: Bool = false, menuMaxHeight: CGFloat = 320
    ) {
        self.title = title
        self._selection = selection
        self.options = options
        self.placeholder = placeholder
        self.size = size
        self.searchable = searchable
        self.clearable = clearable
        self.menuMaxHeight = menuMaxHeight
    }

    private var shownOption: ZZSelectOption? {
        options.first(where: { $0.id == selection }) ?? (selectedOption?.id == selection ? selectedOption : nil)
    }

    public var body: some View {
        HStack(spacing: 0) {
            Button(action: open) {
                HStack(spacing: 4) {
                    Color.clear.frame(width: 12, height: 12)
                    Text(shownOption?.title ?? placeholder)
                        .lineLimit(1)
                        .frame(maxWidth: .infinity)
                        .foregroundStyle((shownOption == nil ? theme.foreground.muted() : theme.foreground).color)
                    Image(systemName: "chevron.down")
                        .font(.system(size: 10, weight: .medium))
                        .foregroundStyle(theme.foreground.muted().color)
                        .frame(width: 12)
                        .accessibilityHidden(true)
                }
                .padding(.horizontal, size.horizontalPadding)
                .frame(height: size.height)
                .contentShape(.rect)
            }
            .buttonStyle(.plain)
            .focused($triggerFocused)
            .focusEffectDisabled()
            .onKeyPress(.downArrow) {
                open()
                return .handled
            }
            .onKeyPress(.upArrow) {
                open()
                return .handled
            }
            .accessibilityLabel(title)
            .accessibilityValue(shownOption?.title ?? placeholder)
            .accessibilityHint("Opens a list of options")
            if clearable && selection != nil {
                Button {
                    selection = nil
                    selectedOption = nil
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .font(.system(size: 12))
                        .foregroundStyle(theme.foreground.muted().color)
                }
                .buttonStyle(.plain)
                .padding(.trailing, size.horizontalPadding)
                .accessibilityLabel("Clear \(title)")
            }
        }
        .font(theme.font(size: size.fontSize))
        .zzControlSurface(focused: presented || triggerFocused)
        .opacity(isEnabled ? 1 : 0.5)
        .onGeometryChange(for: CGFloat.self) {
            $0.size.width
        } action: {
            triggerWidth = $0
        }
        .popover(isPresented: $presented, arrowEdge: .bottom) { menu }
        .onChange(of: selection, initial: true) { rememberSelection() }
        .onChange(of: isEnabled) { if !isEnabled { presented = false } }
        .onChange(of: options) {
            rememberSelection()
            if !filtered.contains(where: { $0.id == cursor && !$0.disabled }) {
                cursor = ZZSelectOption.next(after: nil, forward: true, in: filtered)
            }
        }
    }

    private var filtered: [ZZSelectOption] { ZZSelectOption.matching(query, in: options) }

    private var menu: some View {
        let rows = filtered
        let content = VStack(spacing: 4) {
            if searchable {
                ZZTextField(
                    "Search \(title)", text: $query, contentType: .search, clearable: true, focus: $searchFocused
                )
                .onSubmit { if let option = rows.first(where: { $0.id == cursor }) { commit(option) } }
                .padding(4)
                ZZSeparator()
            }
            if rows.isEmpty {
                VStack(spacing: 8) {
                    Image(systemName: "tray").font(.system(size: 28))
                    Text(query.isEmpty ? "No options" : "No matches").font(theme.font(size: 12))
                }
                .foregroundStyle(theme.foreground.muted().color)
                .frame(maxWidth: .infinity)
                .padding(24)
            } else {
                ScrollViewReader { proxy in
                    ScrollView {
                        LazyVStack(spacing: 0) {
                            ForEach(rows) { option in
                                VStack(alignment: .leading, spacing: 0) {
                                    if let group = option.group, beginsGroup(option, in: rows) {
                                        Text(group)
                                            .font(theme.font(size: 11, weight: .medium))
                                            .foregroundStyle(theme.foreground.muted().color)
                                            .padding(.horizontal, 8)
                                            .padding(.top, 10)
                                            .padding(.bottom, 4)
                                    }
                                    row(option)
                                }
                                .id(option.id)
                            }
                        }
                    }
                    .frame(
                        height: min(
                            menuMaxHeight,
                            CGFloat(rows.count) * (size.height + 2) + CGFloat(Set(rows.compactMap(\.group)).count) * 28)
                    )
                    .onChange(of: cursor) { if let cursor { proxy.scrollTo(cursor) } }
                    .onAppear { if let cursor { proxy.scrollTo(cursor, anchor: .center) } }
                }
            }
        }
        .padding(4)
        .frame(width: max(180, triggerWidth))
        .background(theme.background.raised(1).color)
        return focusedMenu(content)
            .onKeyPress(.downArrow) {
                move(forward: true)
                return .handled
            }
            .onKeyPress(.upArrow) {
                move(forward: false)
                return .handled
            }
            .onKeyPress(.return) {
                if let option = rows.first(where: { $0.id == cursor }) { commit(option) }
                return .handled
            }
            .onKeyPress(.escape) {
                dismiss()
                return .handled
            }
            .onChange(of: query) { cursor = ZZSelectOption.next(after: nil, forward: true, in: filtered) }
    }

    @ViewBuilder private func focusedMenu(_ content: some View) -> some View {
        if searchable {
            content
                .defaultFocus($searchFocused, true)
                .onAppear { searchFocused = true }
        } else {
            content
                .focusable()
                .focused($menuFocused)
                .focusEffectDisabled()
                .defaultFocus($menuFocused, true)
                .onAppear { menuFocused = true }
        }
    }

    private func row(_ option: ZZSelectOption) -> some View {
        Button {
            commit(option)
        } label: {
            HStack(spacing: 8) {
                if let systemImage = option.systemImage {
                    Image(systemName: systemImage).font(.system(size: size.fontSize)).accessibilityHidden(true)
                }
                VStack(alignment: .leading, spacing: 2) {
                    Text(option.title).lineLimit(1)
                    if let detail = option.detail {
                        Text(detail).font(theme.font(size: 11)).foregroundStyle(theme.foreground.muted().color)
                            .lineLimit(1)
                    }
                }
                Spacer(minLength: 8)
                Image(systemName: "checkmark")
                    .font(.system(size: 10, weight: .semibold))
                    .opacity(option.id == selection ? 1 : 0)
                    .accessibilityHidden(true)
            }
            .font(theme.font(size: size.fontSize))
            .foregroundStyle(theme.foreground.color)
            .padding(.horizontal, 8)
            .frame(minHeight: size.height)
            .padding(.vertical, option.detail == nil ? 1 : 4)
            .frame(maxWidth: .infinity)
            .background(
                option.id == cursor ? theme.background.raised(2).color : .clear,
                in: ZZRoundedRectangle(radius: theme.radius)
            )
            .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .disabled(option.disabled)
        .opacity(option.disabled ? 0.5 : 1)
        .onHover { hovering in if hovering && !option.disabled { cursor = option.id } }
        .accessibilityAddTraits(option.id == selection ? .isSelected : [])
    }

    private func beginsGroup(_ option: ZZSelectOption, in rows: [ZZSelectOption]) -> Bool {
        guard let index = rows.firstIndex(where: { $0.id == option.id }), index > 0 else { return true }
        return rows[index - 1].group != option.group
    }

    private func open() {
        guard isEnabled else { return }
        query = ""
        cursor =
            options.first(where: { $0.id == selection && !$0.disabled })?.id
            ?? ZZSelectOption.next(after: nil, forward: true, in: options)
        presented = true
    }

    private func dismiss() {
        presented = false
        triggerFocused = true
    }

    private func commit(_ option: ZZSelectOption) {
        guard isEnabled && !option.disabled else { return }
        selectedOption = option
        selection = option.id
        dismiss()
    }

    private func move(forward: Bool) {
        guard isEnabled else { return }
        cursor = ZZSelectOption.next(after: cursor, forward: forward, in: filtered)
    }

    private func rememberSelection() {
        if selection == nil {
            selectedOption = nil
        } else if let option = options.first(where: { $0.id == selection }) {
            selectedOption = option
        }
    }
}
