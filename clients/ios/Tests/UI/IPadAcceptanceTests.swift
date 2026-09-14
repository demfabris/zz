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
        showDetailToolbar()
        capture("Connected iPad workspace")

        tap(app.buttons["ipad-overflow-menu"])
        tap(app.buttons["settings"])
        for section in ["appearance", "terminal", "panes", "status", "mux"] {
            XCTAssertTrue(element("settings-section-\(section)").waitForExistence(timeout: 5))
        }
        capture("Settings sections")
        tap(element("settings-section-terminal"))
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
        if !element("settings-section-mux").exists {
            tap(app.navigationBars["Color Theme"].buttons["Terminal"])
            tap(app.navigationBars["Terminal"].buttons["Settings"])
        }
        tap(element("settings-section-mux"))
        XCTAssertTrue(app.navigationBars["Multiplexer"].waitForExistence(timeout: 5))
        capture("Multiplexer preferences")
        if !app.buttons["settings-done"].exists {
            tap(app.navigationBars["Multiplexer"].buttons["Settings"])
        }
        tap(app.buttons["settings-done"])

        let beforeIDs = Set(paneActions.allElementsBoundByIndex.map(\.identifier))
        tap(app.buttons["ipad-new-pane-menu"])
        tap(app.buttons["Choose Pane Type"])
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

    private var paneActions: XCUIElementQuery {
        app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "pane-actions-"))
    }

    private func element(_ identifier: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: identifier).firstMatch
    }

    private func showDetailToolbar() {
        if app.buttons["ipad-overflow-menu"].waitForExistence(timeout: 2) { return }
        let sidebar = app.navigationBars.buttons.matching(NSPredicate(format: "label CONTAINS[c] 'sidebar'")).firstMatch
        XCTAssertTrue(sidebar.waitForExistence(timeout: 5), "Sidebar visibility control must be accessible")
        sidebar.tap()
        XCTAssertTrue(app.buttons["ipad-overflow-menu"].waitForExistence(timeout: 5))
    }

    private func tap(_ element: XCUIElement, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertTrue(element.waitForExistence(timeout: 5), "Missing control: \(element)", file: file, line: line)
        let hittable = waitUntil { element.isHittable }
        if !hittable {
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
