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
