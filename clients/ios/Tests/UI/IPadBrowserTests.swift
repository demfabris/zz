import UIKit
import XCTest

@MainActor
final class IPadBrowserTests: XCTestCase {
    func testBrowserCreationNavigationTabsAndRetainedPage() throws {
        continueAfterFailure = false
        guard UIDevice.current.userInterfaceIdiom == .pad,
              let socket = ProcessInfo.processInfo.environment["ZZ_IOS_UI_TEST_SOCKET"],
              let url = ProcessInfo.processInfo.environment["ZZ_IOS_BROWSER_TEST_URL"] else {
            throw XCTSkip("Requires an isolated daemon and the browser HTML fixture server.")
        }
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = XCUIApplication()
        app.launchEnvironment["ZZ_SOCKET"] = socket
        app.launch()
        defer { app.terminate() }
        let closePanorama = app.buttons["ipad-panorama-close"]
        XCTAssertTrue(closePanorama.waitForExistence(timeout: 20))
        closePanorama.tap()
        if !app.buttons["ipad-new-pane-menu"].waitForExistence(timeout: 2) {
            app.navigationBars.buttons.matching(NSPredicate(format: "label CONTAINS[c] 'sidebar'")).firstMatch.tap()
        }
        app.buttons["ipad-new-pane-menu"].tap()
        app.buttons["Choose Pane Type"].tap()
        let choice = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "pane-kind-2-")).firstMatch
        XCTAssertTrue(choice.waitForExistence(timeout: 10))
        let pane = String(choice.identifier.dropFirst("pane-kind-2-".count))
        choice.tap()
        let address = app.textFields["browser-address-\(pane)"]
        XCTAssertTrue(address.waitForExistence(timeout: 10))
        app.buttons["pane-actions-\(pane)"].tap()
        app.buttons["Zoom Pane"].tap()
        address.tap()
        address.typeText(url + "\n")
        let heading = app.webViews.staticTexts["Browser pane connected"]
        XCTAssertTrue(heading.waitForExistence(timeout: 15))
        let draft = app.webViews.textFields["Draft"]
        XCTAssertTrue(draft.waitForExistence(timeout: 5))
        draft.tap()
        draft.typeText("unfinished work")
        app.webViews.buttons["Keep draft"].tap()
        capture("Native browser pane", app: app)

        app.buttons["ipad-panorama-toggle"].tap()
        XCTAssertTrue(closePanorama.waitForExistence(timeout: 5))
        capture("Browser in Panorama", app: app)
        closePanorama.tap()
        XCTAssertTrue(draft.waitForExistence(timeout: 10))
        XCTAssertEqual(draft.value as? String, "unfinished work")

        app.buttons["browser-new-tab-\(pane)"].tap()
        address.typeText(url + "?second\n")
        XCTAssertTrue(heading.waitForExistence(timeout: 15))
        let tabs = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "browser-tab-\(pane)-"))
        XCTAssertEqual(tabs.count, 2)
        tabs.element(boundBy: 0).tap()
        XCTAssertEqual(draft.value as? String, "unfinished work")
        app.webViews.links["Next page"].tap()
        let nextURL = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "value CONTAINS %@", "?next"), object: address
        )
        XCTAssertEqual(XCTWaiter.wait(for: [nextURL], timeout: 10), .completed)
        app.buttons["Back"].tap()
        XCTAssertTrue(heading.waitForExistence(timeout: 10))
        capture("Browser tabs and navigation", app: app)
    }

    private func capture(_ name: String, app: XCUIApplication) {
        let screenshot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        screenshot.name = name
        screenshot.lifetime = .keepAlways
        add(screenshot)
        let tree = XCTAttachment(string: app.debugDescription)
        tree.name = "\(name) accessibility"
        tree.lifetime = .keepAlways
        add(tree)
    }
}
