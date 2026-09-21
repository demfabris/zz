import XCTest
import UIKit
@testable import ZZ

final class ClientSettingsTests: XCTestCase {
    @MainActor
    func testDefaults() {
        withDefaults { defaults in
            let settings = ZZClientSettings(defaults: defaults)

            XCTAssertEqual(settings.appearance, .system)
            XCTAssertEqual(settings.terminalFont, .systemMono)
            XCTAssertEqual(settings.terminalFontSize, 13)
            XCTAssertEqual(settings.terminalPresentation, .default)
        }
    }

    @MainActor
    func testPersistsEverySetting() {
        withDefaults { defaults in
            let settings = ZZClientSettings(defaults: defaults)
            settings.appearance = .dark
            settings.terminalFont = .menlo
            settings.terminalFontSize = 19

            let reloaded = ZZClientSettings(defaults: defaults)
            XCTAssertEqual(reloaded.appearance, .dark)
            XCTAssertEqual(reloaded.terminalFont, .menlo)
            XCTAssertEqual(reloaded.terminalFontSize, 19)
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
            XCTAssertEqual(settings.appearance, .system)
            XCTAssertEqual(settings.terminalFont, .systemMono)
            XCTAssertEqual(settings.terminalFontSize, 23)

            settings.terminalFontSize = -20
            XCTAssertEqual(settings.terminalFontSize, 9)
            XCTAssertEqual(ZZClientSettings(defaults: defaults).terminalFontSize, 9)

            defaults.set("giant", forKey: "zz.client.terminal.font-size")
            XCTAssertEqual(ZZClientSettings(defaults: defaults).terminalFontSize, 13)
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
    func testLegacyPreferencesKeepOnlyFontSizeAndThemeWithoutChangingMuxFile() throws {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let config = directory.appending(path: "config")
        let mux = directory.appending(path: "mux.conf")
        let muxSource = "set -g prefix C-a\nbind x split-window\n"
        try muxSource.write(to: mux, atomically: true, encoding: .utf8)
        try """
        font-family = Fira Code
        font-size = 19
        theme = Dracula
        background = #ff0000
        foreground = #00ff00
        palette = 0=#ff0000
        cursor-style = bar
        cursor-style-blink = off
        background-opacity = 0.25
        window-padding-x = 64
        pane-background-opacity = 0.2
        pane-glow-strength = 3
        """.write(to: config, atomically: true, encoding: .utf8)
        let shared = ZZSharedSettings(directory: directory)
        XCTAssertTrue(shared.terminalPreferencesReady)
        XCTAssertNil(shared.error)
        XCTAssertEqual(shared.text("font-family"), "Fira Code")
        XCTAssertEqual(shared.number("font-size"), 19)
        XCTAssertEqual(shared.text("theme"), "Dracula")
        XCTAssertFalse(shared.snapshot?.terminal_source?.contains("cursor-style") ?? true)
        XCTAssertFalse(shared.snapshot?.terminal_source?.contains("palette") ?? true)
        let theme = try XCTUnwrap(shared.themes.first { $0.name == "Dracula" })
        XCTAssertEqual(shared.mobileAppearance?.background, UInt32(theme.background.dropFirst(), radix: 16))
        XCTAssertEqual(try String(contentsOf: mux, encoding: .utf8), muxSource)
        withDefaults { defaults in
            defaults.set(false, forKey: "zz.client.terminal.cursor-blinking")
            defaults.set(true, forKey: "zz.client.ipad.extend-panes-under-home-indicator")
            defaults.set("dark", forKey: "zz.client.appearance")
            let settings = ZZClientSettings(defaults: defaults, configDirectory: directory)
            XCTAssertEqual(settings.terminalFont, .firaCode)
            XCTAssertEqual(settings.terminalFontSize, 19)
            XCTAssertEqual(settings.terminalPresentation.backgroundOpacity, 1)
            XCTAssertEqual(settings.terminalPresentation.paddingX, 8)
            XCTAssertEqual(settings.terminalPresentation.paddingY, 8)
            XCTAssertTrue(settings.terminalPresentation.cursorBlinking)
            XCTAssertNil(settings.terminalPresentation.cursorStyle)
        }
        let migrated = try String(contentsOf: config, encoding: .utf8)
        _ = ZZSharedSettings(directory: directory)
        XCTAssertEqual(try String(contentsOf: config, encoding: .utf8), migrated)
    }

    @MainActor
    func testFontControlsPersistThroughSharedSettings() {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        withDefaults { defaults in
            let settings = ZZClientSettings(defaults: defaults, configDirectory: directory)
            settings.terminalFont = .firaCode
            settings.terminalFontSize = 19
            let reloaded = ZZClientSettings(defaults: defaults, configDirectory: directory)
            XCTAssertEqual(reloaded.terminalFont, .firaCode)
            XCTAssertEqual(reloaded.terminalFontSize, 19)
            XCTAssertEqual(reloaded.terminalPresentation.pointSize, 19)
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
