import UIKit
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

    func testSnapshotActivePaneTransfersExistingInputSelection() {
        var input = TerminalInputState()
        input.acquire(11)

        XCTAssertEqual(
            input.snapshotTarget(selectedPane: 11, activePane: 22, navigationPending: false),
            22
        )
        XCTAssertNil(
            input.snapshotTarget(selectedPane: 11, activePane: 11, navigationPending: false)
        )
        XCTAssertNil(
            input.snapshotTarget(selectedPane: nil, activePane: 22, navigationPending: false)
        )
        XCTAssertNil(
            input.snapshotTarget(selectedPane: 11, activePane: 22, navigationPending: true)
        )

        input.release()
        XCTAssertEqual(
            input.snapshotTarget(
                selectedPane: 11,
                activePane: 22,
                replacingPane: 11,
                navigationPending: false
            ),
            22
        )
        XCTAssertNil(
            input.snapshotTarget(selectedPane: 11, activePane: 22, navigationPending: false)
        )
        XCTAssertNil(
            input.snapshotTarget(
                selectedPane: 11,
                activePane: 22,
                replacingPane: 33,
                navigationPending: false
            )
        )
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

    func testOnlyActiveCursorBlinksWithoutDisablingBlinkingText() {
        XCTAssertFalse(
            TerminalBlinkPolicy.cursorShouldAnimate(
                cursorActive: true,
                frameRequestsBlink: true,
                cursorBlinking: false
            )
        )
        XCTAssertFalse(
            TerminalBlinkPolicy.cursorShouldAnimate(
                cursorActive: false,
                frameRequestsBlink: true,
                cursorBlinking: true
            )
        )
        XCTAssertTrue(
            TerminalBlinkPolicy.shouldRunTimer(
                interactive: true,
                cursorActive: false,
                cursorRequestsBlink: true,
                blinkingText: true,
                cursorBlinking: false
            )
        )
        XCTAssertTrue(
            TerminalBlinkPolicy.shouldRunTimer(
                interactive: true,
                cursorActive: true,
                cursorRequestsBlink: true,
                blinkingText: false,
                cursorBlinking: true
            )
        )
        XCTAssertFalse(
            TerminalBlinkPolicy.shouldRunTimer(
                interactive: true,
                cursorActive: false,
                cursorRequestsBlink: true,
                blinkingText: false,
                cursorBlinking: true
            )
        )
        XCTAssertFalse(
            TerminalBlinkPolicy.shouldRunTimer(
                interactive: true,
                cursorActive: true,
                cursorRequestsBlink: false,
                blinkingText: false,
                cursorBlinking: true
            )
        )
    }

    func testBlinkingRowsFindOnlyRowsUsingBlinkingStyles() {
        let blink: UInt16 = 1 << 3
        // 2 columns x 3 rows, row-major style indices.
        let cells = [0, 1, 0, 0, 1, 0]
        XCTAssertEqual(
            TerminalBlinkPolicy.blinkingRows(
                cellStyleIndices: cells,
                columns: 2,
                rowCount: 3,
                styleAttributes: [0, blink]
            ),
            [0, 2]
        )
        // Unused blinking entry: no rows.
        XCTAssertEqual(
            TerminalBlinkPolicy.blinkingRows(
                cellStyleIndices: [0, 0, 0, 0, 0, 0],
                columns: 2,
                rowCount: 3,
                styleAttributes: [0, blink]
            ),
            []
        )
        XCTAssertEqual(
            TerminalBlinkPolicy.blinkingRows(
                cellStyleIndices: cells,
                columns: 0,
                rowCount: 3,
                styleAttributes: [0, blink]
            ),
            []
        )
    }

    func testBlinkDirtyRectInvalidatesCursorCellOnly() {
        let cell = CGSize(width: 8, height: 16)
        XCTAssertEqual(
            TerminalBlinkPolicy.blinkDirtyRects(
                cursorColumn: 3,
                cursorRow: 2,
                cursorAnimates: true,
                blinkingRows: [],
                columns: 10,
                rowCount: 10,
                cellSize: cell,
                boundsWidth: 80
            ),
            [CGRect(x: 24, y: 32, width: 8, height: 16)]
        )
        // Idle cursor does not animate: nothing to redraw.
        XCTAssertEqual(
            TerminalBlinkPolicy.blinkDirtyRects(
                cursorColumn: 3,
                cursorRow: 2,
                cursorAnimates: false,
                blinkingRows: [],
                columns: 10,
                rowCount: 10,
                cellSize: cell,
                boundsWidth: 80
            ),
            []
        )
    }

    func testBlinkDirtyRectMergesBlinkingRowsIntoBands() {
        let cell = CGSize(width: 8, height: 16)
        // Rows 1-2 merge into one band; row 4 is its own band. The cursor on
        // row 1 is already covered, so it adds no extra rect.
        XCTAssertEqual(
            TerminalBlinkPolicy.blinkDirtyRects(
                cursorColumn: 5,
                cursorRow: 1,
                cursorAnimates: true,
                blinkingRows: [4, 2, 1, 1],
                columns: 10,
                rowCount: 10,
                cellSize: cell,
                boundsWidth: 80
            ),
            [
                CGRect(x: 0, y: 16, width: 80, height: 32),
                CGRect(x: 0, y: 64, width: 80, height: 16),
            ]
        )
        // Cursor outside the blinking bands gets its own cell rect.
        XCTAssertEqual(
            TerminalBlinkPolicy.blinkDirtyRects(
                cursorColumn: 0,
                cursorRow: 7,
                cursorAnimates: true,
                blinkingRows: [1],
                columns: 10,
                rowCount: 10,
                cellSize: cell,
                boundsWidth: 80
            ),
            [
                CGRect(x: 0, y: 16, width: 80, height: 16),
                CGRect(x: 0, y: 112, width: 8, height: 16),
            ]
        )
        // Out-of-range rows and cursor are dropped.
        XCTAssertEqual(
            TerminalBlinkPolicy.blinkDirtyRects(
                cursorColumn: 99,
                cursorRow: -1,
                cursorAnimates: true,
                blinkingRows: [-1, 3, 99],
                columns: 10,
                rowCount: 10,
                cellSize: cell,
                boundsWidth: 80
            ),
            [CGRect(x: 0, y: 48, width: 80, height: 16)]
        )
        XCTAssertEqual(
            TerminalBlinkPolicy.blinkDirtyRects(
                cursorColumn: 0,
                cursorRow: 0,
                cursorAnimates: true,
                blinkingRows: [],
                columns: 10,
                rowCount: 10,
                cellSize: .zero,
                boundsWidth: 80
            ),
            []
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

    func testAgentThreadAccumulatesChunksByMessageID() {
        var thread = ZZAgentThread()
        let effect = thread.applyBatch(firstSeq: 1, items: [
            agentItem(1, #"update"#, #"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Hello"},"messageId":"m1"}"#),
            agentItem(2, #"update"#, #"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":" world"},"messageId":"m1"}"#),
            agentItem(3, #"update"#, #"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Next"},"messageId":"m2"}"#),
        ])

        XCTAssertEqual(effect, .applied)
        XCTAssertEqual(thread.cursor, 3)
        XCTAssertEqual(thread.blocks.count, 2)
        XCTAssertEqual(
            thread.blocks[0].kind,
            .agentText(messageID: "m1", text: "Hello world")
        )
        XCTAssertEqual(
            thread.blocks[1].kind,
            .agentText(messageID: "m2", text: "Next")
        )
    }

    func testAgentThreadTracksToolsAcrossCallAndUpdate() {
        var thread = ZZAgentThread()
        _ = thread.applyBatch(firstSeq: 1, items: [
            agentItem(1, #"update"#, #"{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Run tests","kind":"execute","locations":[{"path":"/work/app/main.swift","line":42}]}"#),
            agentItem(2, #"update"#, #"{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"in_progress"}"#),
            agentItem(3, #"update"#, #"{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"completed"}"#),
        ])

        XCTAssertEqual(thread.blocks.count, 1)
        guard case let .tool(call) = thread.blocks[0].kind else {
            return XCTFail("expected a tool block")
        }
        XCTAssertEqual(call.title, "Run tests")
        XCTAssertEqual(call.kind, .execute)
        XCTAssertEqual(call.status, .done)
        // An update that names neither title nor locations leaves both alone.
        XCTAssertEqual(call.location?.path, "/work/app/main.swift")
        XCTAssertEqual(call.target, "main.swift:42")
    }

    func testAgentToolUpdatesReplaceCollectionsOnlyWhenPresent() {
        var thread = ZZAgentThread()
        _ = thread.applyBatch(firstSeq: 1, items: [
            agentItem(1, #"update"#, #"{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Edit","kind":"edit","locations":[{"path":"/a/one.swift"}],"content":[{"type":"diff","path":"/a/one.swift","newText":"x"}]}"#),
        ])
        guard case let .tool(first) = thread.blocks[0].kind else {
            return XCTFail("expected a tool block")
        }
        XCTAssertEqual(first.location?.display, "one.swift")
        XCTAssertEqual(first.changedPaths, ["/a/one.swift"])

        // ACP replaces a collection when the key is present, so an empty array
        // clears it while an absent key leaves it untouched.
        _ = thread.applyBatch(firstSeq: 2, items: [
            agentItem(2, #"update"#, #"{"sessionUpdate":"tool_call_update","toolCallId":"t1","locations":[],"status":"completed"}"#),
        ])
        guard case let .tool(second) = thread.blocks[0].kind else {
            return XCTFail("expected a tool block")
        }
        XCTAssertNil(second.location)
        XCTAssertEqual(second.changedPaths, ["/a/one.swift"])
        XCTAssertEqual(second.status, .done)
        XCTAssertEqual(second.title, "Edit")
        XCTAssertEqual(second.kind, .edit)
        XCTAssertEqual(second.target, "one.swift")
    }

    func testAgentToolKindFallsBackForUnknownWireValues() {
        XCTAssertEqual(ZZAgentToolKind(wireValue: "execute"), .execute)
        XCTAssertEqual(ZZAgentToolKind(wireValue: "switch_mode"), .switchMode)
        XCTAssertEqual(ZZAgentToolKind(wireValue: "from_the_future"), .other)
        XCTAssertEqual(ZZAgentToolKind(wireValue: nil), .other)
    }

    func testAgentToolLocationDisplayPrefersBasenameAndLine() {
        XCTAssertEqual(
            ZZAgentToolLocation(path: "/work/app/Sources/Main.swift", line: 12).display,
            "Main.swift:12"
        )
        XCTAssertEqual(
            ZZAgentToolLocation(path: "/work/app/Sources/Main.swift", line: nil).display,
            "Main.swift"
        )
        XCTAssertNil(ZZAgentToolLocation.parse(["path": ""]))
        XCTAssertEqual(ZZAgentToolLocation.parse(["path": "/a/b"])?.line, nil)
    }

    func testMarkdownParsesHeadingsListsAndParagraphs() {
        let blocks = ZZAgentMarkdown.blocks(
            "## Plan\n\nlead in\n\n- Simple\n- Valid\n\n1. First\n2. Second"
        ).map(\.block)

        XCTAssertEqual(blocks, [
            .heading(level: 2, text: "Plan"),
            .paragraph("lead in"),
            .list(ZZMarkdownList(ordered: false, start: 1, items: [
                ZZMarkdownListItem(checked: nil, blocks: [.paragraph("Simple")]),
                ZZMarkdownListItem(checked: nil, blocks: [.paragraph("Valid")]),
            ])),
            .list(ZZMarkdownList(ordered: true, start: 1, items: [
                ZZMarkdownListItem(checked: nil, blocks: [.paragraph("First")]),
                ZZMarkdownListItem(checked: nil, blocks: [.paragraph("Second")]),
            ])),
        ])
    }

    func testMarkdownKeepsAdjacentFencesApart() {
        // The failure this replaced: a second fenced block's contents leaked
        // out as prose because fence tracking drifted between two blocks.
        let blocks = ZZAgentMarkdown.blocks(
            """
            intro

            ```md
            # Notes
            ```

            ```rust
            fn main() {}
            ```

            outro
            """
        ).map(\.block)

        XCTAssertEqual(blocks, [
            .paragraph("intro"),
            .code(language: "md", code: "# Notes"),
            .code(language: "rust", code: "fn main() {}"),
            .paragraph("outro"),
        ])
    }

    func testMarkdownHonorsLongerFencesAroundInnerOnes() {
        // A four-backtick fence closes only on four; the inner three-backtick
        // fence is content, not a delimiter.
        let source = "````md\n```swift\nlet x = 1\n```\n````\nafter"
        let blocks = ZZAgentMarkdown.blocks(source).map(\.block)

        XCTAssertEqual(blocks, [
            .code(language: "md", code: "```swift\nlet x = 1\n```"),
            .paragraph("after"),
        ])
    }

    func testMarkdownFenceWithInfoStringNeverClosesAnOuterBlock() {
        // Agents demonstrating markdown nest a language fence inside an `md`
        // one. A closing fence may carry no info string, so ```python is
        // content and the following bare ``` closes the outer block.
        let source = "```md\n## Title\n\n```python\nx = 1\n```\n\nafter"
        let blocks = ZZAgentMarkdown.blocks(source).map(\.block)

        XCTAssertEqual(blocks, [
            .code(language: "md", code: "## Title\n\n```python\nx = 1"),
            .paragraph("after"),
        ])
    }

    func testMarkdownTreatsAnOpenFenceAsCodeWhileStreaming() {
        // A block still arriving has no closing fence yet; it must not flash
        // as prose until one lands.
        let blocks = ZZAgentMarkdown.blocks("Try:\n\n```\nnpm test").map(\.block)

        XCTAssertEqual(blocks, [
            .paragraph("Try:"),
            .code(language: nil, code: "npm test"),
        ])
    }

    func testMarkdownParsesGFMTablesWithAlignments() {
        let blocks = ZZAgentMarkdown.blocks(
            """
            | Status | Task | Owner |
            |:-------|:----:|------:|
            | done | Login flow | Alex |
            | busy | Billing UI | Maya |
            """
        ).map(\.block)

        XCTAssertEqual(blocks.count, 1)
        guard case let .table(table) = blocks[0] else {
            return XCTFail("expected a table")
        }
        XCTAssertEqual(table.head, ["Status", "Task", "Owner"])
        XCTAssertEqual(table.rows, [
            ["done", "Login flow", "Alex"],
            ["busy", "Billing UI", "Maya"],
        ])
        XCTAssertEqual(table.columnCount, 3)
        XCTAssertEqual(table.alignment(0), .leading)
        XCTAssertEqual(table.alignment(1), .center)
        XCTAssertEqual(table.alignment(2), .trailing)
        // Past the last column the table answers rather than trapping.
        XCTAssertEqual(table.alignment(9), .unspecified)
        XCTAssertEqual(table.cell(row: table.head, column: 9), "")
    }

    func testMarkdownParsesTaskListsQuotesAndBreaks() {
        let blocks = ZZAgentMarkdown.blocks(
            """
            - [x] Set up repo
            - [ ] Ship MVP

            > Tip: automate deployment next.

            ---
            """
        ).map(\.block)

        XCTAssertEqual(blocks, [
            .list(ZZMarkdownList(ordered: false, start: 1, items: [
                ZZMarkdownListItem(checked: true, blocks: [.paragraph("Set up repo")]),
                ZZMarkdownListItem(checked: false, blocks: [.paragraph("Ship MVP")]),
            ])),
            .quote([.paragraph("Tip: automate deployment next.")]),
            .thematicBreak,
        ])
    }

    func testMarkdownKeepsInlineSyntaxForTheTextRenderer() {
        // Inline styling is `AttributedString`'s job, so the block model hands
        // it the source rather than flattening it.
        let blocks = ZZAgentMarkdown.blocks("Keep config in one `config.toml`").map(\.block)
        XCTAssertEqual(blocks, [.paragraph("Keep config in one `config.toml`")])

        let styled = ZZAgentMarkdown.inline("a **bold** word")
        XCTAssertEqual(String(styled.characters), "a bold word")
    }

    func testMarkdownNestsListsAndOrdersFromTheirStart() {
        let blocks = ZZAgentMarkdown.blocks(
            """
            3. third
            4. fourth
               - nested
            """
        ).map(\.block)

        XCTAssertEqual(blocks, [
            .list(ZZMarkdownList(ordered: true, start: 3, items: [
                ZZMarkdownListItem(checked: nil, blocks: [.paragraph("third")]),
                ZZMarkdownListItem(checked: nil, blocks: [
                    .paragraph("fourth"),
                    .list(ZZMarkdownList(ordered: false, start: 1, items: [
                        ZZMarkdownListItem(checked: nil, blocks: [.paragraph("nested")]),
                    ])),
                ]),
            ])),
        ])
    }

    func testAgentThreadRevisionAndSubmittedTurnsAdvance() {
        var thread = ZZAgentThread()
        XCTAssertEqual(thread.revision, 0)
        XCTAssertEqual(thread.submittedTurns, 0)

        thread.appendUserTurn("go")
        let afterSubmit = thread.revision
        XCTAssertGreaterThan(afterSubmit, 0)
        XCTAssertEqual(thread.submittedTurns, 1)

        _ = thread.applyBatch(firstSeq: 1, items: [
            agentItem(1, #"update"#, #"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hi"},"messageId":"m1"}"#),
        ])
        XCTAssertGreaterThan(thread.revision, afterSubmit)
        // Streaming is not a submission: the view uses this to tell them apart.
        XCTAssertEqual(thread.submittedTurns, 1)
    }

    func testAgentChunkTextDegradesNonTextBlocks() {
        var thread = ZZAgentThread()
        _ = thread.applyBatch(firstSeq: 1, items: [
            agentItem(1, #"update"#, #"{"sessionUpdate":"agent_message_chunk","content":{"type":"image","mimeType":"image/png"},"messageId":"m1"}"#),
            agentItem(2, #"update"#, #"{"sessionUpdate":"agent_message_chunk","content":{"type":"resource_link","uri":"file:///a/b.txt","name":"b.txt"},"messageId":"m2"}"#),
        ])

        XCTAssertEqual(
            thread.blocks[0].kind,
            .agentText(messageID: "m1", text: "*[Image: image/png]*")
        )
        XCTAssertEqual(
            thread.blocks[1].kind,
            .agentText(messageID: "m2", text: "[b.txt](file:///a/b.txt)")
        )
    }

    func testAgentThreadSkipsUserEchoesAndUnknownTags() {
        var thread = ZZAgentThread()
        let effect = thread.applyBatch(firstSeq: 1, items: [
            agentItem(1, #"update"#, #"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"hi"},"messageId":"u1"}"#),
            agentItem(2, #"update"#, #"{"sessionUpdate":"from_the_future"}"#),
            agentItem(3, #"plan"#, #"{"unknown":true}"#),
        ])

        XCTAssertEqual(effect, .applied)
        XCTAssertEqual(thread.cursor, 3)
        XCTAssertTrue(thread.blocks.isEmpty)
    }

    func testAgentThreadRequestsReplayOnGapAndClosesIt() {
        var thread = ZZAgentThread()
        _ = thread.applyBatch(firstSeq: 1, items: [
            agentItem(1, #"update"#, #"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"A"},"messageId":"m1"}"#),
            agentItem(2, #"update"#, #"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"B"},"messageId":"m1"}"#),
        ])

        let gap = thread.applyBatch(firstSeq: 5, items: [
            agentItem(5, #"update"#, #"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"E"},"messageId":"m1"}"#),
        ])
        XCTAssertEqual(gap, .needsReplay)
        XCTAssertEqual(thread.cursor, 2)

        thread.markReplayPending()
        let replay = thread.applyBatch(firstSeq: 2, items: [
            agentItem(2, #"update"#, #"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"B"},"messageId":"m1"}"#),
            agentItem(3, #"update"#, #"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"C"},"messageId":"m1"}"#),
            agentItem(4, #"update"#, #"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"D"},"messageId":"m1"}"#),
            agentItem(5, #"update"#, #"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"E"},"messageId":"m1"}"#),
        ])
        XCTAssertEqual(replay, .applied)
        XCTAssertFalse(thread.replayPending)
        XCTAssertEqual(thread.cursor, 5)
        XCTAssertEqual(thread.blocks.count, 1)
        XCTAssertEqual(
            thread.blocks[0].kind,
            .agentText(messageID: "m1", text: "ABCDE")
        )
    }

    func testAgentThreadRestoringResetJumpsTheGapAndKeepsTurns() {
        var thread = ZZAgentThread()
        thread.appendUserTurn("fix it")
        _ = thread.applyBatch(firstSeq: 1, items: [
            agentItem(1, #"update"#, #"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"old"},"messageId":"m1"}"#),
            agentItem(2, #"update"#, #"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":" news"},"messageId":"m1"}"#),
        ])

        let effect = thread.applyBatch(firstSeq: 10, items: [
            agentItem(10, #"sessionReset"#, #"{"restoring":true}"#),
            agentItem(11, #"update"#, #"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"fresh"},"messageId":"m9"}"#),
        ])

        XCTAssertEqual(effect, .applied)
        XCTAssertEqual(thread.cursor, 11)
        XCTAssertEqual(thread.blocks.count, 2)
        XCTAssertTrue(thread.blocks[0].isUserTurn)
        XCTAssertEqual(
            thread.blocks[1].kind,
            .agentText(messageID: "m9", text: "fresh")
        )
    }

    func testAgentThreadDropsUndecodableItemsPastThem() {
        var thread = ZZAgentThread()
        let effect = thread.applyBatch(firstSeq: 1, items: [
            Data("not json".utf8),
            agentItem(2, #"update"#, #"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"ok"},"messageId":"m1"}"#),
        ])

        XCTAssertEqual(effect, .applied)
        XCTAssertEqual(thread.cursor, 2)
        XCTAssertEqual(thread.blocks.count, 1)
        XCTAssertEqual(
            thread.blocks[0].kind,
            .agentText(messageID: "m1", text: "ok")
        )
    }

    func testAgentThreadSettlesTurnsAndResetsStream() {
        var thread = ZZAgentThread()
        thread.appendUserTurn("first")
        thread.appendUserTurn("second")
        thread.settleOldestWorkingTurn(.done)
        thread.settleOldestWorkingTurn(.failed)

        let receipts = thread.blocks.compactMap { block -> ZZAgentTurnStatus? in
            if case let .user(turn) = block.kind {
                return turn.status
            }
            return nil
        }
        XCTAssertEqual(receipts, [.done, .failed])

        _ = thread.applyBatch(firstSeq: 1, items: [
            agentItem(1, #"update"#, #"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hi"},"messageId":"m1"}"#),
        ])
        thread.resetStream()

        XCTAssertEqual(thread.cursor, 0)
        XCTAssertFalse(thread.replayPending)
        XCTAssertEqual(thread.blocks.count, 2)
        XCTAssertTrue(thread.blocks.allSatisfy(\.isUserTurn))
    }

    func testAgentConfigOptionsParseSelectsAndSkipBooleans() {
        let json = """
        [
          {"id":"model","name":"Model","category":"model","type":"select",
           "currentValue":"opus","options":[
             {"value":"opus","name":"Opus","description":"Big brain"},
             {"value":"sonnet","name":"Sonnet"}]},
          {"id":"effort","name":"Effort","category":"thought_level","type":"select",
           "currentValue":"high","options":[
             {"label":"High","options":[
               {"value":"high","name":"High"},
               {"value":"max","name":"Max"}]},
             {"value":"low","name":"Low"}]},
          {"id":"fancy","name":"Fancy","type":"boolean","currentValue":true},
          {"id":"broken","name":"Broken","type":"select"}
        ]
        """
        let options = ZZAgentConfigOption.parseAll(Data(json.utf8))

        XCTAssertEqual(options.count, 2)
        XCTAssertEqual(options[0].category, .model)
        XCTAssertEqual(options[0].currentChoiceName, "Opus")
        XCTAssertEqual(options[0].choices.map(\.value), ["opus", "sonnet"])
        XCTAssertEqual(options[0].choices[0].description, "Big brain")
        XCTAssertEqual(options[1].category, .thoughtLevel)
        XCTAssertEqual(options[1].choices.map(\.value), ["high", "max", "low"])
        XCTAssertEqual(options[1].currentChoiceName, "High")
    }

    func testAgentModeStateParsesCurrentAndChoices() {
        let json = """
        {"currentModeId":"plan","availableModes":[
          {"id":"default","name":"Default"},
          {"id":"plan","name":"Plan","description":"Think first"}]}
        """
        let state = ZZAgentModeState.parse(Data(json.utf8))

        XCTAssertEqual(state?.currentID, "plan")
        XCTAssertEqual(state?.currentName, "Plan")
        XCTAssertEqual(state?.modes.map(\.id), ["default", "plan"])
        XCTAssertNil(ZZAgentModeState.parse(Data("[]".utf8)))
    }

    func testAgentSessionSummariesParseListFailureAndEncoding() {
        let json = """
        {"item":"sessionsListed","replace":true,"sessions":[
          {"sessionId":"s-1","cwd":"/work/app","additionalDirectories":["/work/lib"],
           "title":"Fix bug","updatedAt":"2026-09-01"},
          {"sessionId":"s-2","cwd":"/work/other","additionalDirectories":[]}]}
        """
        let sessions = ZZAgentSessionSummary.parseList(Data(json.utf8))

        XCTAssertEqual(sessions?.count, 2)
        XCTAssertEqual(sessions?[0].displayTitle, "Fix bug")
        XCTAssertEqual(sessions?[1].displayTitle, "other")
        let encodedDirs = sessions?[0].additionalDirectoriesJSON() ?? ""
        let decodedDirs = try? JSONSerialization.jsonObject(with: Data(encodedDirs.utf8)) as? [String]
        XCTAssertEqual(decodedDirs, ["/work/lib"])
        XCTAssertEqual(
            ZZAgentSessionSummary.parseListFailure(
                Data(#"{"item":"sessionListFailed","message":"nope"}"#.utf8)
            ),
            "nope"
        )
        XCTAssertNil(ZZAgentSessionSummary.parseList(Data(#"{"item":"plan"}"#.utf8)))
    }

    private func agentItem(_ seq: UInt64, _ item: String, _ payload: String) -> Data {
        if item == "update" {
            return Data(#"{"seq":\#(seq),"item":"update","update":\#(payload)}"#.utf8)
        }
        let inner = payload.dropFirst().dropLast()
        return Data(#"{"seq":\#(seq),"item":"\#(item)",\#(inner)}"#.utf8)
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

    func testPrefixKeySpellingMirrorsDesktopHints() {
        XCTAssertEqual(ZZKeySpelling.display("D-="), "cmd-=")
        XCTAssertEqual(ZZKeySpelling.display("D-M-Right"), "cmd-alt-right")
        XCTAssertEqual(ZZKeySpelling.display("D-S-["), "cmd-shift-[")
        XCTAssertEqual(ZZKeySpelling.display("C-S-Tab"), "ctrl-shift-tab")
        XCTAssertEqual(ZZKeySpelling.display("C-NPage"), "ctrl-pagedown")
        XCTAssertEqual(ZZKeySpelling.display("C-PPage"), "ctrl-pageup")
        XCTAssertEqual(ZZKeySpelling.display("M-Left"), "alt-left")
        XCTAssertEqual(ZZKeySpelling.display("M-A"), "alt-shift-a")
        XCTAssertEqual(ZZKeySpelling.display("F5"), "f5")
        XCTAssertEqual(ZZKeySpelling.display("G"), "shift-g")
        XCTAssertEqual(ZZKeySpelling.display("Escape"), "escape")
        XCTAssertEqual(ZZKeySpelling.display(":"), ":")
        XCTAssertEqual(ZZKeySpelling.display("C-,"), "ctrl-,")
        XCTAssertEqual(ZZKeySpelling.display("C- "), "ctrl-space")
        XCTAssertEqual(ZZKeySpelling.display("BSpace"), "backspace")
        XCTAssertEqual(
            ZZPrefixBinding(key: "M-1", summary: "select-window -t :1", note: "", repeats: false)
                .displayKey,
            "alt-1"
        )
    }

    @MainActor
    func testHardwareKeysUseRawMappingWithoutKeyCommands() {
        let view = TerminalGridView()
        XCTAssertTrue(view.keyCommands?.isEmpty ?? true)

        let arrow = TerminalGridView.map(
            keyCode: .keyboardLeftArrow,
            charactersIgnoringModifiers: "",
            modifierFlags: []
        )
        XCTAssertEqual(arrow?.code, UInt32(ZZ_KEY_ARROW_LEFT.rawValue))
        XCTAssertEqual(arrow?.scalar, 0)

        let controlA = TerminalGridView.map(
            keyCode: .keyboardA,
            charactersIgnoringModifiers: "a",
            modifierFlags: .control
        )
        XCTAssertEqual(controlA?.code, UInt32(ZZ_KEY_CHARACTER.rawValue))
        XCTAssertEqual(controlA?.scalar, UnicodeScalar("a").value)

        XCTAssertNil(
            TerminalGridView.map(
                keyCode: .keyboardA,
                charactersIgnoringModifiers: "a",
                modifierFlags: []
            )
        )
    }

    func testCommandPromptSplitsNameAndArguments() {
        let parsed = ZZCommandLine.split("  kill-pane   -t  %1  ")
        XCTAssertEqual(parsed?.name, "kill-pane")
        XCTAssertEqual(parsed?.args, ["-t", "%1"])
        XCTAssertEqual(ZZCommandLine.split("list-keys")?.args, [])
        XCTAssertNil(ZZCommandLine.split("   "))
    }

    func testOverlayShortcutsReuseExistingDaemonCommands() {
        XCTAssertEqual(ZZCommandLine.chooseBufferArgs, [])
        XCTAssertEqual(ZZCommandLine.displayPanesArgs, ["-d", "0"])
    }

    func testTmuxImportOffersOncePerHost() {
        XCTAssertFalse(ZZTMuxImport.shouldOffer(endpoint: "", offered: []))
        XCTAssertTrue(ZZTMuxImport.shouldOffer(endpoint: "ssh://fab@mini", offered: []))
        XCTAssertTrue(
            ZZTMuxImport.promptMessage(endpoint: "ssh://fab@mini")
                .contains("replaces zz/mux.conf on the host")
        )
        XCTAssertFalse(
            ZZTMuxImport.shouldOffer(
                endpoint: "ssh://fab@mini",
                offered: ["ssh://fab@mini"]
            )
        )
        XCTAssertTrue(
            ZZTMuxImport.shouldOffer(
                endpoint: "ssh://fab@other",
                offered: ["ssh://fab@mini"]
            )
        )
    }

    func testTmuxImportOfferedHostsRoundTrip() {
        let suite = "TmuxImportTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defaults.removePersistentDomain(forName: suite)
        defer {
            defaults.removePersistentDomain(forName: suite)
        }
        XCTAssertTrue(ZZTMuxImport.offeredHosts(in: defaults).isEmpty)
        ZZTMuxImport.markOffered(endpoint: "ssh://fab@mini", in: defaults)
        XCTAssertEqual(
            ZZTMuxImport.offeredHosts(in: defaults),
            ["ssh://fab@mini"]
        )
    }

    func testTmuxImportResultMessage() {
        let binding = ZZPrefixBinding(key: "h", summary: "select-pane -L", note: "", repeats: true)
        XCTAssertNil(ZZTMuxImport.resultMessage(baseline: [binding], current: [binding]))
        XCTAssertEqual(
            ZZTMuxImport.resultMessage(baseline: [], current: [binding]),
            "Imported 1 new binding. It’s live now."
        )
        let rebound = ZZPrefixBinding(
            key: "h",
            summary: "select-pane -R",
            note: "",
            repeats: true
        )
        XCTAssertEqual(
            ZZTMuxImport.resultMessage(baseline: [binding], current: [rebound]),
            "Tmux config imported and reloaded."
        )
    }

    func testTmuxImportPhaseAlertRouting() {
        XCTAssertFalse(ZZTMuxImportPhase.hidden.needsAlert)
        XCTAssertFalse(ZZTMuxImportPhase.working(baseline: []).needsAlert)
        let prompting = ZZTMuxImportPhase.prompting(endpoint: "ssh://fab@mini")
        XCTAssertTrue(prompting.needsAlert)
        XCTAssertEqual(prompting.promptEndpoint, "ssh://fab@mini")
        XCTAssertNil(prompting.resultMessage)
        let done = ZZTMuxImportPhase.done(message: "Imported 1 new binding. It’s live now.")
        XCTAssertTrue(done.needsAlert)
        XCTAssertNil(done.promptEndpoint)
        XCTAssertEqual(done.resultMessage, "Imported 1 new binding. It’s live now.")
    }
}
