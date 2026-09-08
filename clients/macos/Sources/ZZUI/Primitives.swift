import SwiftUI

public struct ZZIcon: View {
    private let systemName: String
    private let size: ZZControlSize

    public init(systemName: String, size: ZZControlSize = .small) {
        self.systemName = systemName
        self.size = size
    }

    public var body: some View {
        Image(systemName: systemName).font(.system(size: size.iconSize))
            .frame(width: size.iconSize, height: size.iconSize)
    }
}

public enum ZZTagVariant: String, CaseIterable, Sendable {
    case primary, secondary, success
}

public struct ZZTag: View {
    @Environment(\.zzTheme) private var theme
    private let title: String
    private let variant: ZZTagVariant
    private let tone: ZZTone?
    private let outline: Bool
    private let size: ZZControlSize
    private let fontSize: CGFloat

    public init(_ title: String, tone: ZZTone = .neutral, fontSize: CGFloat = 12) {
        self.title = title
        self.tone = tone
        self.variant = .secondary
        self.outline = false
        self.size = .small
        self.fontSize = fontSize
    }

    public init(
        _ title: String, variant: ZZTagVariant, outline: Bool = false, size: ZZControlSize = .medium,
        fontSize: CGFloat = 12
    ) {
        self.title = title
        self.variant = variant
        self.outline = outline
        self.size = size
        self.fontSize = fontSize
        self.tone = nil
    }

    private var base: ZZColor {
        if let tone { return tone == .neutral ? theme.background.raised(2) : tone.color(in: theme) }
        switch variant {
        case .primary: return theme.foreground
        case .secondary: return theme.background.raised(2)
        case .success: return theme.success
        }
    }

    private var foreground: ZZColor {
        if let tone { return tone.color(in: theme) }
        if variant == .secondary { return outline ? theme.foreground.muted() : theme.foreground }
        return outline ? base : base.on()
    }

    public var body: some View {
        let compact = size == .xSmall || size == .small
        Text(title)
            .font(theme.font(size: fontSize))
            .foregroundStyle(foreground.color)
            .padding(.horizontal, compact ? 6 : 10)
            .padding(.vertical, compact ? 2 : 4)
            .background(
                (outline ? ZZColor.clear : tone != nil && tone != .neutral ? base.fill() : base).color,
                in: ZZRoundedRectangle(radius: theme.radius)
            )
            .overlay {
                ZZRoundedRectangle(radius: theme.radius)
                    .stroke(
                        (variant == .secondary && (tone == nil || tone == .neutral)
                            ? theme.border : tone == nil ? base : base.outline()).color, lineWidth: 1)
            }
            .fixedSize()
    }
}

public struct ZZKbd: View {
    @Environment(\.zzTheme) private var theme
    private let label: String
    private let backgroundElevation: Int

    public init(_ label: String, backgroundElevation: Int = 2) {
        self.label = label
        self.backgroundElevation = backgroundElevation
    }

    public var body: some View {
        Text(label).font(theme.font(size: 12))
            .foregroundStyle(theme.foreground.muted().color)
            .padding(.horizontal, 4).padding(.vertical, 2)
            .frame(minWidth: 20)
            .background(
                theme.background.raised(backgroundElevation).color, in: ZZRoundedRectangle(radius: theme.radius)
            )
            .fixedSize()
            .accessibilityLabel(accessibilityLabel)
    }

    private var accessibilityLabel: String {
        label.replacingOccurrences(of: "⌘", with: "Command ")
            .replacingOccurrences(of: "⇧", with: "Shift ")
            .replacingOccurrences(of: "⌥", with: "Option ")
            .replacingOccurrences(of: "⌃", with: "Control ")
            .replacingOccurrences(of: "↩", with: "Return ")
            .replacingOccurrences(of: "⎋", with: "Escape ")
    }
}

public struct ZZSpinner: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    private let size: CGFloat
    public init(size: CGFloat = 14) { self.size = size }
    public var body: some View {
        Group {
            if reduceMotion {
                Image(systemName: "hourglass").font(.system(size: size))
            } else {
                ProgressView().controlSize(.small).scaleEffect(size / 16)
            }
        }
        .frame(width: size, height: size)
        .accessibilityLabel("In progress")
    }
}

public struct ZZSeparator: View {
    @Environment(\.zzTheme) private var theme
    private let axis: Axis
    public init(axis: Axis = .horizontal) { self.axis = axis }
    public var body: some View {
        Rectangle().fill(theme.border.color)
            .frame(width: axis == .vertical ? 1 : nil, height: axis == .horizontal ? 1 : nil)
            .accessibilityHidden(true)
    }
}

public struct ZZListItem<Content: View>: View {
    @Environment(\.zzTheme) private var theme
    @Environment(\.isEnabled) private var isEnabled
    @State private var hovered = false
    private let selected: Bool
    private let action: () -> Void
    private let content: Content

    public init(selected: Bool = false, action: @escaping () -> Void, @ViewBuilder content: () -> Content) {
        self.selected = selected
        self.action = action
        self.content = content()
    }

    public var body: some View {
        Button(action: action) {
            content.frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 12).padding(.vertical, 4)
                .font(theme.font(size: 16))
                .foregroundStyle((isEnabled ? theme.foreground : theme.foreground.muted()).color)
                .background(background.color, in: ZZRoundedRectangle(radius: theme.radius))
                .overlay {
                    ZZRoundedRectangle(radius: theme.radius)
                        .stroke(selected && isEnabled ? theme.foreground.color : .clear, lineWidth: 1)
                }
                .contentShape(ZZRoundedRectangle(radius: theme.radius))
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 }
        .accessibilityAddTraits(selected ? .isSelected : [])
    }

    private var background: ZZColor {
        if selected && isEnabled { return theme.foreground.wash() }
        return hovered && isEnabled ? theme.background.hover() : .clear
    }
}

public struct ZZScrollView<Content: View>: View {
    private let axes: Axis.Set
    private let showsIndicators: Bool
    private let content: Content
    public init(_ axes: Axis.Set = .vertical, showsIndicators: Bool = true, @ViewBuilder content: () -> Content) {
        self.axes = axes
        self.showsIndicators = showsIndicators
        self.content = content()
    }
    public var body: some View {
        ScrollView(axes, showsIndicators: showsIndicators) { content }
    }
}
