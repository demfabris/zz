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
    case needsHost(String?)
    case connecting
    case reconnecting(attempt: Int, delay: Int)
    case connected
    case disconnected
    case failed(String)
}

enum ZZConnectFailure: UInt32, Sendable {
    case none = 0
    case retryable = 1
    case authentication = 2
    case hostKey = 3
    case configuration = 4
    case incompatible = 5

    var shouldRetry: Bool {
        self == .retryable
    }
}

enum ZZReconnectPolicy {
    static func delay(for attempt: Int) -> Int {
        1 << min(max(attempt - 1, 0), 4)
    }
}

enum ZZHostEndpoint {
    static func normalized(_ value: String) -> String? {
        let value = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !value.isEmpty else {
            return nil
        }
        let endpoint = value.contains("://") ? value : "ssh://\(value)"
        let authority = endpoint
            .dropFirst("ssh://".count)
            .split(separator: "/", maxSplits: 1)
            .first
        guard endpoint.hasPrefix("ssh://"), authority?.contains("@") == true else {
            return nil
        }
        return endpoint
    }
}

enum ZZSSHPromptKind: UInt32, Sendable {
    case secret = 0
    case hostKey = 1
    case confirmation = 2
}

struct ZZSSHPromptRequest: Identifiable, Equatable, Sendable {
    let id: UInt64
    let kind: ZZSSHPromptKind
    let title: String
    let message: String
    let echo: Bool
}

enum ZZSSHPromptAnswer: Equatable, Sendable {
    case cancel
    case answer(String)
    case trustOnce
    case trustAndSave
}

enum ZZAgentPhase: UInt32, Equatable, Sendable {
    case starting = 0
    case ready = 1
    case running = 2
    case awaitingPermission = 3
    case failed = 4

    var label: String {
        switch self {
        case .starting: "Starting"
        case .ready: "Ready"
        case .running: "Working"
        case .awaitingPermission: "Needs approval"
        case .failed: "Failed"
        }
    }
}

enum ZZAgentPermissionKind: String, Equatable, Sendable {
    case allowOnce = "allow_once"
    case allowAlways = "allow_always"
    case rejectOnce = "reject_once"
    case rejectAlways = "reject_always"
    case unknown

    var isApproval: Bool {
        self == .allowOnce || self == .allowAlways
    }
}

struct ZZAgentPermissionOption: Identifiable, Equatable, Sendable {
    let id: String
    let name: String
    let kind: ZZAgentPermissionKind
}

struct ZZAgentPermission: Equatable, Sendable {
    let requestID: UInt64
    let title: String
    let options: [ZZAgentPermissionOption]
}

struct ZZAgentGitSummary: Equatable, Sendable {
    let branch: String?
    let changedFiles: UInt32
    let additions: UInt32
    let deletions: UInt32
}

struct ZZAgentState: Equatable, Sendable {
    let pane: UInt64
    let phase: ZZAgentPhase
    let status: ZZAgentStatus
    let queuedPrompts: UInt32
    let sessionID: String?
    let title: String?
    let error: String?
    let permission: ZZAgentPermission?
    let git: ZZAgentGitSummary?
}

enum ZZAgentStatus: UInt32, Equatable, Sendable {
    case idle = 0
    case working = 1
    case needsInput = 2
    case failed = 3
}

enum ZZAgentAttentionKind: Int, Comparable, Sendable {
    case working = 0
    case done = 1
    case failed = 2
    case blocked = 3

    static func < (lhs: Self, rhs: Self) -> Bool {
        lhs.rawValue < rhs.rawValue
    }

    var label: String {
        switch self {
        case .working: "Working"
        case .done: "Done"
        case .failed: "Failed"
        case .blocked: "Needs approval"
        }
    }

    var symbol: String {
        switch self {
        case .working: "sparkles"
        case .done: "checkmark.circle.fill"
        case .failed: "exclamationmark.triangle.fill"
        case .blocked: "hand.raised.fill"
        }
    }
}

struct ZZAgentAttention: Identifiable, Equatable, Sendable {
    let pane: UInt64
    let session: UInt64?
    let title: String
    let kind: ZZAgentAttentionKind

    var id: UInt64 { pane }
}

struct TerminalModifierLatchState: Equatable, Sendable {
    private(set) var active: UInt8 = 0
    private(set) var locked: UInt8 = 0
    private var lastTap: [UInt8: TimeInterval] = [:]

    mutating func tap(_ bit: UInt8, at time: TimeInterval) {
        if locked & bit != 0 {
            active &= ~bit
            locked &= ~bit
            lastTap.removeValue(forKey: bit)
            return
        }
        if active & bit != 0 {
            if let previous = lastTap[bit], time - previous <= 0.4 {
                locked |= bit
                lastTap.removeValue(forKey: bit)
            } else {
                active &= ~bit
                lastTap.removeValue(forKey: bit)
            }
            return
        }
        active |= bit
        lastTap[bit] = time
    }

    mutating func consumeOneShot() {
        active = locked
        lastTap = lastTap.filter { locked & $0.key != 0 }
    }

    mutating func reset() {
        self = Self()
    }

    func contains(_ bit: UInt8) -> Bool {
        active & bit != 0
    }

    func isLocked(_ bit: UInt8) -> Bool {
        locked & bit != 0
    }
}

struct ZZNavigationTarget: Equatable, Sendable {
    let session: UInt64?
    let pane: UInt64?
    let attention: Bool

    init?(url: URL) {
        guard url.scheme?.lowercased() == "zz",
              let host = url.host?.lowercased(),
              ["open", "pane", "attention"].contains(host) else {
            return nil
        }
        let components = URLComponents(url: url, resolvingAgainstBaseURL: false)
        session = components?.queryItems?
            .first { $0.name == "session" }?
            .value
            .flatMap(UInt64.init)
        pane = components?.queryItems?
            .first { $0.name == "pane" }?
            .value
            .flatMap(UInt64.init)
        attention = host == "attention"
        if session == nil, pane == nil, !attention {
            return nil
        }
    }

    init(session: UInt64?, pane: UInt64?) {
        self.session = session
        self.pane = pane
        attention = false
    }
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
