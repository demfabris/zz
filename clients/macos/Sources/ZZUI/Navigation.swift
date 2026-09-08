import SwiftUI

public enum ZZWorkspaceMetrics {
    public static let rowHeight: CGFloat = 32
    public static let indentWidth: CGFloat = 20
    public static let contentInset: CGFloat = 8
    public static let markerWidth: CGFloat = 18
    public static let iconSize: CGFloat = 14
    public static let sidebarWidth: CGFloat = 256
    public static let statusHeight: CGFloat = 24
    public static let titlebarHeight: CGFloat = 35
}

public struct ZZWorkspaceTreeRow<Actions: View>: View {
    @Environment(\.zzTheme) private var theme
    @State private var hovered = false
    private let title: String
    private let icon: String
    private let depth: Int
    private let active: Bool
    private let selected: Bool
    private let focused: Bool
    private let connected: Bool
    private let emphasized: Bool
    private let hoverActions: Bool
    private let expanded: Binding<Bool>?
    private let bell: Bool
    private let badge: ZZColor?
    private let connecting: Bool
    private let connectionDetail: String?
    private let showConnectionDetail: () -> Void
    private let action: () -> Void
    private let actions: Actions

    public init(
        _ title: String, icon: String, depth: Int = 0, active: Bool = false, selected: Bool = false,
        focused: Bool = true, connected: Bool = true, emphasized: Bool = true, hoverActions: Bool = true,
        expanded: Binding<Bool>? = nil, bell: Bool = false, badge: ZZColor? = nil,
        connecting: Bool = false, connectionDetail: String? = nil, showConnectionDetail: @escaping () -> Void = {},
        action: @escaping () -> Void,
        @ViewBuilder actions: () -> Actions
    ) {
        self.title = title
        self.icon = icon
        self.depth = max(0, depth)
        self.active = active
        self.selected = selected
        self.focused = focused
        self.connected = connected
        self.emphasized = emphasized
        self.hoverActions = hoverActions
        self.expanded = expanded
        self.bell = bell
        self.badge = badge
        self.connecting = connecting
        self.connectionDetail = connectionDetail
        self.showConnectionDetail = showConnectionDetail
        self.action = action
        self.actions = actions()
    }

