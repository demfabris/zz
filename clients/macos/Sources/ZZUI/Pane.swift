import SwiftUI

public struct ZZPaneCorners: Equatable, Sendable {
    public var topLeft: CGFloat
    public var topRight: CGFloat
    public var bottomLeft: CGFloat
    public var bottomRight: CGFloat

    public init(topLeft: CGFloat, topRight: CGFloat, bottomLeft: CGFloat, bottomRight: CGFloat) {
        self.topLeft = topLeft
        self.topRight = topRight
        self.bottomLeft = bottomLeft
        self.bottomRight = bottomRight
    }

    public init(_ radius: CGFloat) {
        self.init(topLeft: radius, topRight: radius, bottomLeft: radius, bottomRight: radius)
    }
}

struct ZZPaneShape: InsettableShape {
    let corners: ZZPaneCorners
    private var insetAmount: CGFloat = 0

    init(_ corners: ZZPaneCorners) { self.corners = corners }

    func path(in rect: CGRect) -> Path {
        func radius(_ value: CGFloat) -> CGFloat {
            max(0, ZZRoundedRectangle.resolvedRadius(value, in: rect.size) - insetAmount)
        }
        return UnevenRoundedRectangle(
            topLeadingRadius: radius(corners.topLeft), bottomLeadingRadius: radius(corners.bottomLeft),
            bottomTrailingRadius: radius(corners.bottomRight), topTrailingRadius: radius(corners.topRight),
            style: .continuous
        ).path(in: rect.insetBy(dx: insetAmount, dy: insetAmount))
    }

    func inset(by amount: CGFloat) -> Self {
        var result = self
        result.insetAmount += amount
        return result
    }
}

public struct ZZPane<Content: View, Overlay: View>: View {
    @Environment(\.zzTheme) private var theme
    private let active: Bool
    private let gap: CGFloat
    private let inactiveOpacity: Double
    private let borderWidth: CGFloat?
    private let radius: CGFloat
    private let corners: ZZPaneCorners?
    private let shadow: Bool
    private let content: Content
    private let overlay: Overlay

    public init(
        active: Bool = false, gap: CGFloat = 8, inactiveOpacity: Double = 1,
        borderWidth: CGFloat? = nil, radius: CGFloat = 13.5, corners: ZZPaneCorners? = nil, shadow: Bool = true,
        @ViewBuilder content: () -> Content, @ViewBuilder overlay: () -> Overlay
    ) {
        self.active = active
        self.gap = max(0, gap)
        self.inactiveOpacity = min(1, max(0, inactiveOpacity))
        self.borderWidth = borderWidth.map { max(0, $0) }
        self.radius = max(0, radius)
        self.corners = corners
        self.shadow = shadow
        self.content = content()
        self.overlay = overlay()
    }

    public var body: some View {
        ZStack {
            content.frame(maxWidth: .infinity, maxHeight: .infinity)
            theme.background.color.opacity(1 - inactiveOpacity).allowsHitTesting(false)
            overlay
        }
        .background(theme.background.color)
        .clipShape(ZZPaneShape(corners ?? ZZPaneCorners(gap > 0 ? radius : 0)))
        .overlay {
            ZZPaneShape(corners ?? ZZPaneCorners(gap > 0 ? radius : 0))
                .strokeBorder(
                    (active ? theme.accent : theme.foreground.opacity(0.1)).color,
                    lineWidth: borderWidth ?? 0.5
                )
                .allowsHitTesting(false)
        }
        .shadow(
            color: theme.scrim.opaque().opacity(
                (gap > 0 || corners != nil) && shadow && theme.shadows ? 0.2 * theme.shadowStrength : 0
            ).color,
            radius: 1, y: 1
        )
        .padding(gap / 2)
    }
}

extension ZZPane where Overlay == EmptyView {
    public init(
        active: Bool = false, gap: CGFloat = 8, inactiveOpacity: Double = 1,
        borderWidth: CGFloat? = nil, radius: CGFloat = 13.5, corners: ZZPaneCorners? = nil, shadow: Bool = true,
        @ViewBuilder content: () -> Content
    ) {
        self.init(
            active: active, gap: gap, inactiveOpacity: inactiveOpacity, borderWidth: borderWidth,
            radius: radius, corners: corners, shadow: shadow, content: content
        ) { EmptyView() }
    }
}

