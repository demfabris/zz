import Observation
import SwiftUI
import UIKit

enum ZZAppAppearance: String, CaseIterable, Identifiable {
    case system, light, dark

    var id: Self { self }

    var label: String {
        switch self {
        case .system: "System"
        case .light: "Light"
        case .dark: "Dark"
        }
    }

    var colorScheme: ColorScheme? {
        switch self {
        case .system: nil
        case .light: .light
        case .dark: .dark
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
    var paddingX: CGFloat = 8
    var paddingY: CGFloat = 8
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
    var appearance: ZZAppAppearance {
        didSet { defaults.set(appearance.rawValue, forKey: Keys.appearance) }
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

    let shared: ZZSharedSettings?

    @ObservationIgnored private let defaults: UserDefaults

    var chromeTint: Color { .accentColor }
    var chromeBackground: Color { Color(uiColor: .secondarySystemBackground) }
    var chromeForeground: Color { .primary }
    var chromeSurface: Color { Color(uiColor: .tertiarySystemBackground) }
    var chromeBorder: Color { Color(uiColor: .separator) }
    var chromeSecondaryForeground: Color { .secondary }
    var widgetCornerRadius: CGFloat { 8 }
    var shadowOpacity: Double { 0.2 }
    var terminalPresentation: ZZTerminalPresentation {
        ZZTerminalPresentation(
            font: terminalFont,
            pointSize: CGFloat(terminalFontSize),
            cursorBlinking: true,
            selectionBackground: shared?.mobileAppearance?.selection_background,
            selectionForeground: shared?.mobileAppearance?.selection_foreground,
            searchMatchColor: shared?.mobileAppearance?.search_match_color,
            searchCurrentColor: shared?.mobileAppearance?.search_current_color,
            copyCursorColor: shared?.mobileAppearance?.copy_cursor_color
        )
    }

    init(defaults: UserDefaults = .standard, configDirectory: URL? = nil) {
        self.defaults = defaults
        appearance = ZZAppAppearance(rawValue: defaults.string(forKey: Keys.appearance) ?? "") ?? .system
        shared = defaults === UserDefaults.standard || configDirectory != nil
            ? ZZSharedSettings(directory: configDirectory) : nil
        savedTerminalFont = ZZTerminalFont(
            rawValue: defaults.string(forKey: Keys.terminalFont) ?? ""
        ) ?? .systemMono
        savedTerminalFontSize = Self.clampFontSize(
            Self.integer(defaults.object(forKey: Keys.terminalFontSize))
                ?? Self.defaultTerminalFontSize
        )
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

    private enum Keys {
        static let appearance = "zz.client.appearance"
        static let terminalFont = "zz.client.terminal.font"
        static let terminalFontSize = "zz.client.terminal.font-size"
    }
}