    public var body: some View {
        HStack(spacing: 6) {
            if let expanded {
                Button {
                    expanded.wrappedValue.toggle()
                } label: {
                    ZZWorkspaceMarker(
                        hovered ? (expanded.wrappedValue ? "chevron.down" : "chevron.right") : icon,
                        bell: bell, badge: badge
                    ).frame(height: 32)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("\(expanded.wrappedValue ? "Collapse" : "Expand") \(title)")
                .accessibilityValue(expanded.wrappedValue ? "Expanded" : "Collapsed")
            } else {
                ZZWorkspaceMarker(icon, bell: bell, badge: badge)
            }
            Button(action: action) {
                Text(title).font(theme.font(size: 14, weight: active ? .medium : .regular))
                    .lineLimit(1).frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityAddTraits(selected || active ? [.isSelected] : [])
            if connecting || connectionDetail != nil {
                ZZHostIndicator(connecting: connecting, detail: connectionDetail, action: showConnectionDetail)
            }
            actions.opacity(!hoverActions || hovered ? 1 : 0)
                .allowsHitTesting(!hoverActions || hovered)
                .accessibilityHidden(hoverActions && !hovered)
        }
        .foregroundStyle((connected && emphasized ? theme.foreground : theme.foreground.muted()).color)
        .padding(.leading, 8 + CGFloat(depth) * 20).padding(.trailing, 4)
        .frame(height: 32)
        .background {
            ZZRoundedRectangle(radius: theme.radius)
                .fill(theme.background.washed(2).color.opacity(highlighted ? 1 : 0))
                .overlay {
                    ZZRoundedRectangle(radius: theme.radius)
                        .strokeBorder(
                            theme.foreground.opacity(theme.shadows && highlighted ? 0.1 : 0).color,
                            lineWidth: 0.5
                        )
                }
                .shadow(
                    color: theme.scrim.opaque().opacity(
                        theme.shadows && highlighted ? 0.2 * theme.shadowStrength : 0
                    ).color,
                    radius: 1, y: 1
                )
                .padding(.horizontal, theme.radius > 0 ? 4 : 0).padding(.vertical, 1)
        }
        .onHover { hovered = $0 }
    }

    private var highlighted: Bool { hovered || selected || active && focused }
}

extension ZZWorkspaceTreeRow where Actions == EmptyView {
    public init(
        _ title: String, icon: String, depth: Int = 0, active: Bool = false, selected: Bool = false,
        focused: Bool = true, connected: Bool = true, emphasized: Bool = true,
        expanded: Binding<Bool>? = nil, bell: Bool = false, badge: ZZColor? = nil,
        connecting: Bool = false, connectionDetail: String? = nil, showConnectionDetail: @escaping () -> Void = {},
        action: @escaping () -> Void
    ) {
        self.init(
            title, icon: icon, depth: depth, active: active, selected: selected, focused: focused, connected: connected,
            emphasized: emphasized, expanded: expanded, bell: bell, badge: badge, connecting: connecting,
            connectionDetail: connectionDetail, showConnectionDetail: showConnectionDetail, action: action
        ) { EmptyView() }
    }
}

public struct ZZWorkspaceActionRow: View {
    @Environment(\.zzTheme) private var theme
    @State private var hovered = false
    private let title: String
    private let icon: String
    private let depth: Int
    private let action: () -> Void

    public init(_ title: String, icon: String = "plus", depth: Int = 0, action: @escaping () -> Void) {
        self.title = title
        self.icon = icon
        self.depth = max(0, depth)
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                Image(systemName: icon).frame(width: 18).foregroundStyle(theme.foreground.muted().color)
                Text(title)
                Spacer(minLength: 0)
            }
            .font(theme.font(size: 14)).padding(.leading, 8 + CGFloat(depth) * 20).padding(.trailing, 8)
            .frame(height: 32).contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .foregroundStyle((hovered ? theme.foreground : theme.foreground.muted()).color)
        .onHover { hovered = $0 }
    }
}

public struct ZZWorkspaceSidebar<Titlebar: View, Content: View>: View {
    @Environment(\.zzTheme) private var theme
    private let transparent: Bool
    private let titlebar: Titlebar
    private let content: Content

    public init(
        transparent: Bool = false, @ViewBuilder titlebar: () -> Titlebar, @ViewBuilder content: () -> Content
    ) {
        self.transparent = transparent
        self.titlebar = titlebar()
        self.content = content()
    }

    public var body: some View {
        VStack(spacing: 0) {
            titlebar.frame(height: 35).frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 8)
            ScrollView { content.padding(.bottom, 8) }.frame(maxHeight: .infinity)
        }
        .frame(minWidth: 160, idealWidth: ZZWorkspaceMetrics.sidebarWidth, maxWidth: 640)
        .background {
            if !transparent { Rectangle().fill(.ultraThinMaterial) }
        }
        .overlay(alignment: .trailing) {
            if !transparent { theme.border.raised(2).color.frame(width: 1) }
        }
    }
}

public struct ZZWorkspaceIndentGuide: Identifiable, Sendable {
    public let id: String
    public let level: Int
    public let startRow: Int
    public let rowCount: Int
    public let active: Bool

    public init(_ id: String, level: Int, startRow: Int, rowCount: Int, active: Bool = false) {
        self.id = id
        self.level = max(0, level)
        self.startRow = max(0, startRow)
        self.rowCount = max(0, rowCount)
        self.active = active
    }
}

public struct ZZWorkspaceIndentGuides: View {
    @Environment(\.zzTheme) private var theme
    @Environment(\.displayScale) private var displayScale
    private let guides: [ZZWorkspaceIndentGuide]

    public init(_ guides: [ZZWorkspaceIndentGuide]) { self.guides = guides }

