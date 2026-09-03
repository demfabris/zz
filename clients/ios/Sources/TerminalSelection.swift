import UIKit

struct TerminalGridCell: Equatable {
    let column: Int
    let row: Int

    init(column: Int, row: Int) {
        self.column = column
        self.row = row
    }

    init(point: CGPoint, cellSize: CGSize, columns: Int, rows: Int) {
        column = min(
            max(Int(floor(point.x / max(cellSize.width, 1))), 0),
            max(columns - 1, 0)
        )
        row = min(
            max(Int(floor(point.y / max(cellSize.height, 1))), 0),
            max(rows - 1, 0)
        )
    }
}

struct TerminalSelectionFeedback {
    private var reported: TerminalGridCell?

    mutating func shouldTick(at cell: TerminalGridCell) -> Bool {
        guard reported != cell else {
            return false
        }
        reported = cell
        return true
    }

    mutating func reset() {
        reported = nil
    }
}

@MainActor
final class TerminalSelectionLoupe {
    private var session: UITextLoupeSession?

    func begin(at point: CGPoint, in view: UIView) {
        end()
        guard view.window != nil else {
            return
        }
        session = UITextLoupeSession.begin(at: point, fromSelectionWidgetView: nil, in: view)
        move(to: point)
    }

    func move(to point: CGPoint) {
        session?.move(to: point, withCaretRect: .null, trackingCaret: false)
    }

    func end() {
        session?.invalidate()
        session = nil
    }
}
