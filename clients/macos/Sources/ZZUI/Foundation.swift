import AppKit
import SwiftUI

public struct ZZColor: Sendable, Hashable {
    public var red: Double
    public var green: Double
    public var blue: Double
    public var alpha: Double

    public init(red: Double, green: Double, blue: Double, alpha: Double = 1) {
        self.red = red
        self.green = green
        self.blue = blue
        self.alpha = alpha
    }

    public init(hue: Double, saturation: Double, lightness: Double, alpha: Double = 1) {
        let h = hue / 360
        let s = saturation / 100
        let l = lightness / 100
        let a = s * min(l, 1 - l)
        func channel(_ n: Double) -> Double {
            let k = (n + h * 12).truncatingRemainder(dividingBy: 12)
            return l - a * max(-1, min(k - 3, min(9 - k, 1)))
        }
        self.init(red: channel(0), green: channel(8), blue: channel(4), alpha: alpha)
    }

    public init?(hex: String) {
        var digits = hex.trimmingCharacters(in: .whitespacesAndNewlines)
        if digits.hasPrefix("#") { digits.removeFirst() }
        guard [3, 6, 8].contains(digits.count), digits.allSatisfy({ $0.isASCII && $0.isHexDigit }) else { return nil }
        if digits.count == 3 { digits = digits.map { "\($0)\($0)" }.joined() }
        guard let value = UInt32(digits, radix: 16) else { return nil }
        let hasAlpha = digits.count == 8
        let rgb = hasAlpha ? value >> 8 : value
        self.init(
            red: Double((rgb >> 16) & 255) / 255,
            green: Double((rgb >> 8) & 255) / 255,
            blue: Double(rgb & 255) / 255,
            alpha: hasAlpha ? Double(value & 255) / 255 : 1)
    }

    public var hex: String {
        let channels = [red, green, blue] + (alpha < 1 ? [alpha] : [])
        return "#" + channels.map { String(format: "%02x", Int(($0.clamped * 255).rounded())) }.joined()
    }

    public var color: Color { Color(.sRGB, red: red, green: green, blue: blue, opacity: alpha) }
    var nsColor: NSColor { NSColor(srgbRed: red, green: green, blue: blue, alpha: alpha) }
    public var lightness: Double { (max(red, green, blue) + min(red, green, blue)) / 2 }
    public var oklabLightness: Double { oklab.0 }

    public func raised(_ level: Int) -> Self { towardContrast(0.04 * Double(max(0, level))) }
    public func washed(_ level: Int) -> Self { contrastPole.opacity(0.04 * Double(max(0, level))) }
    public func hover() -> Self { towardContrast(0.06) }
    public func active() -> Self { towardContrast(0.12) }
    public func muted() -> Self { towardContrast(0.35) }
    public func on() -> Self { contrastPole.towardContrast(0.06) }
    public func fill() -> Self { opacity(0.12) }
    public func outline() -> Self { opacity(0.55) }
    public func subtle() -> Self { opacity(0.75) }
    public func glow() -> Self { opacity(0.35) }
    public func wash() -> Self { opacity(0.28) }
    public func floating() -> Self { opacity(0.9) }
    public func opaque() -> Self { Self(red: red, green: green, blue: blue) }
    public func opacity(_ factor: Double) -> Self {
        Self(red: red, green: green, blue: blue, alpha: alpha * factor.clamped)
    }

    public func mix(_ other: Self, weight: Double) -> Self {
        let weight = weight.clamped
        let first = weight * alpha
        let second = (1 - weight) * other.alpha
        let resultAlpha = first + second
        guard resultAlpha > 0 else { return .clear }
        let a = oklab
        let b = other.oklab
        return Self.fromOklab(
            (a.0 * first + b.0 * second) / resultAlpha,
            (a.1 * first + b.1 * second) / resultAlpha,
            (a.2 * first + b.2 * second) / resultAlpha,
            alpha: resultAlpha
        )
    }

    public static let clear = Self(red: 0, green: 0, blue: 0, alpha: 0)

    private var contrastPole: Self {
        let value: Double = lightness < 0.5 ? 1 : 0
        return Self(red: value, green: value, blue: value)
    }

    private func towardContrast(_ amount: Double) -> Self {
        var result = mix(contrastPole, weight: 1 - amount.clamped)
        result.alpha = alpha
        return result
    }

