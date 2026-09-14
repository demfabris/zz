import XCTest
import UIKit
@testable import ZZ

final class ClientSettingsTests: XCTestCase {
    @MainActor
    func testDefaults() {
        withDefaults { defaults in
            let settings = ZZClientSettings(defaults: defaults)

            XCTAssertEqual(settings.appearance, .dark)
            XCTAssertEqual(settings.terminalFont, .systemMono)
            XCTAssertEqual(settings.terminalFontSize, 13)
            XCTAssertTrue(settings.cursorBlinking)
            XCTAssertFalse(settings.extendPanesUnderHomeIndicator)
            XCTAssertEqual(settings.terminalPresentation, .default)
        }
    }

    @MainActor
    func testPersistsEverySetting() {
        withDefaults { defaults in
            let settings = ZZClientSettings(defaults: defaults)
            settings.appearance = .light
            settings.terminalFont = .menlo
            settings.terminalFontSize = 19
            settings.cursorBlinking = false
            settings.extendPanesUnderHomeIndicator = true

            let reloaded = ZZClientSettings(defaults: defaults)
            XCTAssertEqual(reloaded.appearance, .light)
            XCTAssertEqual(reloaded.terminalFont, .menlo)
            XCTAssertEqual(reloaded.terminalFontSize, 19)
            XCTAssertFalse(reloaded.cursorBlinking)
            XCTAssertTrue(reloaded.extendPanesUnderHomeIndicator)
        }
    }

    @MainActor
    func testMalformedValuesFallBackAndFontSizeClamps() {
        withDefaults { defaults in
            defaults.set("neon", forKey: "zz.client.appearance")
            defaults.set("papyrus", forKey: "zz.client.terminal.font")
            defaults.set(99, forKey: "zz.client.terminal.font-size")
            defaults.set("sometimes", forKey: "zz.client.terminal.cursor-blinking")
            defaults.set("sometimes", forKey: "zz.client.ipad.extend-panes-under-home-indicator")

            let settings = ZZClientSettings(defaults: defaults)
            XCTAssertEqual(settings.appearance, .dark)
            XCTAssertEqual(settings.terminalFont, .systemMono)
            XCTAssertEqual(settings.terminalFontSize, 23)
            XCTAssertTrue(settings.cursorBlinking)
            XCTAssertFalse(settings.extendPanesUnderHomeIndicator)

            settings.terminalFontSize = -20
            XCTAssertEqual(settings.terminalFontSize, 9)
            XCTAssertEqual(ZZClientSettings(defaults: defaults).terminalFontSize, 9)

            defaults.set("giant", forKey: "zz.client.terminal.font-size")
            XCTAssertEqual(ZZClientSettings(defaults: defaults).terminalFontSize, 13)
        }
    }

    @MainActor
    func testRestoreDefaults() {
        withDefaults { defaults in
            let settings = ZZClientSettings(defaults: defaults)
            settings.appearance = .system
            settings.terminalFont = .courierNew
            settings.terminalFontSize = 21
            settings.cursorBlinking = false
            settings.extendPanesUnderHomeIndicator = true

            settings.restoreDefaults()

            XCTAssertEqual(settings.appearance, .dark)
            XCTAssertEqual(settings.terminalFont, .systemMono)
            XCTAssertEqual(settings.terminalFontSize, 13)
            XCTAssertTrue(settings.cursorBlinking)
            XCTAssertFalse(settings.extendPanesUnderHomeIndicator)
            XCTAssertEqual(ZZClientSettings(defaults: defaults).terminalPresentation, .default)
        }
    }

    @MainActor
    func testBundledFontsResolveToSelectedFamiliesAndPersist() {
        withDefaults { defaults in
            for (font, postscript) in [
                (ZZTerminalFont.firaCode, "FiraCode-Regular"),
                (.geistMono, "GeistMono-Regular"),
                (.proto, "0xProto-Regular"),
            ] {
                XCTAssertNotNil(UIFont(name: postscript, size: 13))
                XCTAssertEqual(font.uiFont(size: 13).fontName, postscript)
                XCTAssertTrue(font.uiFont(size: 13, bold: true).fontDescriptor.symbolicTraits.contains(.traitBold))
                let regular = font.uiFont(size: 40)
                let italic = font.uiFont(size: 40, italic: true)
                XCTAssertEqual(regular.familyName, italic.familyName)
                let renderer = UIGraphicsImageRenderer(size: CGSize(width: 200, height: 80))
                let uprightImage = renderer.image { _ in
                    ("MMMM" as NSString).draw(at: CGPoint(x: 10, y: 10), withAttributes:
                        ZZTerminalFont.drawingAttributes(font: regular, foreground: .black, italic: false))
                }
                let italicImage = renderer.image { _ in
                    ("MMMM" as NSString).draw(at: CGPoint(x: 10, y: 10), withAttributes:
                        ZZTerminalFont.drawingAttributes(font: italic, foreground: .black, italic: true))
                }
                XCTAssertNotEqual(uprightImage.pngData(), italicImage.pngData(),
                                  "\(font.label) italic glyphs must render differently from upright glyphs")
                let settings = ZZClientSettings(defaults: defaults)
                settings.terminalFont = font
                XCTAssertEqual(ZZClientSettings(defaults: defaults).terminalFont, font)
            }
        }
    }

