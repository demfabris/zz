import XCTest
import UIKit

@MainActor
final class IPadAcceptanceTests: XCTestCase {
    private var app: XCUIApplication!

    private func launchApp() throws {
        continueAfterFailure = false
        guard UIDevice.current.userInterfaceIdiom == .pad else {
            throw XCTSkip("This acceptance test exercises the regular iPad workspace.")
        }
        guard let socket = ProcessInfo.processInfo.environment["ZZ_IOS_UI_TEST_SOCKET"],
              !socket.isEmpty, !socket.contains("$(") else {
            throw XCTSkip("Set ZZ_IOS_UI_TEST_SOCKET to an isolated test daemon socket.")
        }
        XCUIDevice.shared.orientation = .landscapeLeft
        app = XCUIApplication()
        app.launchEnvironment["ZZ_SOCKET"] = socket
        app.launch()
    }

    private func finish() {
        if let app {
            capture("Final state")
            let tree = XCTAttachment(string: app.debugDescription)
            tree.name = "Accessibility tree"
            tree.lifetime = .keepAlways
            add(tree)
            app.terminate()
        }
    }

    func testSettingsPickerResizeAndCopyMode() throws {
        try launchApp()
        defer { finish() }
        let closePanorama = app.buttons["ipad-panorama-close"]
        XCTAssertTrue(closePanorama.waitForExistence(timeout: 20), "Connected workspace must appear")
        closePanorama.tap()
        XCTAssertTrue(app.buttons["ipad-new-pane-menu"].isHittable)
        capture("Sidebar footer actions")
        XCTAssertEqual(app.navigationBars.count, 0)
        let originalPaneID = app.staticTexts.matching(NSPredicate(format: "identifier MATCHES %@", "ipad-pane-[0-9]+")).firstMatch.identifier
        tap(app.buttons["ipad-new-pane-menu"])
        tap(app.buttons["ipad-new-window"])
        tap(app.staticTexts[originalPaneID])
        XCTAssertTrue(app.staticTexts[originalPaneID].isSelected)
        capture("Window creation and sidebar navigation without a titlebar")
        tap(app.buttons["ipad-panorama-toggle"])
        tap(closePanorama)
        XCTAssertTrue(app.buttons["ipad-new-pane-menu"].waitForExistence(timeout: 5))
        let firstTile = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "ipad-pane-tile-")).firstMatch
        XCTAssertTrue(firstTile.waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["ipad-sidebar-toggle"].exists)
        let footer = element("ipad-sidebar-footer")
        let terminal = app.textViews.firstMatch
        XCTAssertTrue(terminal.waitForExistence(timeout: 5))
        XCTAssertLessThan(footer.frame.maxX, terminal.frame.minX)
        XCTAssertGreaterThan(terminal.frame.maxY, app.frame.maxY - 24)
        tap(app.buttons["settings"])
        let settingsPanel = element("ipad-settings")
        XCTAssertTrue(settingsPanel.waitForExistence(timeout: 5))
        XCTAssertLessThan(settingsPanel.frame.width, app.frame.width)
        XCTAssertGreaterThan(settingsPanel.frame.minX, 0)
        XCTAssertFalse(element("settings-section-terminal").exists)
        XCTAssertTrue(element("settings-typeface").exists)
        capture("Three terminal preferences")
        tap(element("settings-typeface"))
        for font in ["System Mono", "Menlo", "Fira Code", "Geist Mono", "0xProto", "Courier New"] {
            XCTAssertTrue(app.buttons[font].exists || app.staticTexts[font].exists, "Missing font: \(font)")
        }
        capture("Bundled monospace fonts")
        tap(app.buttons["Fira Code"].exists ? app.buttons["Fira Code"] : app.staticTexts["Fira Code"])
        tap(element("settings-color-theme"))
        let themes = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "terminal-theme-"))
        XCTAssertGreaterThan(themes.count, 0, "Desktop terminal themes must load")
        let chosenTheme = themes.element(boundBy: 0)
        let themeIdentifier = chosenTheme.identifier
        chosenTheme.tap()
        XCTAssertTrue(app.buttons[themeIdentifier].isSelected)
        capture("Terminal theme selection")
        tap(app.navigationBars["Color Theme"].buttons.element(boundBy: 0))
        tap(app.buttons["settings-done"])

        let beforeIDs = Set(paneActions.allElementsBoundByIndex.map(\.identifier))
        let splitDown = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "pane-split-down-")).firstMatch
        XCTAssertTrue(splitDown.waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "pane-split-right-")).firstMatch.isHittable)
        tap(splitDown)
        let terminalChoice = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "pane-kind-1-")).firstMatch
        XCTAssertTrue(terminalChoice.waitForExistence(timeout: 10))
        let paneID = String(terminalChoice.identifier.dropFirst("pane-kind-1-".count))
        capture("Pane type picker")
        terminalChoice.tap()
        let actions = app.buttons["pane-actions-\(paneID)"]
        XCTAssertTrue(actions.waitForExistence(timeout: 10))
        XCTAssertFalse(beforeIDs.contains(actions.identifier))
        XCTAssertTrue(waitUntil { !terminalChoice.exists })
        capture("Picker materialized as terminal")

        let tile = element("ipad-pane-tile-\(paneID)")
        XCTAssertTrue(tile.waitForExistence(timeout: 5))
        tap(actions)
        tap(app.buttons["Resize Pane"])
        let initialFrame = tile.frame
        tap(app.buttons["pane-resize-up"])
        XCTAssertTrue(waitUntil {
            abs(tile.frame.height - initialFrame.height) > 0.5
        }, "Resizing must change the daemon-backed pane geometry")
        capture("Pane resize controls")
        tap(app.navigationBars["Resize Pane"].buttons["Done"])

        tap(actions)
        tap(app.buttons["Copy Mode"])
        let copyBar = element("copy-mode-bar-\(paneID)")
        XCTAssertTrue(copyBar.waitForExistence(timeout: 10))
        capture("Copy mode controls")
        tap(app.buttons["copy-mode-find-\(paneID)"])
        let search = app.textFields["Search text"]
        XCTAssertTrue(search.waitForExistence(timeout: 5))
        search.tap()
        search.typeText("zz")
        capture("Terminal search editor")
        tap(app.buttons["terminal-search-submit"])
        XCTAssertTrue(copyBar.waitForExistence(timeout: 5))
        capture("Terminal search results")
        let done = app.buttons["copy-mode-done-\(paneID)"]
        if !done.isHittable { copyBar.swipeLeft() }
        tap(done)
        XCTAssertTrue(waitUntil { !copyBar.exists })

        tap(actions)
        tap(app.buttons["Keyboard Bindings"])
        XCTAssertTrue(app.navigationBars["Keyboard Bindings"].waitForExistence(timeout: 5))
        let tableHeader = app.staticTexts.matching(NSPredicate(format: "label IN %@", ["root", "prefix", "copy-mode", "copy-mode-vi", "choose-tree", "choose-buffer"])).firstMatch
        XCTAssertTrue(tableHeader.waitForExistence(timeout: 5))
        capture("Live daemon keyboard bindings")
        tap(app.navigationBars["Keyboard Bindings"].buttons["Done"])

    }

    func testPaneHeaderCloseAndPortraitSettings() throws {
        try launchApp()
        defer { finish() }
        tap(app.buttons["ipad-panorama-close"])
        let originalPaneID = app.staticTexts.matching(NSPredicate(format: "identifier MATCHES %@", "ipad-pane-[0-9]+")).firstMatch.identifier
        let splitRight = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "pane-split-right-")).firstMatch
        tap(splitRight)
        let terminalChoice = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "pane-kind-1-")).firstMatch
        XCTAssertTrue(terminalChoice.waitForExistence(timeout: 10))
        let paneID = String(terminalChoice.identifier.dropFirst("pane-kind-1-".count))
        tap(terminalChoice)
        let tile = element("ipad-pane-tile-\(paneID)")
        XCTAssertTrue(tile.waitForExistence(timeout: 5))
        capture("Pane headers and full-height workspace")
        tap(app.staticTexts[originalPaneID])
        XCTAssertTrue(app.staticTexts[originalPaneID].isSelected)
        tap(app.staticTexts["ipad-pane-\(paneID)"])
        XCTAssertTrue(app.staticTexts["ipad-pane-\(paneID)"].isSelected)
        capture("Native sidebar pane selection")
        tap(app.buttons["pane-close-\(paneID)"])
        tap(app.alerts.buttons["Cancel"])
        XCTAssertTrue(tile.exists)
        tap(app.buttons["pane-close-\(paneID)"])
        tap(app.alerts.buttons["Close Pane"])
        XCTAssertTrue(waitUntil { !tile.exists })
        XCTAssertFalse(app.buttons["ipad-new-session"].exists)
        capture("Expanded sidebar and split panes")
        XCUIDevice.shared.orientation = .portrait
        XCTAssertTrue(app.buttons["ipad-new-pane-menu"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.staticTexts[originalPaneID].isHittable)
        XCTAssertFalse(app.buttons["Show Sidebar"].exists)
        tap(app.buttons["settings"])
        XCTAssertTrue(element("ipad-settings").waitForExistence(timeout: 5))
        capture("Portrait settings sheet")
        tap(app.buttons["settings-done"])
        XCUIDevice.shared.orientation = .landscapeLeft
    }

    func testSimplifiedSettingsAndHelp() throws {
        try launchApp()
        defer { finish() }
        tap(app.buttons["ipad-panorama-close"])
        XCTAssertEqual(app.navigationBars.count, 0)
        tap(app.buttons["settings"])
        for title in ["Animations", "Panes", "Status Bar", "Multiplexer", "Background opacity", "Cursor style", "Edit terminal config", "Restore Defaults"] {
            XCTAssertFalse(app.staticTexts[title].exists)
        }
        XCTAssertTrue(element("settings-typeface").exists)
        XCTAssertTrue(element("settings-color-theme").exists)
        XCTAssertEqual(app.steppers.count, 1)
        XCTAssertEqual(app.switches.count, 0)
        XCTAssertEqual(app.sliders.count, 0)
        let fontSize = app.steppers.firstMatch
        let previousSize = fontSize.value as? String
        tap(fontSize.buttons["Increment"])
        XCTAssertNotEqual(fontSize.value as? String, previousSize)
        for mode in ["light", "system", "dark"] {
            let choice = app.buttons["settings-appearance-\(mode)"]
            tap(choice)
            XCTAssertTrue(choice.isSelected)
        }
        capture("Dark appearance settings")
        tap(app.buttons["settings-done"])
        capture("Dark workspace")
        tap(app.buttons["settings"])
        XCTAssertTrue(app.buttons["settings-appearance-dark"].isSelected)
        tap(app.buttons["settings-done"])
        tap(app.buttons["ipad-overflow-menu"])
        tap(app.buttons["Help"])
        tap(app.buttons["Keyboard Bindings"])
        XCTAssertTrue(app.navigationBars["Keyboard Bindings"].waitForExistence(timeout: 5))
        capture("Keyboard reference under Help")
        tap(app.navigationBars["Keyboard Bindings"].buttons["Done"])
    }

    private var paneActions: XCUIElementQuery {
        app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "pane-actions-"))
    }

    private func element(_ identifier: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: identifier).firstMatch
    }

    private func tap(_ element: XCUIElement, file: StaticString = #filePath, line: UInt = #line) {
        let exists = element.waitForExistence(timeout: 5)
        if !exists { print(app.debugDescription) }
        XCTAssertTrue(exists, "Missing control: \(element)", file: file, line: line)
        let hittable = waitUntil { element.isHittable }
        if !hittable {
            print(app.debugDescription)
            capture("Control not hittable")
            let tree = XCTAttachment(string: app.debugDescription)
            tree.name = "Control accessibility tree"
            tree.lifetime = .keepAlways
            add(tree)
        }
        XCTAssertTrue(hittable, "Control is not hittable: \(element)", file: file, line: line)
        element.tap()
    }

    private func waitUntil(_ condition: @escaping () -> Bool) -> Bool {
        let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)
        return XCTWaiter.wait(for: [expectation], timeout: 5) == .completed
    }

    private func capture(_ name: String) {
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
