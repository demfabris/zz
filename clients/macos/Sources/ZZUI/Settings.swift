import SwiftUI

public struct ZZSettingsNavigationItem: Identifiable, Hashable, Sendable {
    public let id: String
    public let title: String
    public let icon: String
    public let group: String

    public init(_ id: String, title: String, icon: String, group: String) {
        self.id = id
        self.title = title
        self.icon = icon
        self.group = group
    }
}

public struct ZZSettingsNavigation: View {
    @Environment(\.zzTheme) private var theme
    @Binding private var selection: String
    private let items: [ZZSettingsNavigationItem]

    public init(selection: Binding<String>, items: [ZZSettingsNavigationItem]) {
        _selection = selection
        self.items = items
    }

    private var groups: [String] {
        items.reduce(into: []) { result, item in if !result.contains(item.group) { result.append(item.group) } }
    }

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                ForEach(groups, id: \.self) { group in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(group).font(theme.font(size: 11, weight: .medium))
                            .foregroundStyle(theme.foreground.muted().color).padding(.horizontal, 12).padding(
                                .bottom, 4)
                        ForEach(items.filter { $0.group == group }) { item in
                            ZZWorkspaceTreeRow(item.title, icon: item.icon, selected: selection == item.id) {
                                selection = item.id
                            }
                        }
                    }
                }
            }.padding(.vertical, 12)
        }.frame(minWidth: 160, idealWidth: 200, maxWidth: 240).background(.ultraThinMaterial)
    }
}

public struct ZZSettingsPage<Content: View>: View {
    @Environment(\.zzTheme) private var theme
    private let title: String
    private let description: String
    private let content: Content

    public init(_ title: String, description: String = "", @ViewBuilder content: () -> Content) {
        self.title = title
        self.description = description
        self.content = content()
    }

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(title).font(theme.font(size: 20, weight: .medium))
                    if !description.isEmpty {
                        Text(description).font(theme.font(size: 11)).foregroundStyle(theme.foreground.muted().color)
                    }
                }
                content
            }.frame(maxWidth: 960).frame(maxWidth: .infinity).padding(14)
        }
    }
}

public struct ZZSettingsStack<Content: View>: View {
    @Environment(\.zzTheme) private var theme
    private let title: String?
    private let description: String?
    private let content: Content

    public init(_ title: String? = nil, description: String? = nil, @ViewBuilder content: () -> Content) {
        self.title = title
        self.description = description
        self.content = content()
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if let title {
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(theme.font(size: 12, weight: .medium))
                    if let description {
                        Text(description).font(theme.font(size: 11)).foregroundStyle(theme.foreground.muted().color)
                    }
                }.padding(.horizontal, 2)
            }
            VStack(spacing: 0) { content }.zzControlSurface()
        }
    }
}

public struct ZZSettingEntry<Control: View, Detail: View>: View {
    @Environment(\.zzTheme) private var theme
    private let title: String
    private let description: String
    private let icon: String?
    private let provenance: String?
    private let enabled: Bool
    private let reset: (() -> Void)?
    private let control: Control
    private let detail: Detail

    public init(
        _ title: String, description: String = "", icon: String? = nil, provenance: String? = nil,
        enabled: Bool = true, reset: (() -> Void)? = nil, @ViewBuilder control: () -> Control,
        @ViewBuilder detail: () -> Detail
    ) {
        self.title = title
        self.description = description
        self.icon = icon
        self.provenance = provenance
        self.enabled = enabled
        self.reset = reset
        self.control = control()
        self.detail = detail()
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 16) {
                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 4) {
                        if let icon { Image(systemName: icon).font(.system(size: 14)).accessibilityHidden(true) }
                        Text(title).font(theme.font(size: 13))
                        if let provenance { ZZTag(provenance, variant: .secondary, outline: true, size: .small) }
                        if let reset {
                            ZZButton(
                                "Reset \(title)", icon: "arrow.uturn.backward", variant: .ghost, size: .xSmall,
                                flat: true, iconOnly: true, action: reset
                            ).help("Reset \(title)")
                        }
                    }
                    if !description.isEmpty {
                        Text(description).font(theme.font(size: 11)).foregroundStyle(theme.foreground.muted().color)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }.frame(maxWidth: .infinity, alignment: .leading)
                control.fixedSize(horizontal: true, vertical: false)
            }
            detail
        }
        .padding(.horizontal, 12).padding(.vertical, 11).disabled(!enabled).opacity(enabled ? 1 : 0.5)
    }
}

extension ZZSettingEntry where Detail == EmptyView {
    public init(
        _ title: String, description: String = "", icon: String? = nil, provenance: String? = nil,
        enabled: Bool = true, reset: (() -> Void)? = nil, @ViewBuilder control: () -> Control
    ) {
        self.init(
            title, description: description, icon: icon, provenance: provenance, enabled: enabled,
            reset: reset, control: control
        ) { EmptyView() }
    }
}

