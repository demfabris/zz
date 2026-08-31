import XCTest
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