    @MainActor
    func testLocalTerminalAndMuxSettingsPersistIndependently() throws {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let shared = ZZSharedSettings(directory: directory)
        let prefix = try XCTUnwrap(shared.snapshot?.settings.first { $0.key == "prefix" })
        shared.set(prefix, .string("C-a"))
        XCTAssertNil(shared.error)
        let padding = try XCTUnwrap(shared.snapshot?.settings.first { $0.key == "window-padding-x" })
        shared.set(padding, .number(18))
        XCTAssertNil(shared.error)
        let reloaded = ZZSharedSettings(directory: directory)
        XCTAssertEqual(reloaded.text("prefix"), "C-a")
        XCTAssertEqual(reloaded.number("window-padding-x"), 18)
        XCTAssertEqual(reloaded.mobileAppearance?.padding[1], 18)
        XCTAssertTrue((try String(contentsOf: directory.appending(path: "mux.conf"), encoding: .utf8)).contains("C-a"))
        reloaded.set(prefix, .null)
        XCTAssertEqual(reloaded.value("prefix"), prefix.default_value)
    }

    @MainActor
    func testFontControlsAndConfigEditorUseTheSamePreferences() {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        withDefaults { defaults in
            defaults.set("menlo", forKey: "zz.client.terminal.font")
            defaults.set(16, forKey: "zz.client.terminal.font-size")
            let settings = ZZClientSettings(defaults: defaults, configDirectory: directory)
            XCTAssertEqual(settings.terminalFont, .menlo)
            XCTAssertEqual(settings.terminalFontSize, 16)
            settings.terminalFont = .firaCode
            settings.terminalFontSize = 19
            XCTAssertEqual(settings.shared?.text("font-family"), "Fira Code")
            XCTAssertEqual(settings.shared?.number("font-size"), 19)
            XCTAssertEqual(settings.shared?.action("save-terminal", [
                "source": "font-family = Menlo\nfont-size = 17\n",
            ]), true)
            XCTAssertEqual(settings.terminalFont, .menlo)
            XCTAssertEqual(settings.terminalFontSize, 17)
            XCTAssertEqual(settings.terminalPresentation.pointSize, 17)
        }
    }

    @MainActor
    func testExplicitCursorPolicyOverridesMigratedPreference() {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        withDefaults { defaults in
            defaults.set(false, forKey: "zz.client.terminal.cursor-blinking")
            let settings = ZZClientSettings(defaults: defaults, configDirectory: directory)
            XCTAssertFalse(settings.cursorBlinking)
            XCTAssertEqual(settings.shared?.text("cursor-style-blink"), "off")
            XCTAssertEqual(settings.shared?.action("set-appearance", [
                "key": "cursor-style-blink", "value": "on",
            ]), true)
            XCTAssertTrue(settings.cursorBlinking)
            XCTAssertEqual(settings.shared?.action("set-appearance", [
                "key": "cursor-style-blink", "value": "terminal",
            ]), true)
            XCTAssertTrue(settings.cursorBlinking)
            settings.cursorBlinking = false
            XCTAssertEqual(settings.shared?.text("cursor-style-blink"), "off")
        }
    }

    @MainActor
    func testBundledThemeSelectionChangesLocalPaletteAndSurvivesReload() throws {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let shared = ZZSharedSettings(directory: directory)
        XCTAssertGreaterThan(shared.themes.count, 100)
        let theme = try XCTUnwrap(shared.themes.first { $0.name == "Dracula" })
        XCTAssertTrue(shared.action("set-appearance", ["key": "theme", "value": theme.name]))
        XCTAssertEqual(shared.text("theme"), theme.name)
        XCTAssertEqual(shared.mobileAppearance?.background, UInt32(theme.background.dropFirst(), radix: 16))
        let reloaded = ZZSharedSettings(directory: directory)
        XCTAssertEqual(reloaded.text("theme"), theme.name)
        XCTAssertEqual(reloaded.mobileAppearance, shared.mobileAppearance)
    }

    @MainActor
    private func withDefaults(_ body: (UserDefaults) -> Void) {
        let suite = "ClientSettingsTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defaults.removePersistentDomain(forName: suite)
        defer {
            defaults.removePersistentDomain(forName: suite)
        }
        body(defaults)
    }
}
