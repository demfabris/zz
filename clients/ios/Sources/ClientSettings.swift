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
    case courierNew = "courier-new"

    var id: Self { self }

    var label: String {
        switch self {
        case .systemMono: "System Mono"
        case .menlo: "Menlo"
        case .courierNew: "Courier New"
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

    func swiftUIFont(size: CGFloat) -> Font {
        let pointSize = size.isFinite && size > 0 ? size : 13
        switch self {
        case .systemMono:
            return .system(size: pointSize, design: .monospaced)
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
        didSet {
            defaults.set(appearance.rawValue, forKey: Keys.appearance)
        }
    }

    var terminalFont: ZZTerminalFont {
        didSet {
            defaults.set(terminalFont.rawValue, forKey: Keys.terminalFont)
        }
    }

    var terminalFontSize: Int {
        didSet {
            let clamped = Self.clampFontSize(terminalFontSize)
            if terminalFontSize != clamped {
                terminalFontSize = clamped
            }
            defaults.set(clamped, forKey: Keys.terminalFontSize)
        }
    }

    var cursorBlinking: Bool {
        didSet {
            defaults.set(cursorBlinking, forKey: Keys.cursorBlinking)
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

    @ObservationIgnored private let defaults: UserDefaults

    var terminalPresentation: ZZTerminalPresentation {
        ZZTerminalPresentation(
            font: terminalFont,
            pointSize: CGFloat(terminalFontSize),
            cursorBlinking: cursorBlinking
        )
    }

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        appearance = ZZAppAppearance(
            rawValue: defaults.string(forKey: Keys.appearance) ?? ""
        ) ?? .dark
        terminalFont = ZZTerminalFont(
            rawValue: defaults.string(forKey: Keys.terminalFont) ?? ""
        ) ?? .systemMono
        terminalFontSize = Self.clampFontSize(
            Self.integer(defaults.object(forKey: Keys.terminalFontSize))
                ?? Self.defaultTerminalFontSize
        )
        cursorBlinking = Self.boolean(
            defaults.object(forKey: Keys.cursorBlinking)
        ) ?? true
        extendPanesUnderHomeIndicator = Self.boolean(
            defaults.object(forKey: Keys.extendPanesUnderHomeIndicator)
        ) ?? false
    }

    func restoreDefaults() {
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