    private var oklab: (Double, Double, Double) {
        func linear(_ value: Double) -> Double {
            value <= 0.04045 ? value / 12.92 : pow((value + 0.055) / 1.055, 2.4)
        }
        let r = linear(red)
        let g = linear(green)
        let b = linear(blue)
        let l = cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b)
        let m = cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b)
        let s = cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b)
        return (
            0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
            1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
            0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s
        )
    }

    private static func fromOklab(_ lightness: Double, _ a: Double, _ b: Double, alpha: Double) -> Self {
        func srgb(_ value: Double) -> Double {
            (value <= 0.0031308 ? value * 12.92 : 1.055 * pow(value, 1 / 2.4) - 0.055).clamped
        }
        let l = pow(lightness + 0.3963377774 * a + 0.2158037573 * b, 3)
        let m = pow(lightness - 0.1055613458 * a - 0.0638541728 * b, 3)
        let s = pow(lightness - 0.0894841775 * a - 1.2914855480 * b, 3)
        return Self(
            red: srgb(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
            green: srgb(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
            blue: srgb(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s),
            alpha: alpha
        )
    }
}

extension Double {
    fileprivate var clamped: Double { min(1, max(0, self)) }
}

public struct ZZTheme: Sendable, Equatable {
    public var background: ZZColor
    public var foreground: ZZColor
    public var border: ZZColor { background.mix(foreground, weight: 0.86).opaque() }
    public var accent: ZZColor
    public var success: ZZColor
    public var warning: ZZColor
    public var danger: ZZColor
    public var scrim: ZZColor
    public var radius: CGFloat = 6
    public var fontFamily: String?
    public var monospacedFontFamily: String?
    public var shadows: Bool = true
    public var shadowStrength: Double = 1

    public init(
        background: ZZColor, foreground: ZZColor, accent: ZZColor,
        success: ZZColor, warning: ZZColor, danger: ZZColor, scrim: ZZColor, radius: CGFloat = 6,
        fontFamily: String? = nil, monospacedFontFamily: String? = nil,
        shadows: Bool = true, shadowStrength: Double = 1
    ) {
        self.background = background
        self.foreground = foreground
        self.accent = accent
        self.success = success
        self.warning = warning
        self.danger = danger
        self.scrim = scrim
        self.radius = radius
        self.fontFamily = fontFamily
        self.monospacedFontFamily = monospacedFontFamily
        self.shadows = shadows
        self.shadowStrength = max(0, shadowStrength)
    }

    public func font(size: CGFloat, weight: Font.Weight = .regular, monospaced: Bool = false) -> Font {
        if let family = monospaced ? monospacedFontFamily : fontFamily, !family.isEmpty {
            return .custom(family, fixedSize: size).weight(weight)
        }
        return .system(size: size, weight: weight, design: monospaced ? .monospaced : .default)
    }

    public func nsFont(size: CGFloat, weight: NSFont.Weight = .regular, monospaced: Bool = false) -> NSFont {
        if let family = monospaced ? monospacedFontFamily : fontFamily,
            let custom = NSFont(name: family, size: size)
        {
            return weight >= .semibold ? NSFontManager.shared.convert(custom, toHaveTrait: .boldFontMask) : custom
        }
        return monospaced
            ? .monospacedSystemFont(ofSize: size, weight: weight) : .systemFont(ofSize: size, weight: weight)
    }

    public static let light = Self(
        background: ZZColor(hue: 0, saturation: 0, lightness: 100),
        foreground: ZZColor(hue: 0, saturation: 0, lightness: 3.9),
        accent: ZZColor(hue: 221.2, saturation: 83.2, lightness: 53.3),
        success: ZZColor(hue: 142.1, saturation: 70.6, lightness: 45.3),
        warning: ZZColor(hue: 45.4, saturation: 93.4, lightness: 47.5),
        danger: ZZColor(hue: 0, saturation: 84.2, lightness: 60.2),
        scrim: ZZColor(red: 0, green: 0, blue: 0, alpha: 0.05)
    )

    public static let dark = Self(
        background: ZZColor(hue: 0, saturation: 0, lightness: 3.9),
        foreground: ZZColor(hue: 0, saturation: 0, lightness: 98),
        accent: ZZColor(hue: 213.1, saturation: 93.9, lightness: 67.8),
        success: ZZColor(hue: 141.9, saturation: 69.2, lightness: 58),
        warning: ZZColor(hue: 47.9, saturation: 95.8, lightness: 53.1),
        danger: ZZColor(hue: 0, saturation: 90.6, lightness: 70.8),
        scrim: ZZColor(red: 0, green: 0, blue: 0, alpha: 0.2)
    )

    public static let macOSClassicLight = Self(
        background: ZZColor(hex: "#ffffff")!, foreground: ZZColor(hex: "#1a1a1a")!,
        accent: ZZColor(hex: "#007aff")!, success: ZZColor(hex: "#036a07")!,
        warning: ZZColor(hex: "#9e7008")!, danger: ZZColor(hex: "#c5060b")!,
        scrim: light.scrim
    )

    public static let macOSClassicDark = Self(
        background: ZZColor(hex: "#131313")!, foreground: ZZColor(hex: "#caccca")!,
        accent: ZZColor(hex: "#0a84ff")!, success: ZZColor(hex: "#62ba46")!,
        warning: ZZColor(hex: "#b0a878")!, danger: ZZColor(hex: "#d2602d")!,
        scrim: dark.scrim
    )
}

extension EnvironmentValues {
    @Entry public var zzTheme: ZZTheme = .light
}

public struct ZZThemeContainer<Content: View>: View {
    @Environment(\.colorScheme) private var colorScheme
    private let theme: ZZTheme?
    private let content: Content

    public init(theme: ZZTheme? = nil, @ViewBuilder content: () -> Content) {
        self.theme = theme
        self.content = content()
    }

    public var body: some View {
        let resolved = theme ?? (colorScheme == .dark ? .dark : .light)
        content.environment(\.zzTheme, resolved)
            .foregroundStyle(resolved.foreground.color)
            .tint(resolved.foreground.color)
            .font(resolved.font(size: 13))
    }
}

public enum ZZControlSize: String, CaseIterable, Sendable, Identifiable {
    case xSmall = "XS"
    case small = "S"
    case medium = "M"
    case large = "L"
    public var id: Self { self }
    public var height: CGFloat {
        switch self {
        case .xSmall: 24
        case .small: 28
        case .medium: 36
        case .large: 40
        }
    }
    public var fontSize: CGFloat {
        switch self {
        case .xSmall: 12
        case .small: 13
        case .medium: 14
        case .large: 16
        }
    }
    public var iconSize: CGFloat {
        switch self {
        case .xSmall: 12
        case .small: 14
        case .medium: 16
        case .large: 24
        }
    }
    public var horizontalPadding: CGFloat {
        switch self {
        case .xSmall: 4
        case .small: 8
        case .medium: 12
        case .large: 16
        }
    }
}

public enum ZZTone: String, CaseIterable, Sendable, Identifiable {
    case neutral, success, warning, danger
    public var id: Self { self }
    public func color(in theme: ZZTheme) -> ZZColor {
        switch self {
        case .neutral: theme.foreground
        case .success: theme.success
        case .warning: theme.warning
        case .danger: theme.danger
        }
    }
    public var systemImage: String {
        switch self {
        case .neutral: "info.circle"
        case .success: "checkmark.circle"
        case .warning: "exclamationmark.triangle"
        case .danger: "xmark.octagon"
        }
    }
}

public struct ZZRoundedRectangle: InsettableShape {
    public var radius: CGFloat
    private var insetAmount: CGFloat = 0
    public init(radius: CGFloat) { self.radius = radius }
    public static func resolvedRadius(_ radius: CGFloat, in size: CGSize) -> CGFloat {
        let cap = min(size.width, size.height) * 0.45
        guard cap > 0 else { return 0 }
        return cap * tanh(max(0, radius) / cap)
    }
    public func path(in rect: CGRect) -> Path {
        RoundedRectangle(
            cornerRadius: max(0, Self.resolvedRadius(radius, in: rect.size) - insetAmount), style: .continuous
        )
        .path(in: rect.insetBy(dx: insetAmount, dy: insetAmount))
    }
    public func inset(by amount: CGFloat) -> Self {
        var result = self
        result.insetAmount += amount
        return result
    }
}

extension View {
    public func zzSurface(elevation: Int = 1) -> some View { modifier(ZZSurfaceStyle(elevation: elevation)) }
    public func zzControlSurface(focused: Bool = false, invalid: Bool = false) -> some View {
        modifier(ZZControlSurfaceStyle(focused: focused, invalid: invalid))
    }
}

private struct ZZSurfaceStyle: ViewModifier {
    @Environment(\.zzTheme) private var theme
    let elevation: Int
    func body(content: Content) -> some View {
        content.background(theme.background.raised(elevation).color, in: ZZRoundedRectangle(radius: theme.radius))
            .overlay {
                ZZRoundedRectangle(radius: theme.radius).stroke(theme.foreground.opacity(0.1).color, lineWidth: 0.5)
                    .allowsHitTesting(false)
            }
    }
}

private struct ZZControlSurfaceStyle: ViewModifier {
    @Environment(\.zzTheme) private var theme
    @Environment(\.colorSchemeContrast) private var contrast
    let focused: Bool
    let invalid: Bool
    func body(content: Content) -> some View {
        content.background(theme.background.raised(1).color, in: ZZRoundedRectangle(radius: theme.radius))
            .overlay {
                ZZRoundedRectangle(radius: theme.radius)
                    .stroke(
                        invalid
                            ? theme.danger.color : theme.foreground.opacity(contrast == .increased ? 0.5 : 0.1).color,
                        lineWidth: 0.5
                    )
                    .allowsHitTesting(false)
            }
            .shadow(
                color: theme.scrim.opaque().opacity(theme.shadows ? 0.2 * theme.shadowStrength : 0).color, radius: 1,
                x: 0, y: 1
            )
            .overlay {
                if focused {
                    ZZRoundedRectangle(radius: theme.radius + 2)
                        .stroke(theme.foreground.glow().color, lineWidth: 2)
                        .padding(-2)
                        .allowsHitTesting(false)
                }
            }
    }
}
