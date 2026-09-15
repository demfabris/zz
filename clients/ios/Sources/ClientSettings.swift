import Observation
import SwiftUI
import UIKit

enum ZZAppAppearance: String, CaseIterable, Identifiable, Sendable {
    case system
    case dark
    case light

    var id: Self { self }

    var label: String {
        switch self {
        case .system: "System"
        case .dark: "Dark"
        case .light: "Light"
        }
    }

    var colorScheme: ColorScheme? {
        switch self {
        case .system: nil
        case .dark: .dark
        case .light: .light
        }
    }

    var interfaceStyle: UIUserInterfaceStyle {
        switch self {
        case .system: .unspecified
        case .dark: .dark
        case .light: .light
        }
    }
}

enum ZZWindowAppearance {
    @MainActor
    static func apply(_ appearance: ZZAppAppearance) {
        let style = appearance.interfaceStyle
        for scene in UIApplication.shared.connectedScenes {
            guard let windowScene = scene as? UIWindowScene else {
                continue
            }
            for window in windowScene.windows {
                window.overrideUserInterfaceStyle = style
            }
        }
    }
}

enum ZZTerminalFont: String, CaseIterable, Identifiable, Sendable {
    case systemMono = "system-mono"
    case menlo
    case firaCode = "fira-code"
    case geistMono = "geist-mono"
    case proto = "0xproto"
    case courierNew = "courier-new"

    var id: Self { self }

    var label: String {
        switch self {
        case .systemMono: "System Mono"
        case .menlo: "Menlo"
        case .firaCode: "Fira Code"
        case .geistMono: "Geist Mono"
        case .proto: "0xProto"
        case .courierNew: "Courier New"
        }
    }

    var configFamily: String {
        switch self {
        case .systemMono: "monospace"
        default: label
        }
    }

    func uiFont(
        size: CGFloat,
        bold: Bool = false,
        italic: Bool = false
    ) -> UIFont {
        let pointSize = size.isFinite && size > 0 ? size : 13
        let systemFallback = Self.systemFont(
            size: pointSize,
            bold: bold,
            italic: italic
        )

        let fontName: String?
        switch (self, bold, italic) {
        case (.systemMono, _, _):
            return systemFallback
        case (.firaCode, _, _), (.geistMono, _, _), (.proto, _, _):
            let family = self == .firaCode ? "FiraCode" : self == .geistMono ? "GeistMono" : "0xProto"
            let style = bold ? "Bold" : self == .proto && italic ? "Italic" : "Regular"
            let base = UIFont(name: "\(family)-\(style)", size: pointSize) ?? systemFallback
            guard italic else { return base }
            var traits = base.fontDescriptor.symbolicTraits
            traits.insert(.traitItalic)
            guard let descriptor = base.fontDescriptor.withSymbolicTraits(traits) else { return base }
            return UIFont(descriptor: descriptor, size: pointSize)
        case (.menlo, false, false):
            fontName = "Menlo-Regular"
        case (.menlo, true, false):
            fontName = "Menlo-Bold"
        case (.menlo, false, true):
            fontName = "Menlo-Italic"
        case (.menlo, true, true):
            fontName = "Menlo-BoldItalic"
        case (.courierNew, false, false):
            fontName = "CourierNewPSMT"
        case (.courierNew, true, false):
            fontName = "CourierNewPS-BoldMT"
        case (.courierNew, false, true):
            fontName = "CourierNewPS-ItalicMT"
        case (.courierNew, true, true):
            fontName = "CourierNewPS-BoldItalicMT"
        }

        return fontName.flatMap { UIFont(name: $0, size: pointSize) }
            ?? systemFallback
    }

    static func drawingAttributes(font: UIFont, foreground: UIColor, italic: Bool) -> [NSAttributedString.Key: Any] {
        var attributes: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: foreground]
        if italic && !font.fontDescriptor.symbolicTraits.contains(.traitItalic) {
            attributes[.obliqueness] = 0.2
        }
        return attributes
    }

    func swiftUIFont(size: CGFloat) -> Font {
        let pointSize = size.isFinite && size > 0 ? size : 13
        switch self {
        case .systemMono:
            return .system(size: pointSize, design: .monospaced)
        case .firaCode, .geistMono, .proto:
            return Font(uiFont(size: pointSize))
        case .menlo:
            return .custom("Menlo-Regular", fixedSize: pointSize)
        case .courierNew:
            return .custom("CourierNewPSMT", fixedSize: pointSize)
        }
    }

    private static func systemFont(
        size: CGFloat,
        bold: Bool,
        italic: Bool
    ) -> UIFont {
        let base = UIFont.monospacedSystemFont(
            ofSize: size,
            weight: bold ? .bold : .regular
        )
        guard italic else {
            return base
        }
        var traits = base.fontDescriptor.symbolicTraits
        traits.insert(.traitItalic)
        if bold {
            traits.insert(.traitBold)
        }
        guard let descriptor = base.fontDescriptor.withSymbolicTraits(traits) else {
            return base
        }
        return UIFont(descriptor: descriptor, size: size)
    }
}