    public var body: some View {
        Canvas { context, _ in
            for guide in guides {
                let x = 17 + CGFloat(guide.level) * 20
                let y = CGFloat(guide.startRow) * 32 + 4
                let end = CGFloat(guide.startRow + guide.rowCount) * 32 - 4
                guard end > y else { continue }
                let path = Path { path in
                    path.move(to: CGPoint(x: x, y: y))
                    path.addLine(to: CGPoint(x: x, y: end))
                }
                context.stroke(
                    path,
                    with: .color((guide.active ? theme.foreground.muted() : theme.foreground.muted().wash()).color),
                    lineWidth: 1 / displayScale)
            }
        }.allowsHitTesting(false).accessibilityHidden(true)
    }
}

public struct ZZWorkspaceStatusItem: View {
    @Environment(\.zzTheme) private var theme
    private let text: String
    private let icon: String?

    public init(_ text: String, icon: String? = nil) {
        self.text = text
        self.icon = icon
    }

    public var body: some View {
        HStack(spacing: 5) {
            if let icon { Image(systemName: icon).font(.system(size: 13)).accessibilityHidden(true) }
            Text(text).font(theme.font(size: 12)).lineLimit(1)
        }
        .frame(maxWidth: 180).frame(height: 24).foregroundStyle(theme.foreground.muted().color)
        .help(text)
    }
}

public struct ZZWorkspaceStatusWindow: View {
    @Environment(\.zzTheme) private var theme
    @State private var hovered = false
    private let index: String
    private let name: String
    private let active: Bool
    private let connected: Bool
    private let bell: Bool
    private let activity: Bool
    private let agent: Bool
    private let action: () -> Void
    private let close: (() -> Void)?
    private let rename: (() -> Void)?

    public init(
        _ name: String, index: String, active: Bool = false, connected: Bool = true,
        bell: Bool = false, activity: Bool = false, agent: Bool = false,
        close: (() -> Void)? = nil, rename: (() -> Void)? = nil, action: @escaping () -> Void
    ) {
        self.name = name
        self.index = index
        self.active = active
        self.connected = connected
        self.bell = bell
        self.activity = activity
        self.agent = agent
        self.action = action
        self.close = close
        self.rename = rename
    }

    public var body: some View {
        HStack(spacing: 0) {
            Button(action: action) {
                HStack(spacing: 5) {
                    Text(index).font(theme.font(size: 12)).foregroundStyle(theme.foreground.muted().color)
                    Text(name).font(theme.font(size: 13)).lineLimit(1)
                    if agent { Image(systemName: "cpu").font(.system(size: 13)) }
                    if bell { Circle().fill(theme.warning.color).frame(width: 5, height: 5) }
                    if activity { Circle().fill(theme.success.color).frame(width: 5, height: 5) }
                }
                .padding(.horizontal, 9).frame(minWidth: 36, maxWidth: .infinity).frame(height: 24)
                .contentShape(Rectangle())
            }.buttonStyle(.plain).accessibilityLabel("Window \(index), \(name)")
                .accessibilityAddTraits(active ? [.isSelected] : [])
                .contextMenu {
                    if connected {
                        if let rename { Button("Rename window", action: rename) }
                        if let close { Button("Close window", systemImage: "xmark", action: close) }
                    }
                }
            if let close {
                ZZIconButton("Close window \(name)", systemName: "xmark", action: close)
                    .opacity(hovered ? 1 : 0).allowsHitTesting(hovered).accessibilityHidden(!hovered)
            }
        }
        .background(
            active || hovered ? theme.background.washed(2).color : .clear,
            in: ZZRoundedRectangle(radius: theme.radius)
        )
        .frame(minWidth: 36, maxWidth: 180).frame(height: 24)
        .contentShape(Rectangle()).disabled(!connected)
        .foregroundStyle((active || hovered ? theme.foreground : theme.foreground.muted()).color)
        .onHover { hovered = $0 }.help("Window \(index): \(name)")
        .accessibilityLabel("Window \(index), \(name)")
        .accessibilityValue(
            [
                active ? "Active" : nil, bell ? "Bell" : nil, activity ? "Activity" : nil,
                agent ? "Agent running" : nil,
            ].compactMap { $0 }.joined(separator: ", ")
        )
        .accessibilityAddTraits(active ? [.isSelected] : [])
    }
}
