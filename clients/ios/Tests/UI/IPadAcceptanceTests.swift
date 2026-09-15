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
        XCTAssertTrue(element("ipad-control-pill").waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["ipad-new-pane-menu"].isHittable)
        capture("Sidebar with floating controls")
        tap(app.buttons["ipad-sidebar-toggle"])
        XCTAssertTrue(app.buttons["ipad-new-window"].waitForExistence(timeout: 5))
        XCTAssertEqual(app.navigationBars.count, 0)
        let originalWindow = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "ipad-window-")).firstMatch
        let originalWindowID = originalWindow.identifier
        tap(app.buttons["ipad-new-window"])
        XCTAssertTrue(waitUntil {
            self.app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "ipad-window-")).count > 1
        })
        tap(app.buttons[originalWindowID])
        capture("Collapsed sidebar with window pill")
        tap(app.buttons["ipad-panorama-toggle"])
        tap(closePanorama)
        XCTAssertTrue(app.buttons["ipad-new-pane-menu"].waitForExistence(timeout: 5))
        let firstTile = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "ipad-pane-tile-")).firstMatch
        XCTAssertTrue(firstTile.waitForExistence(timeout: 5))
        let tileFrame = firstTile.frame
        tap(app.buttons["settings"])
        let settingsPanel = element("ipad-settings-sidebar")
        XCTAssertTrue(settingsPanel.waitForExistence(timeout: 5))
        XCTAssertLessThan(settingsPanel.frame.width, app.frame.width / 2)
        XCTAssertGreaterThan(settingsPanel.frame.minX, app.frame.width / 2)
        XCTAssertEqual(firstTile.frame, tileFrame, "Opening Settings must preserve pane geometry")
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
        tap(element("settings-section-mux"))
        XCTAssertTrue(element("settings-section-mux").isSelected)
        capture("Multiplexer preferences")
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
        tap(app.buttons["ipad-sidebar-toggle"])
        let splitRight = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "pane-split-right-")).firstMatch
        tap(splitRight)
        let terminalChoice = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "pane-kind-1-")).firstMatch
        XCTAssertTrue(terminalChoice.waitForExistence(timeout: 10))
        let paneID = String(terminalChoice.identifier.dropFirst("pane-kind-1-".count))
        tap(terminalChoice)
        let tile = element("ipad-pane-tile-\(paneID)")
        XCTAssertTrue(tile.waitForExistence(timeout: 5))
        capture("Flat pane headers and floating window controls")
        tap(app.buttons["pane-close-\(paneID)"])
        tap(app.alerts.buttons["Cancel"])
        XCTAssertTrue(tile.exists)
        tap(app.buttons["pane-close-\(paneID)"])
        tap(app.alerts.buttons["Close Pane"])
        XCTAssertTrue(waitUntil { !tile.exists })
        tap(app.buttons["ipad-sidebar-toggle"])
        XCTAssertFalse(app.buttons["ipad-new-session"].exists)
        XCTAssertTrue(app.staticTexts["Sessions"].waitForExistence(timeout: 5))
        capture("Expanded sidebar and split panes")
        XCUIDevice.shared.orientation = .portrait
        XCTAssertTrue(app.buttons["ipad-new-pane-menu"].waitForExistence(timeout: 5))
        tap(app.buttons["settings"])
        XCTAssertTrue(element("ipad-settings-sidebar").waitForExistence(timeout: 5))
        capture("Portrait settings sidebar")
        tap(app.buttons["settings-done"])
        XCUIDevice.shared.orientation = .landscapeLeft
    }

    func testAnnotatedSettingsAndPaneAppearance() throws {
        try launchApp()
        defer { finish() }
        tap(app.buttons["ipad-panorama-close"])
        XCTAssertFalse(app.buttons["ipad-new-session"].exists)
        tap(app.buttons["settings"])
        for title in ["Widget corner radius", "Contrast", "Interface font", "Chrome preset dark", "Shadow strength"] {
            XCTAssertFalse(app.staticTexts[title].exists)
        }
        capture("Simplified appearance and pill selectors")
        tap(element("settings-section-terminal"))
        tap(element("settings-color-theme"))
        let themes = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "terminal-theme-"))
        tap(themes.firstMatch)
        tap(element("settings-section-panes"))
        let gaps = app.switches["Pane gaps"]
        if gaps.value as? String == "1" { tap(gaps.switches.firstMatch) }
        XCTAssertEqual(gaps.value as? String, "0")
        setSlider("pane-background-opacity", position: 1)
        setSlider("pane-glow-strength", position: 0)
        XCTAssertFalse(app.staticTexts["Pane border width"].exists)
        tap(element("settings-section-terminal"))
        setSlider("background-opacity", position: 0.5)
        let opacityField = app.textFields["setting-value-background-opacity"]
        let previousOpacity = try XCTUnwrap(Int(opacityField.value as? String ?? ""))
        tap(app.buttons["setting-stepper-background-opacity-Increment"])
        XCTAssertEqual(opacityField.value as? String, String(previousOpacity + 1))
        setSlider("window-padding-x", position: 1)
        setSlider("window-padding-y", position: 1)
        XCTAssertEqual(app.textFields["setting-value-window-padding-x"].value as? String, "16")
        XCTAssertEqual(app.textFields["setting-value-window-padding-y"].value as? String, "16")
        capture("Precise opacity and bounded padding")
        tap(app.buttons["settings-done"])
        let tile = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "ipad-pane-tile-")).firstMatch
        XCTAssertTrue(tile.waitForExistence(timeout: 5))
        capture("Matching header padding and terminal background")
        tap(app.buttons["settings"])
        tap(element("settings-section-panes"))
        setSlider("pane-glow-strength", position: 0.5)
        XCTAssertGreaterThan(Int(app.textFields["setting-value-pane-glow-strength"].value as? String ?? "") ?? 0, 0)
        tap(app.buttons["settings-done"])
        capture("Selected pane glow without gaps")
        tap(app.buttons["settings"])
        tap(element("settings-section-panes"))
        tap(app.switches["Pane gaps"].switches.firstMatch)
        XCTAssertEqual(app.switches["Pane gaps"].value as? String, "1")
        let radius = element("setting-pane-corner-radius")
        scrollSettings(to: radius)
        XCTAssertEqual(radius.buttons.count, 3)
        for index in 0..<3 {
            let choice = radius.buttons.element(boundBy: index)
            tap(choice)
            XCTAssertTrue(choice.isSelected)
        }
        capture("Three pane corner sizes")
        tap(app.buttons["settings-done"])
    }

    private func setSlider(_ key: String, position: CGFloat) {
        let slider = app.sliders["setting-slider-\(key)"]
        scrollSettings(to: slider)
        slider.adjust(toNormalizedSliderPosition: position)
    }

    private func scrollSettings(to control: XCUIElement) {
        for _ in 0..<8 {
            if control.exists && control.isHittable { return }
            element("ipad-settings-sidebar").swipeUp()
        }
        XCTAssertTrue(control.isHittable)
    }

    private var paneActions: XCUIElementQuery {
        app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "pane-actions-"))
    }

    private func element(_ identifier: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: identifier).firstMatch
    }

    private func tap(_ element: XCUIElement, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertTrue(element.waitForExistence(timeout: 5), "Missing control: \(element)", file: file, line: line)
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
