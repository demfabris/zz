import XCTest
@testable import ZZ

final class TerminalInteractionTests: XCTestCase {
    func testHostEndpointAcceptsShortAndExplicitSSHAddresses() {
        XCTAssertEqual(ZZHostEndpoint.normalized("fab@mini"), "ssh://fab@mini")
        XCTAssertEqual(
            ZZHostEndpoint.normalized("  ssh://fab@mini:2222  "),
            "ssh://fab@mini:2222"
        )
        XCTAssertNil(ZZHostEndpoint.normalized("mini"))
        XCTAssertNil(ZZHostEndpoint.normalized("https://fab@mini"))
        XCTAssertNil(ZZHostEndpoint.normalized("   "))
    }

    func testLiveBoundsDriveGridAndRestoreExactly() throws {
        let cell = CGSize(width: 10, height: 17)
        let full = try XCTUnwrap(TerminalLayout(bounds: CGSize(width: 390, height: 714), cell: cell))
        let keyboard = try XCTUnwrap(TerminalLayout(bounds: CGSize(width: 390, height: 374), cell: cell))
        let restored = try XCTUnwrap(TerminalLayout(bounds: CGSize(width: 390, height: 714), cell: cell))

        XCTAssertEqual(full.columns, 39)
        XCTAssertEqual(full.rows, 42)
        XCTAssertEqual(keyboard.columns, 39)
        XCTAssertEqual(keyboard.rows, 22)
        XCTAssertEqual(restored, full)
    }

    func testReconnectUsesLastKeyboardHiddenGrid() {
        let cell = CGSize(width: 9, height: 18)
        let full = TerminalLayout(columns: 43, rows: 39, cell: cell)
        let keyboard = TerminalLayout(columns: 43, rows: 20, cell: cell)
        var geometry = TerminalGeometryState()

        XCTAssertTrue(geometry.observe(full, stable: true))
        geometry.markSent(full)
        XCTAssertTrue(geometry.observe(keyboard, stable: false))
        geometry.markSent(keyboard)
        XCTAssertEqual(geometry.lastStable, full)
        XCTAssertEqual(geometry.stableLayoutToRestore, full)

        geometry.invalidateSent()
        XCTAssertEqual(geometry.reconnectLayout, full)
    }

    func testStableObservationUpdatesWithoutDuplicateResize() {
        let layout = TerminalLayout(columns: 40, rows: 30, cell: CGSize(width: 8, height: 16))
        var geometry = TerminalGeometryState()

        XCTAssertTrue(geometry.observe(layout, stable: false))
        geometry.markSent(layout)
        XCTAssertFalse(geometry.observe(layout, stable: true))
        XCTAssertEqual(geometry.lastStable, layout)
    }

    func testInputOwnershipIsExclusiveAndActivationIsMonotonic() {
        var input = TerminalInputState()

        input.acquire(11)
        XCTAssertEqual(input.owner, .pane(11))
        XCTAssertEqual(input.activation, 1)

        input.acquire(22)
        XCTAssertEqual(input.owner, .pane(22))
        XCTAssertEqual(input.activation, 2)

        input.release()
        XCTAssertEqual(input.owner, .none)
        XCTAssertEqual(input.activation, 2)
    }

    func testZoomIsClampedAndReportsEveryCrossedStep() {
        XCTAssertEqual(TerminalFontZoom.pointSize(for: -100), 9)
        XCTAssertEqual(TerminalFontZoom.pointSize(for: 100), 23)
        XCTAssertEqual(TerminalFontZoom.targetStep(anchor: 0, scale: 16.0 / 13.0), 3)
        XCTAssertEqual(TerminalFontZoom.pointSize(for: 3, basePointSize: 15), 18)
        XCTAssertEqual(TerminalFontZoom.pointSize(for: -100, basePointSize: 18), 9)
        XCTAssertEqual(TerminalFontZoom.targetStep(anchor: 0, scale: 18.0 / 15.0, basePointSize: 15), 3)
        XCTAssertEqual(TerminalFontZoom.crossedSteps(from: 0, to: 3), [1, 2, 3])
        XCTAssertEqual(TerminalFontZoom.crossedSteps(from: 3, to: 0), [2, 1, 0])
        XCTAssertEqual(TerminalFontZoom.crossedSteps(from: 3, to: 3), [])
    }

