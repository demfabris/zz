import SwiftUI

public enum ZZButtonVariant: Sendable, Equatable {
    case `default`, primary, secondary, danger, success, warning, ghost, link, text
    case custom(ZZColor)

    public static let standard: [Self] = [
        .default, .primary, .secondary, .danger, .success, .warning, .ghost, .link, .text,
    ]

    public var title: String {
        switch self {
        case .default: "Default"
        case .primary: "Primary"
        case .secondary: "Secondary"
        case .danger: "Danger"
        case .success: "Success"
        case .warning: "Warning"
        case .ghost: "Ghost"
        case .link: "Link"
        case .text: "Text"
        case .custom: "Custom"
        }
    }

    var isUnpadded: Bool { self == .link || self == .text }
    var isNeutral: Bool { self == .default || self == .secondary || self == .ghost }

    func color(in theme: ZZTheme) -> ZZColor {
        switch self {
        case .danger: theme.danger
        case .success: theme.success
        case .warning: theme.warning
        case .custom(let color): color
        default: theme.foreground
        }
    }
}

public struct ZZButton: View {
    @FocusState private var focused: Bool
    private let title: String
    private let icon: String?
    private let variant: ZZButtonVariant
    private let size: ZZControlSize
    private let isLoading: Bool
    private let selected: Bool
    private let flat: Bool
    private let outline: Bool
    private let iconOnly: Bool
    private let dropdown: Bool
    private let action: () -> Void
    private var compactIcon = false

    public init(
        _ title: String, icon: String? = nil, variant: ZZButtonVariant = .default,
        size: ZZControlSize = .small, isLoading: Bool = false, selected: Bool = false,
        flat: Bool = false, outline: Bool = false, iconOnly: Bool = false,
        dropdown: Bool = false, action: @escaping () -> Void
    ) {
        self.title = title
        self.icon = icon
        self.variant = variant
        self.size = size
        self.isLoading = isLoading
        self.selected = selected
        self.flat = flat
        self.outline = outline
        self.iconOnly = iconOnly
        self.dropdown = dropdown
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            HStack(spacing: size == .xSmall || size == .small ? 4 : 8) {
                if isLoading {
                    ZZSpinner(size: glyphSize)
                } else if let icon {
                    Image(systemName: icon)
                        .font(.system(size: glyphSize, weight: .regular))
                        .frame(width: glyphSize, height: glyphSize)
                        .offset(y: compactIcon ? 0.5 : 0)
                        .accessibilityHidden(true)
                }
                if !iconOnly {
                    Text(title).lineLimit(1).underline(variant == .link)
                }
                if dropdown {
                    Image(systemName: "chevron.down").font(.system(size: 10, weight: .medium)).accessibilityHidden(true)
                }
            }
        }
        .buttonStyle(
            ZZButtonStyle(
                variant: variant, size: size, selected: selected, flat: flat,
                outline: outline, iconOnly: iconOnly, focused: focused)
        )
        .focused($focused)
        .disabled(isLoading)
        .accessibilityLabel(title)
        .accessibilityValue(isLoading ? "In progress" : "")
        .accessibilityAddTraits(selected ? .isSelected : [])
    }

    private var glyphSize: CGFloat { compactIcon ? 14 : size.iconSize }

    func compactChromeIcon() -> Self {
        var result = self
        result.compactIcon = true
        return result
    }
}

public struct ZZIconButton: View {
    private let title: String
    private let systemName: String
    private let selected: Bool
    private let flat: Bool
    private let action: () -> Void

    public init(
        _ title: String, systemName: String, selected: Bool = false, flat: Bool = false, action: @escaping () -> Void
    ) {
        self.title = title
        self.systemName = systemName
        self.selected = selected
        self.flat = flat
        self.action = action
    }

    public var body: some View {
        ZZButton(
            title, icon: systemName, variant: .ghost, size: .xSmall,
            selected: selected, flat: flat, iconOnly: true, action: action
        )
        .compactChromeIcon()
        .help(title)
    }
}

public struct ZZButtonStyle: ButtonStyle {
    public var variant: ZZButtonVariant
    public var size: ZZControlSize
    public var selected: Bool
    public var flat: Bool
    public var outline: Bool
    public var iconOnly: Bool
    public var focused: Bool