struct ZZTerminalPresentation: Equatable, Sendable {
    let font: ZZTerminalFont
    let pointSize: CGFloat
    let cursorBlinking: Bool
    var backgroundOpacity: Double = 1
    var paddingX: CGFloat = 0
    var paddingY: CGFloat = 0
    var cursorStyle: String? = nil
    var blinkInterval: Double = 0.6
    var fontWeight: Int = 400
    var selectionBackground: UInt32? = nil
    var selectionForeground: UInt32? = nil
    var searchMatchColor: UInt32? = nil
    var searchCurrentColor: UInt32? = nil
    var copyCursorColor: UInt32? = nil

    static let `default` = ZZTerminalPresentation(
        font: .systemMono,
        pointSize: 13,
        cursorBlinking: true
    )
}

extension EnvironmentValues {
    @Entry var zzTerminalPresentation: ZZTerminalPresentation = .default
}

@MainActor
@Observable
final class ZZClientSettings {
    static let terminalFontSizeRange = 9...23
    static let defaultTerminalFontSize = 13
    static let terminalPaddingRange = 0.0...16.0
    static let paneCornerRadii = [0.0, 13.5, 24.0]

    var appearance: ZZAppAppearance {
        didSet {
            defaults.set(appearance.rawValue, forKey: Keys.appearance)
        }
    }

    private var savedTerminalFont: ZZTerminalFont
    private var savedTerminalFontSize: Int

    var terminalFont: ZZTerminalFont {
        get {
            guard let shared,
                  shared.snapshot?.settings.first(where: { $0.key == "font-family" })?.overridden == true
            else { return savedTerminalFont }
            let family = shared.text("font-family")
            return ZZTerminalFont.allCases.first { $0.configFamily.caseInsensitiveCompare(family) == .orderedSame }
                ?? savedTerminalFont
        }
        set {
            savedTerminalFont = newValue
            defaults.set(newValue.rawValue, forKey: Keys.terminalFont)
            shared?.action("set-appearance", ["key": "font-family", "value": newValue.configFamily])
        }
    }

    var terminalFontSize: Int {
        get {
            if let shared,
               shared.snapshot?.settings.first(where: { $0.key == "font-size" })?.overridden == true,
               let size = shared.value("font-size").number, size.isFinite {
                return Self.clampFontSize(Int(size))
            }
            return savedTerminalFontSize
        }
        set {
            savedTerminalFontSize = Self.clampFontSize(newValue)
            defaults.set(savedTerminalFontSize, forKey: Keys.terminalFontSize)
            shared?.action("set-appearance", ["key": "font-size", "value": String(savedTerminalFontSize)])
        }
    }

    private var savedCursorBlinking: Bool

    var cursorBlinking: Bool {
        get {
            guard let shared,
                  shared.snapshot?.settings.first(where: { $0.key == "cursor-style-blink" })?.overridden == true
            else { return savedCursorBlinking }
            return !["off", "false"].contains(shared.text("cursor-style-blink"))
        }
        set {
            savedCursorBlinking = newValue
            defaults.set(newValue, forKey: Keys.cursorBlinking)
            shared?.action("set-appearance", ["key": "cursor-style-blink", "value": newValue ? "on" : "off"])
        }
    }

    var extendPanesUnderHomeIndicator: Bool {
        didSet {
            defaults.set(
                extendPanesUnderHomeIndicator,
                forKey: Keys.extendPanesUnderHomeIndicator
            )
        }
    }

    let shared: ZZSharedSettings?

    @ObservationIgnored private let defaults: UserDefaults

    var chromeTint: Color { chromeColor("chrome-accent", preset: \.accent, fallback: .accentColor) }
    var chromeBackground: Color { chromeColor("chrome-background", preset: \.background, fallback: Color(uiColor: .systemBackground)) }
    var chromeForeground: Color { chromeColor("chrome-foreground", preset: \.foreground, fallback: .primary) }
    var chromeSurface: Color { chromeBackground }
    var chromeBorder: Color { chromeForeground.opacity(0.14) }
    var chromeSecondaryForeground: Color { chromeForeground.opacity(0.65) }
    var chromeContrast: Double { shared?.number("chrome-contrast", fallback: 1) ?? 1 }
    var widgetCornerRadius: CGFloat { CGFloat(shared?.number("widget-corner-radius", fallback: 8) ?? 8) }
    var shadowOpacity: Double { min(1, max(0, (shared?.number("shadow-strength", fallback: 1) ?? 1) * 0.2)) }
    var paneCornerRadius: CGFloat {
        let saved = shared?.number("pane-corner-radius", fallback: 13.5) ?? 13.5
        return CGFloat(Self.paneCornerRadii.min { abs($0 - saved) < abs($1 - saved) } ?? 13.5)
    }