    func testCursorBlinkPreferenceDoesNotDisableBlinkingText() {
        XCTAssertFalse(
            TerminalBlinkPolicy.cursorShouldAnimate(
                frameRequestsBlink: true,
                cursorBlinking: false
            )
        )
        XCTAssertTrue(
            TerminalBlinkPolicy.shouldRunTimer(
                interactive: true,
                cursorRequestsBlink: true,
                blinkingText: true,
                cursorBlinking: false
            )
        )
        XCTAssertFalse(
            TerminalBlinkPolicy.shouldRunTimer(
                interactive: true,
                cursorRequestsBlink: true,
                blinkingText: false,
                cursorBlinking: false
            )
        )
        XCTAssertFalse(
            TerminalBlinkPolicy.shouldRunTimer(
                interactive: true,
                cursorRequestsBlink: false,
                blinkingText: false,
                cursorBlinking: true
            )
        )
    }

    func testAgentComposerActionTracksDaemonPhaseAndDraft() {
        XCTAssertEqual(
            ZZAgentComposerAction.resolve(
                phase: .ready,
                hasPrompt: true,
                queuedPrompts: 0
            ),
            .send
        )
        XCTAssertEqual(
            ZZAgentComposerAction.resolve(
                phase: .running,
                hasPrompt: true,
                queuedPrompts: 3
            ),
            .queue
        )
        XCTAssertEqual(
            ZZAgentComposerAction.resolve(
                phase: .awaitingPermission,
                hasPrompt: false,
                queuedPrompts: 4
            ),
            .stop
        )
        XCTAssertEqual(
            ZZAgentComposerAction.resolve(
                phase: .running,
                hasPrompt: true,
                queuedPrompts: 4
            ),
            .unavailable
        )
        XCTAssertEqual(
            ZZAgentComposerAction.resolve(
                phase: .failed,
                hasPrompt: true,
                queuedPrompts: 0
            ),
            .unavailable
        )
    }

    func testAgentDraftsStayIndependentAcrossPanes() {
        var drafts = ZZAgentDrafts()
        drafts.save("first", for: 11)
        drafts.save("second", for: 22)

        XCTAssertEqual(drafts.text(for: 11), "first")
        XCTAssertEqual(drafts.text(for: 22), "second")

        drafts.remove(pane: 11)
        XCTAssertEqual(drafts.text(for: 11), "")
        XCTAssertEqual(drafts.text(for: 22), "second")
    }

    func testAgentPromptCommandSeparatesOptionsAndPreservesText() {
        XCTAssertEqual(
            ZZAgentPromptCommand.arguments(pane: 42, text: "- fix the tests"),
            ["-t", "%42", "--submit", "--", "- fix the tests"]
        )
        XCTAssertEqual(
            ZZAgentPromptCommand.arguments(pane: 9, text: "  keep this spacing\n"),
            ["-t", "%9", "--submit", "--", "  keep this spacing\n"]
        )
        XCTAssertNil(ZZAgentPromptCommand.arguments(pane: 1, text: " \n\t"))
    }

    func testReconnectBackoffCapsAtSixteenSeconds() {
        XCTAssertEqual(
            (1...7).map { ZZReconnectPolicy.delay(for: $0) },
            [1, 2, 4, 8, 16, 16, 16]
        )
    }

    func testModifierTapIsOneShotAndDoubleTapLocks() {
        let control: UInt8 = 1 << 1
        var state = TerminalModifierLatchState()

        state.tap(control, at: 1)
        XCTAssertTrue(state.contains(control))
        XCTAssertFalse(state.isLocked(control))
        state.consumeOneShot()
        XCTAssertFalse(state.contains(control))

        state.tap(control, at: 2)
        state.tap(control, at: 2.2)
        XCTAssertTrue(state.isLocked(control))
        state.consumeOneShot()
        XCTAssertTrue(state.contains(control))
        state.tap(control, at: 3)
        XCTAssertFalse(state.contains(control))
    }

    func testDeepLinksAcceptOnlyKnownRoutes() throws {
        let paneURL = try XCTUnwrap(URL(string: "zz://pane?session=7&pane=11"))
        XCTAssertEqual(
            ZZNavigationTarget(url: paneURL),
            ZZNavigationTarget(session: 7, pane: 11)
        )

        let attentionURL = try XCTUnwrap(URL(string: "zz://attention"))
        XCTAssertEqual(ZZNavigationTarget(url: attentionURL)?.attention, true)

        let unknownURL = try XCTUnwrap(URL(string: "zz://delete?session=7&pane=11"))
        XCTAssertNil(ZZNavigationTarget(url: unknownURL))
    }
}
