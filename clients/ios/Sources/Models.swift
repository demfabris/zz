import Foundation

enum ZZPaneKind: UInt32, Sendable {
    case picker = 0
    case terminal = 1
    case browser = 2
    case agent = 3
    case editor = 4

    var label: String {
        switch self {
        case .picker: "New pane"
        case .terminal: "Terminal"
        case .browser: "Browser"
        case .agent: "Agent"
        case .editor: "Editor"
        }
    }

    var symbol: String {
        switch self {
        case .picker: "plus.rectangle.on.rectangle"
        case .terminal: "terminal"
        case .browser: "globe"
        case .agent: "sparkles"
        case .editor: "doc.text"
        }
    }
}

struct ZZPane: Identifiable, Equatable, Sendable {
    let id: UInt64
    let title: String
    let kind: ZZPaneKind
    let isActive: Bool
    let hasBell: Bool
}

struct ZZSession: Identifiable, Equatable, Sendable {
    let id: UInt64
    let name: String
    let activeWindow: UInt64
    let panes: [ZZPane]
    let isAttached: Bool
}

enum ZZConnectionState: Equatable, Sendable {
    case idle
    case connecting
    case connected
    case disconnected
    case failed(String)
}

struct TerminalDamage: Equatable, Sendable {
    let all: Bool
    let firstRow: Int
    let lastRow: Int

    static let full = Self(all: true, firstRow: 0, lastRow: .max)
}

enum TerminalInputOwner: Equatable, Sendable {
    case none
    case pane(UInt64)

    var pane: UInt64? {
        switch self {
        case .none: nil
        case let .pane(id): id
        }
    }

    func owns(_ pane: UInt64) -> Bool {
        self == .pane(pane)
    }
}

struct TerminalInputState: Equatable, Sendable {
    private(set) var owner: TerminalInputOwner = .none
    private(set) var activation: UInt64 = 0

    mutating func acquire(_ pane: UInt64) {
        owner = .pane(pane)
        activation &+= 1
    }

    mutating func release() {
        owner = .none
    }
}

struct TerminalLayout: Equatable, Sendable {
    let columns: UInt16
    let rows: UInt16
    let cellWidth: UInt32
    let cellHeight: UInt32

    init(columns: Int, rows: Int, cell: CGSize) {
        self.columns = UInt16(clamping: columns)
        self.rows = UInt16(clamping: rows)
        self.cellWidth = UInt32(clamping: Int(cell.width.rounded(.up)))
        self.cellHeight = UInt32(clamping: Int(cell.height.rounded(.up)))
    }

    init?(bounds: CGSize, cell: CGSize) {
        guard bounds.width > 0, bounds.height > 0,
              cell.width > 0, cell.height > 0,
              bounds.width.isFinite, bounds.height.isFinite,
              cell.width.isFinite, cell.height.isFinite else {
            return nil
        }
        self.init(
            columns: max(1, Int(floor(bounds.width / cell.width))),
            rows: max(1, Int(floor(bounds.height / cell.height))),
            cell: cell
        )
    }
}

struct TerminalGeometryState: Equatable, Sendable {
    private(set) var lastSent: TerminalLayout?
    private(set) var lastStable: TerminalLayout?

    mutating func observe(_ layout: TerminalLayout, stable: Bool) -> Bool {
        if stable {
            lastStable = layout
        }
        return lastSent != layout
    }

    mutating func markSent(_ layout: TerminalLayout) {
        lastSent = layout
    }

    mutating func invalidateSent() {
        lastSent = nil
    }

    var reconnectLayout: TerminalLayout? {
        lastSent == nil ? lastStable : nil
    }

    var stableLayoutToRestore: TerminalLayout? {
        guard let lastStable, lastStable != lastSent else {
            return nil
        }
        return lastStable
    }
}