    var interfaceFont: Font {
        let family = shared?.text("ui-font-family") ?? ""
        return family.isEmpty || family == "System" ? .body : .custom(family, size: 17, relativeTo: .body)
    }

    private func chromeColor(_ key: String, preset: KeyPath<ZZSettingsSnapshot.ChromePreset, String>, fallback: Color) -> Color {
        guard let shared else { return fallback }
        let override = shared.text(key)
        if !override.isEmpty { return Color(zzHex: override) }
        let selected = shared.text(shared.dark ? "chrome-preset-dark" : "chrome-preset-light")
        guard let theme = shared.snapshot?.presets.first(where: { $0.id == selected }) else { return fallback }
        return Color(zzHex: theme[keyPath: preset])
    }

    var terminalPresentation: ZZTerminalPresentation {
        ZZTerminalPresentation(
            font: terminalFont,
            pointSize: CGFloat(terminalFontSize),
            cursorBlinking: cursorBlinking,
            backgroundOpacity: (shared?.mobileAppearance?.background_opacity ?? 1) * (shared?.number("pane-background-opacity", fallback: 1) ?? 1),
            paddingX: CGFloat(min(Self.terminalPaddingRange.upperBound, max(0, shared?.mobileAppearance?.padding.dropFirst().first ?? 0))),
            paddingY: CGFloat(min(Self.terminalPaddingRange.upperBound, max(0, shared?.mobileAppearance?.padding.first ?? 0))),
            cursorStyle: shared?.mobileAppearance?.cursor_style,
            blinkInterval: Double(shared?.mobileAppearance?.cursor_blink_ms ?? 600) / 1_000,
            fontWeight: Int(shared?.mobileAppearance?.font_weight ?? 400),
            selectionBackground: shared?.mobileAppearance?.selection_background,
            selectionForeground: shared?.mobileAppearance?.selection_foreground,
            searchMatchColor: shared?.mobileAppearance?.search_match_color,
            searchCurrentColor: shared?.mobileAppearance?.search_current_color,
            copyCursorColor: shared?.mobileAppearance?.copy_cursor_color
        )
    }

    init(defaults: UserDefaults = .standard, configDirectory: URL? = nil) {
        self.defaults = defaults
        shared = defaults === UserDefaults.standard || configDirectory != nil
            ? ZZSharedSettings(directory: configDirectory) : nil
        appearance = ZZAppAppearance(
            rawValue: defaults.string(forKey: Keys.appearance) ?? ""
        ) ?? .dark
        savedTerminalFont = ZZTerminalFont(
            rawValue: defaults.string(forKey: Keys.terminalFont) ?? ""
        ) ?? .systemMono
        savedTerminalFontSize = Self.clampFontSize(
            Self.integer(defaults.object(forKey: Keys.terminalFontSize))
                ?? Self.defaultTerminalFontSize
        )
        savedCursorBlinking = Self.boolean(
            defaults.object(forKey: Keys.cursorBlinking)
        ) ?? true
        extendPanesUnderHomeIndicator = Self.boolean(
            defaults.object(forKey: Keys.extendPanesUnderHomeIndicator)
        ) ?? false
        if let shared,
           Self.boolean(defaults.object(forKey: Keys.cursorBlinking)) != nil,
           shared.snapshot?.settings.first(where: { $0.key == "cursor-style-blink" })?.overridden == false {
            shared.action("set-appearance", ["key": "cursor-style-blink", "value": savedCursorBlinking ? "on" : "off"])
        }
    }

    func restoreDefaults() {
        shared?.restoreDefaults()
        appearance = .dark
        terminalFont = .systemMono
        terminalFontSize = Self.defaultTerminalFontSize
        cursorBlinking = true
        extendPanesUnderHomeIndicator = false
    }

    private static func clampFontSize(_ value: Int) -> Int {
        min(max(value, terminalFontSizeRange.lowerBound), terminalFontSizeRange.upperBound)
    }

    private static func integer(_ value: Any?) -> Int? {
        guard let number = value as? NSNumber,
              CFGetTypeID(number) != CFBooleanGetTypeID()
        else {
            return nil
        }
        return number.intValue
    }

    private static func boolean(_ value: Any?) -> Bool? {
        guard let number = value as? NSNumber,
              CFGetTypeID(number) == CFBooleanGetTypeID()
        else {
            return nil
        }
        return number.boolValue
    }

    private enum Keys {
        static let appearance = "zz.client.appearance"
        static let terminalFont = "zz.client.terminal.font"
        static let terminalFontSize = "zz.client.terminal.font-size"
        static let cursorBlinking = "zz.client.terminal.cursor-blinking"
        static let extendPanesUnderHomeIndicator =
            "zz.client.ipad.extend-panes-under-home-indicator"
    }
}