    public init(
        variant: ZZButtonVariant = .default, size: ZZControlSize = .small,
        selected: Bool = false, flat: Bool = false, outline: Bool = false,
        iconOnly: Bool = false, focused: Bool = false
    ) {
        self.variant = variant
        self.size = size
        self.selected = selected
        self.flat = flat
        self.outline = outline
        self.iconOnly = iconOnly
        self.focused = focused
    }

    public func makeBody(configuration: Configuration) -> some View {
        ZZButtonBody(configuration: configuration, style: self)
    }
}

private struct ZZButtonBody: View {
    @Environment(\.zzTheme) private var theme
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.colorSchemeContrast) private var contrast
    @State private var hovered = false
    let configuration: ButtonStyle.Configuration
    let style: ZZButtonStyle

    private var active: Bool { isEnabled && (configuration.isPressed || style.selected) }
    private var highlighted: Bool { active || (isEnabled && hovered) }
    private var radius: CGFloat { theme.radius }

    private var foreground: ZZColor {
        if !isEnabled { return theme.foreground.muted().opacity(0.5) }
        let base = style.variant == .primary && !style.outline ? theme.foreground.on() : style.variant.color(in: theme)
        if isEnabled && style.variant == .text && active { return base.opacity(0.7) }
        return base
    }

    private var background: ZZColor {
        let variant = style.variant
        let root = variant.color(in: theme)
        if variant.isUnpadded { return .clear }
        if variant.isNeutral && highlighted { return theme.background.washed(2) }
        if style.outline && !variant.isNeutral {
            return root.opacity(!isEnabled ? 0.05 : active ? 0.4 : highlighted ? 0.2 : 0.1)
        }
        if !isEnabled {
            switch variant {
            case .primary, .custom: return root.opacity(0.15)
            case .danger, .success, .warning: return root.fill().opacity(0.15)
            case .secondary: return theme.background.raised(2)
            case .default: return theme.background.raised(1).opacity(0.5)
            case .ghost, .link, .text: return .clear
            }
        }
        let base: ZZColor
        switch variant {
        case .default: base = theme.background.raised(1)
        case .primary: base = theme.foreground
        case .secondary: base = theme.background.raised(2)
        case .ghost, .link, .text: base = .clear
        case .custom: base = root.opacity(active ? 0.4 : highlighted ? 0.3 : 0.2)
        default: base = root.fill()
        }
        let resolved = active ? base.active() : highlighted ? base.hover() : base
        return resolved
    }

    private var border: ZZColor {
        if style.flat || style.variant.isUnpadded { return .clear }
        if style.variant.isNeutral {
            if style.variant == .ghost && !highlighted { return .clear }
            return theme.foreground.opacity(contrast == .increased ? 0.5 : 0.1)
        }
        return style.outline ? style.variant.color(in: theme).outline() : .clear
    }

    var body: some View {
        configuration.label
            .font(theme.font(size: buttonFontSize))
            .padding(.horizontal, style.iconOnly || style.variant.isUnpadded ? 0 : horizontalPadding)
            .frame(
                width: style.iconOnly ? style.size.height : nil,
                height: style.variant.isUnpadded ? nil : style.size.height
            )
            .foregroundStyle(foreground.color)
            .background(background.color, in: ZZRoundedRectangle(radius: radius))
            .overlay { ZZRoundedRectangle(radius: radius).stroke(border.color, lineWidth: style.outline ? 1 : 0.5) }
            .shadow(
                color: theme.scrim.opaque().opacity(hasShadow && theme.shadows ? 0.2 * theme.shadowStrength : 0).color,
                radius: 1, x: 0, y: 1
            )
            .contentShape(ZZRoundedRectangle(radius: radius))
            .overlay {
                if style.focused {
                    ZZRoundedRectangle(radius: radius + 2)
                        .stroke(theme.foreground.glow().color, lineWidth: 2)
                        .padding(-2)
                        .allowsHitTesting(false)
                }
            }
            .onHover { hovered = $0 }
            .animation(reduceMotion ? nil : .easeOut(duration: 0.12), value: highlighted)
    }

    private var hasShadow: Bool {
        isEnabled && !style.flat && (style.variant == .default || (style.variant.isNeutral && highlighted))
    }

    private var horizontalPadding: CGFloat {
        switch style.size {
        case .xSmall: 4
        case .small: 12
        case .medium, .large: 16
        }
    }

    private var buttonFontSize: CGFloat {
        switch style.size {
        case .xSmall: 12
        case .small: 14
        case .medium, .large: 16
        }
    }
}