public struct ZZSettingsDivider: View {
    public init() {}
    public var body: some View { ZZSeparator().padding(.horizontal, 12) }
}

public enum ZZThemePreviewMode: String, CaseIterable, Identifiable, Sendable {
    case system = "System"
    case light = "Light"
    case dark = "Dark"
    public var id: Self { self }
}

public struct ZZThemeTile: View {
    @Environment(\.zzTheme) private var theme
    private let mode: ZZThemePreviewMode
    private let selected: Bool
    private let action: () -> Void

    public init(_ mode: ZZThemePreviewMode, selected: Bool = false, action: @escaping () -> Void) {
        self.mode = mode
        self.selected = selected
        self.action = action
    }

    public var body: some View {
        ZZPickerTile(mode.rawValue, selected: selected, action: action) {
            ZStack {
                ZZPalettePreview(mode == .dark ? .dark : .light)
                if mode == .system {
                    ZZPalettePreview(.dark).mask {
                        HStack(spacing: 0) {
                            Color.clear
                            Rectangle()
                        }
                    }
                }
            }
        }
    }
}

/// One chroma palette as a tile, drawn from its own colors rather than the current theme.
public struct ZZPaletteTile: View {
    private let name: String
    private let palette: ZZTheme
    private let selected: Bool
    private let action: () -> Void

    public init(_ name: String, palette: ZZTheme, selected: Bool = false, action: @escaping () -> Void) {
        self.name = name
        self.palette = palette
        self.selected = selected
        self.action = action
    }

    public var body: some View {
        ZZPickerTile(name, selected: selected, action: action) { ZZPalettePreview(palette) }
    }
}

/// A row of picker tiles that scrolls sideways once it outgrows its container.
public struct ZZPickerStrip<Content: View>: View {
    private let content: Content

    public init(@ViewBuilder content: () -> Content) { self.content = content() }

    public var body: some View {
        ScrollView(.horizontal) { HStack(alignment: .top, spacing: 10) { content }.padding(12) }
            .scrollIndicators(.hidden)
    }
}

struct ZZPickerTile<Preview: View>: View {
    @Environment(\.zzTheme) private var theme
    private let label: String
    private let selected: Bool
    private let action: () -> Void
    private let preview: Preview

    init(_ label: String, selected: Bool, action: @escaping () -> Void, @ViewBuilder preview: () -> Preview) {
        self.label = label
        self.selected = selected
        self.action = action
        self.preview = preview()
    }

    var body: some View {
        Button(action: action) {
            VStack(spacing: 6) {
                preview
                    .frame(width: 84, height: 56).clipShape(ZZRoundedRectangle(radius: theme.radius))
                    .overlay {
                        ZZRoundedRectangle(radius: theme.radius).stroke(
                            (selected ? theme.accent : theme.foreground.opacity(0.1)).color,
                            lineWidth: selected ? 2 : 0.5)
                    }
                HStack(spacing: 4) {
                    Text(label).font(theme.font(size: 11))
                    if selected { Image(systemName: "checkmark.circle.fill").font(.system(size: 11)) }
                }
            }
        }.buttonStyle(.plain).accessibilityAddTraits(selected ? [.isSelected] : [])
    }
}

/// The window mockup the theme and palette tiles share, painted from `palette`.
public struct ZZPalettePreview: View {
    private let palette: ZZTheme

    public init(_ palette: ZZTheme) { self.palette = palette }

    public var body: some View {
        HStack(spacing: 0) {
            VStack(alignment: .leading, spacing: 4) {
                RoundedRectangle(cornerRadius: 1).fill(palette.foreground.wash().color).frame(height: 3)
                RoundedRectangle(cornerRadius: 1).fill(palette.foreground.fill().color).frame(height: 3)
                Spacer(minLength: 0)
            }.padding(4).frame(width: 20).background(palette.background.raised(2).color)
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 2) {
                    Circle().fill(palette.danger.color).frame(width: 3, height: 3)
                    Circle().fill(palette.warning.color).frame(width: 3, height: 3)
                    Circle().fill(palette.success.color).frame(width: 3, height: 3)
                }
                RoundedRectangle(cornerRadius: 2).fill(palette.background.raised(1).color)
                    .overlay(alignment: .topLeading) {
                        VStack(alignment: .leading, spacing: 3) {
                            palette.foreground.wash().color.frame(width: 25, height: 2)
                            palette.foreground.fill().color.frame(width: 35, height: 2)
                            palette.foreground.fill().color.frame(width: 18, height: 2)
                            Capsule().fill(palette.accent.color).frame(width: 10, height: 4)
                        }.padding(5)
                    }
            }.padding(4).background(palette.background.color)
        }.accessibilityHidden(true)
    }
}
