import XCTest
@testable import ZZ

final class TmuxControlsTests: XCTestCase {
    func testDaemonOverlayStateRetainsChooserLabelsAndCopyPosition() throws {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let data = Data(#"{"prompt":null,"tree":{"items":[{"label":"work","detail":"2 panes","depth":1,"flags":7,"key":"a","text":"custom row"}],"selected":0,"search":{"query":"wor","reverse":false},"prompt":"","help":false},"buffers":null,"display":null,"confirm":null,"menu":null,"output":null,"copies":[{"pane":8,"position":12,"total":100,"matches":3,"match_index":2}]}"#.utf8)
        let state = try decoder.decode(ZZTmuxState.self, from: data)
        XCTAssertTrue(state.hasOverlay)
        XCTAssertEqual(state.tree?.items.first?.text, "custom row")
        XCTAssertEqual(state.tree?.items.first?.key, "a")
        XCTAssertEqual(state.tree?.search?.query, "wor")
        XCTAssertEqual(state.copies.first?.pane, 8)
        XCTAssertEqual(state.copies.first?.matchIndex, 2)
    }

    func testLiveModeDoesNotObscureTerminal() throws {
        let state = try JSONDecoder().decode(ZZTmuxState.self, from: Data(#"{"copies":[]}"#.utf8))
        XCTAssertFalse(state.hasOverlay)
        XCTAssertTrue(state.copies.isEmpty)
    }

    func testCustomDaemonBindingTablesRemainVisible() throws {
        let data = Data(#"[{"name":"my-table","bindings":[{"key":"M-X","summary":"split-window -h","note":"Custom split","repeats":true}]}]"#.utf8)
        let tables = try JSONDecoder().decode([ZZTmuxKeyTable].self, from: data)
        XCTAssertEqual(tables.first?.name, "my-table")
        XCTAssertEqual(tables.first?.bindings.first?.key, "M-X")
        XCTAssertEqual(tables.first?.bindings.first?.note, "Custom split")
        XCTAssertEqual(tables.first?.bindings.first?.repeats, true)
    }
}