public struct ZZPanePickerRow: View {
    @Environment(\.zzTheme) private var theme
    @State private var hovered = false
    private let title: String
    private let icon: String
    private let shortcut: String
    private let selected: Bool
    private let enabled: Bool
    private let action: () -> Void

    public init(
        _ title: String, icon: String, shortcut: String, selected: Bool = false, enabled: Bool = true,
        action: @escaping () -> Void
    ) {
        self.title = title
        self.icon = icon
        self.shortcut = shortcut
        self.selected = selected
        self.enabled = enabled
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                Image(systemName: icon).font(.system(size: 16)).foregroundStyle(theme.foreground.muted().color)
                Text(title).font(theme.font(size: 12, weight: .medium)).lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
                ZZKbd(shortcut.lowercased(), backgroundElevation: 4)
            }
            .padding(.horizontal, 12).frame(height: 40)
            .background(
                theme.background.washed((selected || hovered) && enabled ? 2 : 1).color,
                in: ZZRoundedRectangle(radius: theme.radius)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain).disabled(!enabled).opacity(enabled ? 1 : 0.4)
        .foregroundStyle(theme.foreground.color).onHover { hovered = $0 }
        .accessibilityAddTraits(selected ? [.isSelected] : [])
    }
}

public struct ZZPanePicker<Rows: View>: View {
    private let rows: Rows

    public init(@ViewBuilder rows: () -> Rows) { self.rows = rows() }

    public var body: some View {
        VStack(spacing: 4) { rows }.frame(maxWidth: 360).padding(.horizontal, 12)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

public struct ZZFloatingSurface<Content: View>: View {
    @Environment(\.zzTheme) private var theme
    private let title: String?
    private let content: Content

    public init(_ title: String? = nil, @ViewBuilder content: () -> Content) {
        self.title = title
        self.content = content()
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if let title { Text(title).font(theme.font(size: 11, monospaced: true)) }
            content
        }
        .padding(12).zzSurface()
        .shadow(color: theme.scrim.color.opacity(theme.shadows ? theme.shadowStrength : 0), radius: 24, y: 8)
    }
}

public enum ZZPaneSplitAxis: String, CaseIterable, Sendable {
    case horizontal, vertical
}

public struct ZZPaneSplit<First: View, Second: View>: View {
    private let axis: ZZPaneSplitAxis
    private let first: First
    private let second: Second

    public init(
        _ axis: ZZPaneSplitAxis = .horizontal, @ViewBuilder first: () -> First,
        @ViewBuilder second: () -> Second
    ) {
        self.axis = axis
        self.first = first()
        self.second = second()
    }

    public var body: some View {
        if axis == .horizontal {
            HSplitView {
                first.frame(minWidth: 120)
                second.frame(minWidth: 120)
            }
        } else {
            VSplitView {
                first.frame(minHeight: 100)
                second.frame(minHeight: 100)
            }
        }
    }
}

public enum ZZPaneDragState: String, CaseIterable, Identifiable, Sendable {
    case armed, source, destination
    public var id: Self { self }
}

public struct ZZPaneDragOverlay: View {
    @Environment(\.zzTheme) private var theme
    private let state: ZZPaneDragState

    public init(_ state: ZZPaneDragState) { self.state = state }

    public var body: some View {
        ZZRoundedRectangle(radius: theme.radius)
            .fill(fill.color)
            .overlay {
                if state == .destination {
                    ZZRoundedRectangle(radius: theme.radius).stroke(theme.foreground.color, lineWidth: 1)
                }
            }
            .accessibilityLabel(state == .destination ? "Drop pane here" : "Move pane")
            .allowsHitTesting(false)
    }

    private var fill: ZZColor {
        switch state {
        case .armed: theme.border.opacity(0.08)
        case .source: theme.background.opacity(0.3)
        case .destination: theme.foreground.fill()
        }
    }
}

public struct ZZPaneDragChip: View {
    @Environment(\.zzTheme) private var theme
    private let pane: String
    private let title: String

    public init(_ pane: String, title: String) {
        self.pane = pane
        self.title = title
    }

    public var body: some View {
        HStack(spacing: 8) {
            Image(systemName: "rectangle.on.rectangle")
            Text(pane).monospaced()
            Text(title).lineLimit(1)
        }
        .font(theme.font(size: 12)).padding(.horizontal, 12).padding(.vertical, 8).zzSurface()
    }
}

public struct ZZPaneOverlayStack<Content: View>: View {
    private let alignment: Alignment
    private let content: Content

    public init(alignment: Alignment = .topTrailing, @ViewBuilder content: () -> Content) {
        self.alignment = alignment
        self.content = content()
    }

    public var body: some View {
        VStack(alignment: .trailing, spacing: 6) { content }
            .padding(8).frame(maxWidth: .infinity, maxHeight: .infinity, alignment: alignment)
    }
}

public struct ZZPaneIndicator: View {
    @Environment(\.zzTheme) private var theme
    private let index: String
    private let key: String
    private let active: Bool
    private let action: () -> Void

    public init(_ index: String, key: String, active: Bool = false, action: @escaping () -> Void) {
        self.index = index
        self.key = key
        self.active = active
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            VStack(spacing: 4) {
                Text(index).font(theme.font(size: 18, weight: .bold, monospaced: true))
                ZZKbd(key)
            }
            .frame(minWidth: 40).padding(.horizontal, 8).padding(.vertical, 6)
            .background(
                (active ? theme.danger.fill() : theme.foreground.wash()).color,
                in: ZZRoundedRectangle(radius: theme.radius))
        }
        .buttonStyle(.plain).foregroundStyle((active ? theme.danger : theme.foreground).color)
        .shadow(color: theme.scrim.color.opacity(theme.shadows ? theme.shadowStrength : 0), radius: 16, y: 6)
        .accessibilityLabel("Select pane \(index), shortcut \(key)")
    }
}

public struct ZZFrameRateBadge: View {
    @Environment(\.zzTheme) private var theme
    private let label: String
    private let fps: Double?

    public init(_ label: String, fps: Double?) {
        self.label = label
        self.fps = fps
    }

    public static func formattedRate(_ fps: Double?) -> String {
        guard let fps, fps.isFinite else { return "--.-" }
        return String(format: "%.1f", fps)
    }

    public var body: some View {
        Text("\(label) \(Self.formattedRate(fps)) FPS")
            .font(theme.font(size: 10, monospaced: true)).padding(.horizontal, 7).frame(height: 22).zzSurface()
    }
}

public struct ZZTerminalSearch: View {
    @Environment(\.zzTheme) private var theme
    @Binding private var query: String
    @FocusState private var focused: Bool
    private let matches: String
    private let previous: () -> Void
    private let next: () -> Void
    private let close: () -> Void

    public init(
        query: Binding<String>, matches: String, previous: @escaping () -> Void,
        next: @escaping () -> Void, close: @escaping () -> Void
    ) {
        _query = query
        self.matches = matches
        self.previous = previous
        self.next = next
        self.close = close
    }

    public var body: some View {
        HStack(spacing: 6) {
            Image(systemName: "magnifyingglass")
            TextField("Find in terminal", text: $query).textFieldStyle(.plain).focused($focused).onSubmit(next)
            Text(matches).font(theme.font(size: 10)).foregroundStyle(theme.foreground.muted().color)
            ZZIconButton("Previous match", systemName: "chevron.up", action: previous)
            ZZIconButton("Next match", systemName: "chevron.down", action: next)
            ZZIconButton("Close search", systemName: "xmark", action: close)
        }
        .font(theme.font(size: 11)).padding(.leading, 8).padding(.trailing, 2).frame(height: 30)
        .frame(maxWidth: 560).zzControlSurface(focused: focused)
        .onExitCommand(perform: close)
    }
}

public struct ZZTerminalStatus: View {
    @Environment(\.zzTheme) private var theme
    private let label: String?
    private let detail: String

    public init(_ detail: String, label: String? = nil) {
        self.detail = detail
        self.label = label
    }

    public var body: some View {
        HStack(spacing: 6) {
            if let label { Text(label).fontWeight(.medium) }
            Text(detail).lineLimit(1)
        }
        .font(theme.font(size: 11)).foregroundStyle(theme.foreground.color)
        .padding(.horizontal, 8).padding(.vertical, 4).frame(maxWidth: 560, alignment: .trailing).zzSurface()
        .help(detail)
    }
}
