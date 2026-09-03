import Combine
import Foundation
import Markdown

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

    /// Kinds the daemon will read `show-last-output` from: it captures OSC 133
    /// marks out of a pane that owns a terminal.
    var recordsCommands: Bool {
        self == .terminal || self == .agent
    }
}

struct ZZPane: Identifiable, Equatable, Sendable {
    let id: UInt64
    let title: String
    let kind: ZZPaneKind
    let isActive: Bool
    let hasBell: Bool
    let layout: ZZPaneLayout?
}

struct ZZPaneLayout: Equatable, Sendable {
    let x: Float
    let y: Float
    let width: Float
    let height: Float
}

struct ZZWindow: Identifiable, Equatable, Sendable {
    let id: UInt64
    let index: UInt32
    let name: String
    let isCurrent: Bool
    let activePane: UInt64
    let zoomedPane: UInt64?
    let panes: [ZZPane]

    var visiblePanes: [ZZPane] {
        panes.filter { $0.layout != nil }
    }
}

struct ZZSession: Identifiable, Equatable, Sendable {
    let id: UInt64
    let name: String
    let activeWindowID: UInt64
    let windows: [ZZWindow]
    let isAttached: Bool

    var activeWindow: ZZWindow? {
        windows.first { $0.id == activeWindowID }
    }

    var panes: [ZZPane] {
        activeWindow?.panes ?? []
    }

    var allPanes: [ZZPane] {
        windows.flatMap(\.panes)
    }
}

enum ZZConnectionState: Equatable, Sendable {
    case idle
    case needsHost(String?)
    case connecting
    case reconnecting(attempt: Int, delay: Int, error: String?)
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
    static let fastTierAttempts = 5
    static let longOutageDelays = [30, 60, 120, 300, 600]
    static let thawGraceSeconds: TimeInterval = 5
    static let thawRetryDelay = 2

    static func delay(for attempt: Int) -> Int {
        1 << min(max(attempt - 1, 0), 4)
    }

    static func backoffDelay(for attempt: Int) -> Int {
        guard attempt > fastTierAttempts else {
            return delay(for: attempt)
        }
        let tier = min(attempt - fastTierAttempts, longOutageDelays.count)
        return longOutageDelays[tier - 1]
    }

    static func nextAttempt(after attempt: Int, thawing: Bool) -> Int {
        thawing ? max(attempt, 1) : attempt + 1
    }

    static func delaySeconds(attempt: Int, thawing: Bool) -> Int {
        thawing ? thawRetryDelay : backoffDelay(for: attempt)
    }
}

enum ZZTMuxImportPhase: Equatable {
    case hidden
    case prompting(endpoint: String)
    case working(baseline: [ZZPrefixBinding])
    case done(message: String)

    var needsAlert: Bool {
        promptEndpoint != nil || resultMessage != nil
    }

    var promptEndpoint: String? {
        if case .prompting(let endpoint) = self {
            return endpoint
        }
        return nil
    }

    var resultMessage: String? {
        if case .done(let message) = self {
            return message
        }
        return nil
    }
}

enum ZZTMuxImport {
    static let offeredHostsKey = "zz.tmux-import-offered-hosts"
    static let settleDelayNanoseconds: UInt64 = 8_000_000_000

    static func shouldOffer(endpoint: String, offered: Set<String>) -> Bool {
        !endpoint.isEmpty && !offered.contains(endpoint)
    }

    static func offeredHosts(in defaults: UserDefaults) -> Set<String> {
        Set(defaults.stringArray(forKey: offeredHostsKey) ?? [])
    }

    static func markOffered(endpoint: String, in defaults: UserDefaults) {
        var offered = offeredHosts(in: defaults)
        offered.insert(endpoint)
        defaults.set(Array(offered), forKey: offeredHostsKey)
    }

    static func promptMessage(endpoint: String) -> String {
        "Import \(endpoint)’s tmux config? This replaces zz/mux.conf on the host, then reloads it so custom binds work in zz."
    }

    static func successMessage(added: Int) -> String {
        added == 1
            ? "Imported 1 new binding. It’s live now."
            : "Imported \(added) new bindings. They’re live now."
    }

    static func resultMessage(baseline: [ZZPrefixBinding], current: [ZZPrefixBinding]) -> String? {
        guard current != baseline else {
            return nil
        }
        let before = Set(baseline.map(\.key))
        let added = current.filter({ !before.contains($0.key) }).count
        if added > 0 {
            return successMessage(added: added)
        }
        return "Tmux config imported and reloaded."
    }

    static let unchangedMessage =
        "No new bindings. The host has no tmux config, or it adds none."
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

enum ZZAgentConfigCategory: Equatable, Sendable {
    case mode
    case model
    case thoughtLevel
    case other

    init(wireValue: String?) {
        switch wireValue {
        case "mode": self = .mode
        case "model": self = .model
        case "thought_level": self = .thoughtLevel
        default: self = .other
        }
    }
}

struct ZZAgentConfigChoice: Identifiable, Equatable, Sendable {
    let value: String
    let name: String
    let description: String?

    var id: String { value }
}

struct ZZAgentConfigOption: Identifiable, Equatable, Sendable {
    let id: String
    let name: String
    let description: String?
    let category: ZZAgentConfigCategory
    let currentValue: String
    let choices: [ZZAgentConfigChoice]

    static func parseAll(_ data: Data) -> [ZZAgentConfigOption] {
        guard let json = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] else {
            return []
        }
        return json.compactMap(parse)
    }

    static func parse(_ dict: [String: Any]) -> ZZAgentConfigOption? {
        guard let id = dict["id"] as? String,
              let name = dict["name"] as? String,
              dict["type"] as? String == "select",
              let currentValue = dict["currentValue"] as? String
        else {
            return nil
        }
        return ZZAgentConfigOption(
            id: id,
            name: name,
            description: dict["description"] as? String,
            category: ZZAgentConfigCategory(wireValue: dict["category"] as? String),
            currentValue: currentValue,
            choices: parseChoices(dict["options"])
        )
    }

    static func parseChoices(_ raw: Any?) -> [ZZAgentConfigChoice] {
        let groups: [[String: Any]]
        if let flat = raw as? [[String: Any]] {
            groups = flat
        } else {
            groups = []
        }
        var choices: [ZZAgentConfigChoice] = []
        for entry in groups {
            if let nested = entry["options"] as? [[String: Any]] {
                choices.append(contentsOf: nested.compactMap(parseChoice))
            } else if let choice = parseChoice(entry) {
                choices.append(choice)
            }
        }
        return choices
    }

    static func parseChoice(_ dict: [String: Any]) -> ZZAgentConfigChoice? {
        guard let value = dict["value"] as? String,
              let name = dict["name"] as? String
        else {
            return nil
        }
        return ZZAgentConfigChoice(
            value: value,
            name: name,
            description: dict["description"] as? String
        )
    }

    var currentChoiceName: String {
        choices.first(where: { $0.value == currentValue })?.name ?? name
    }
}

struct ZZAgentSessionMode: Identifiable, Equatable, Sendable {
    let id: String
    let name: String
    let description: String?

    static func parse(_ dict: [String: Any]) -> ZZAgentSessionMode? {
        guard let id = dict["id"] as? String,
              let name = dict["name"] as? String
        else {
            return nil
        }
        return ZZAgentSessionMode(
            id: id,
            name: name,
            description: dict["description"] as? String
        )
    }
}

struct ZZAgentModeState: Equatable, Sendable {
    let currentID: String
    let modes: [ZZAgentSessionMode]

    static func parse(_ data: Data) -> ZZAgentModeState? {
        guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let currentID = json["currentModeId"] as? String,
              let rawModes = json["availableModes"] as? [[String: Any]]
        else {
            return nil
        }
        return ZZAgentModeState(
            currentID: currentID,
            modes: rawModes.compactMap(ZZAgentSessionMode.parse)
        )
    }

    var currentName: String? {
        modes.first(where: { $0.id == currentID })?.name
    }
}

struct ZZAgentSessionSummary: Identifiable, Equatable, Sendable {
    let sessionID: String
    let cwd: String
    let additionalDirectories: [String]
    let title: String?
    let updatedAt: String?

    var id: String { sessionID }

    var displayTitle: String {
        if let title, !title.isEmpty {
            return title
        }
        return (cwd as NSString).lastPathComponent
    }

    static func parse(_ dict: [String: Any]) -> ZZAgentSessionSummary? {
        guard let sessionID = dict["sessionId"] as? String,
              let cwd = dict["cwd"] as? String
        else {
            return nil
        }
        return ZZAgentSessionSummary(
            sessionID: sessionID,
            cwd: cwd,
            additionalDirectories: dict["additionalDirectories"] as? [String] ?? [],
            title: dict["title"] as? String,
            updatedAt: dict["updatedAt"] as? String
        )
    }

    static func parseList(_ data: Data) -> [ZZAgentSessionSummary]? {
        guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              json["item"] as? String == "sessionsListed",
              let raw = json["sessions"] as? [[String: Any]]
        else {
            return nil
        }
        return raw.compactMap(parse)
    }

    static func parseListFailure(_ data: Data) -> String? {
        guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              json["item"] as? String == "sessionListFailed",
              let message = json["message"] as? String
        else {
            return nil
        }
        return message
    }

    func additionalDirectoriesJSON() -> String {
        guard let data = try? JSONSerialization.data(
            withJSONObject: additionalDirectories,
            options: []
        ) else {
            return "[]"
        }
        return String(decoding: data, as: UTF8.self)
    }
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
    let configOptions: [ZZAgentConfigOption]
    let modeState: ZZAgentModeState?

    func configOption(category: ZZAgentConfigCategory) -> ZZAgentConfigOption? {
        configOptions.first(where: { $0.category == category })
    }
}

struct ZZAgentSessionList: Equatable, Sendable {
    var sessions: [ZZAgentSessionSummary] = []
    var loading = false
    var error: String?
}

enum ZZAgentStatus: UInt32, Equatable, Sendable {
    case idle = 0
    case working = 1
    case needsInput = 2
    case failed = 3
}

enum ZZAgentComposerAction: Equatable, Sendable {
    static let maximumQueuedPrompts: UInt32 = 4

    case send
    case queue
    case stop
    case unavailable

    static func resolve(
        phase: ZZAgentPhase,
        hasPrompt: Bool,
        queuedPrompts: UInt32
    ) -> Self {
        switch phase {
        case .ready:
            return hasPrompt ? .send : .unavailable
        case .running, .awaitingPermission:
            if !hasPrompt {
                return .stop
            }
            return queuedPrompts < maximumQueuedPrompts ? .queue : .unavailable
        case .starting, .failed:
            return .unavailable
        }
    }
}

enum ZZAgentPromptCommand {
    static func arguments(pane: UInt64, text: String) -> [String]? {
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return nil
        }
        return ["-t", "%\(pane)", "--submit", "--", text]
    }
}

struct ZZAgentDrafts: Equatable, Sendable {
    private var values: [UInt64: String] = [:]

    func text(for pane: UInt64) -> String {
        values[pane] ?? ""
    }

    mutating func save(_ text: String, for pane: UInt64) {
        if text.isEmpty {
            values.removeValue(forKey: pane)
        } else {
            values[pane] = text
        }
    }

    mutating func remove(pane: UInt64) {
        values.removeValue(forKey: pane)
    }
}

enum ZZAgentTurnStatus: Equatable, Sendable {
    case working
    case done
    case failed
}

/// What this client knows about a prompt it sent itself: when it left and how
/// the daemon's attention edges settled it. A prompt that only arrived over the
/// journal carries neither.
struct ZZAgentTurnReceipt: Equatable, Sendable {
    let sentAt: Date
    var status: ZZAgentTurnStatus
}

/// Whether the journal has confirmed a bubble yet. A local echo is the one
/// block a replay cannot reproduce, so it survives a replay and is the only
/// bubble a replayed prompt may adopt instead of duplicating.
enum ZZAgentTurnSource: Equatable, Sendable {
    case localEcho
    case stream
}

struct ZZAgentTurn: Identifiable, Equatable, Sendable {
    let id: UInt64
    var text: String
    var source: ZZAgentTurnSource
    var receipt: ZZAgentTurnReceipt?
    /// The journal's `messageId` once the stream confirms the turn. Later
    /// chunks of the same message extend this bubble.
    var messageID: String?
}

/// ACP `ToolKind`. Drives the row icon; unknown wire values fall to `other`
/// the same way the schema's `#[serde(other)]` does.
enum ZZAgentToolKind: String, Equatable, Sendable {
    case read
    case edit
    case delete
    case move
    case search
    case execute
    case think
    case fetch
    case switchMode
    case other

    init(wireValue: String?) {
        switch wireValue {
        case "read": self = .read
        case "edit": self = .edit
        case "delete": self = .delete
        case "move": self = .move
        case "search": self = .search
        case "execute": self = .execute
        case "think": self = .think
        case "fetch": self = .fetch
        case "switch_mode": self = .switchMode
        default: self = .other
        }
    }

    var symbol: String {
        switch self {
        case .read: "doc.text"
        case .edit: "square.and.pencil"
        case .delete: "trash"
        case .move: "arrow.right.doc.on.clipboard"
        case .search: "magnifyingglass"
        case .execute: "terminal"
        case .think: "brain"
        case .fetch: "globe"
        case .switchMode: "arrow.triangle.2.circlepath"
        case .other: "wrench.and.screwdriver"
        }
    }
}

/// One entry of an ACP `locations` array.
struct ZZAgentToolLocation: Equatable, Sendable {
    let path: String
    let line: UInt32?

    static func parse(_ dict: [String: Any]) -> ZZAgentToolLocation? {
        guard let path = dict["path"] as? String, !path.isEmpty else {
            return nil
        }
        return ZZAgentToolLocation(
            path: path,
            line: (dict["line"] as? NSNumber)?.uint32Value
        )
    }

    /// Last path component plus a line when the payload named one. The full
    /// path is absolute and too wide for a split tile.
    var display: String {
        let name = (path as NSString).lastPathComponent
        let base = name.isEmpty ? path : name
        guard let line else {
            return base
        }
        return "\(base):\(line)"
    }
}

/// The reduced tool call a row renders.
struct ZZAgentToolCall: Equatable, Sendable {
    let id: String
    var title: String
    var kind: ZZAgentToolKind
    var status: ZZAgentToolStatus
    /// First location, matching the desktop's choice.
    var location: ZZAgentToolLocation?
    /// Paths named by `content` diff blocks.
    var changedPaths: [String]

    /// The one identifier worth showing beside the title.
    var target: String? {
        if let location {
            return location.display
        }
        guard let first = changedPaths.first else {
            return nil
        }
        let name = (first as NSString).lastPathComponent
        return name.isEmpty ? first : name
    }
}

/// One `tool_call` or `tool_call_update` payload. ACP replaces collections
/// when they are present and leaves them untouched when absent, so a nil
/// array means "unchanged" and an empty one means "cleared".
struct ZZAgentToolCallDelta: Equatable, Sendable {
    let id: String
    var title: String?
    var kind: ZZAgentToolKind?
    var status: ZZAgentToolStatus?
    var locations: [ZZAgentToolLocation]?
    var changedPaths: [String]?

    static func parse(_ dict: [String: Any]) -> ZZAgentToolCallDelta? {
        guard let id = dict["toolCallId"] as? String else {
            return nil
        }
        let locations = (dict["locations"] as? [[String: Any]])
            .map { $0.compactMap(ZZAgentToolLocation.parse) }
        let changedPaths = (dict["content"] as? [[String: Any]]).map { blocks in
            blocks.compactMap { block -> String? in
                guard block["type"] as? String == "diff" else {
                    return nil
                }
                return block["path"] as? String
            }
        }
        let title = dict["title"] as? String
        return ZZAgentToolCallDelta(
            id: id,
            title: (title?.isEmpty == false) ? title : nil,
            kind: dict["kind"].map { ZZAgentToolKind(wireValue: $0 as? String) },
            status: (dict["status"] as? String).map(ZZAgentToolStatus.init(wireValue:)),
            locations: locations,
            changedPaths: changedPaths
        )
    }
}

enum ZZAgentToolStatus: String, Equatable, Sendable {
    case pending
    case running
    case done
    case failed

    init(wireValue: String) {
        switch wireValue {
        case "in_progress": self = .running
        case "completed": self = .done
        case "failed": self = .failed
        default: self = .pending
        }
    }
}

struct ZZAgentThreadBlock: Identifiable, Equatable, Sendable {
    enum Kind: Equatable, Sendable {
        case user(turn: ZZAgentTurn)
        case agentText(messageID: String?, text: String)
        case thought(messageID: String?, text: String)
        case tool(ZZAgentToolCall)
    }

    let id: String
    var kind: Kind

    var userTurn: ZZAgentTurn? {
        if case let .user(turn) = kind {
            return turn
        }
        return nil
    }

    var isLocalEcho: Bool {
        userTurn?.source == .localEcho
    }
}

struct ZZAgentThread: Equatable, Sendable {
    static let maximumBlocks = 300

    enum BatchEffect: Equatable {
        case applied
        case needsReplay
    }

    private(set) var cursor: UInt64 = 0
    private(set) var replayPending = false
    private(set) var blocks: [ZZAgentThreadBlock] = []
    /// Bumped by every mutation that changes something, so a view can detect
    /// change without comparing the whole transcript.
    private(set) var revision: UInt64 = 0
    /// Prompts this client submitted. Lets a view tell "the user just sent
    /// something" apart from "the agent streamed more".
    private(set) var submittedTurns: UInt64 = 0
    private var nextTurnID: UInt64 = 1

    mutating func markReplayPending() {
        replayPending = true
        revision &+= 1
    }

    mutating func appendUserTurn(_ text: String, at date: Date = Date()) {
        appendTurn(
            text: text,
            source: .localEcho,
            receipt: ZZAgentTurnReceipt(sentAt: date, status: .working),
            messageID: nil
        )
        submittedTurns &+= 1
        revision &+= 1
    }

    mutating func settleOldestWorkingTurn(_ status: ZZAgentTurnStatus) {
        for index in blocks.indices {
            if case var .user(turn) = blocks[index].kind, turn.receipt?.status == .working {
                turn.receipt?.status = status
                blocks[index].kind = .user(turn: turn)
                revision &+= 1
                return
            }
        }
    }

    /// Reconnect or replay from the cursor: the journal reproduces everything
    /// it holds, so only a prompt it has not confirmed yet survives.
    mutating func prepareForReplay() {
        dropReplayableBlocks()
        cursor = 0
        replayPending = false
        revision &+= 1
    }

    /// The pane's ACP session changed: none of the previous conversation
    /// belongs to it, this client's own prompts included.
    mutating func resetForNewSession() {
        blocks.removeAll()
        cursor = 0
        replayPending = false
        revision &+= 1
    }

    mutating func applyBatch(firstSeq: UInt64, items: [Data]) -> BatchEffect {
        var gapped = false
        for (offset, data) in items.enumerated() {
            let positional = firstSeq &+ UInt64(offset)
            guard let envelope = ZZAgentStreamEnvelope(data: data) else {
                cursor = max(cursor, positional)
                continue
            }
            let seq = envelope.seq
            if seq > cursor, seq - cursor > 1 {
                if envelope.restoringReset {
                    cursor = seq - 1
                } else {
                    gapped = true
                    break
                }
            }
            if seq <= cursor {
                continue
            }
            cursor = seq
            applyItem(envelope)
        }
        revision &+= 1
        if gapped {
            return .needsReplay
        }
        replayPending = false
        return .applied
    }

    private mutating func applyItem(_ envelope: ZZAgentStreamEnvelope) {
        switch envelope.item {
        case .sessionReset:
            dropReplayableBlocks()
        case let .update(update):
            applyUpdate(update, seq: envelope.seq)
        case .ignored:
            break
        }
    }

    private mutating func applyUpdate(_ update: ZZAgentStreamUpdate, seq: UInt64) {
        switch update.kind {
        case let .agentText(text), let .thought(text):
            guard !text.isEmpty else {
                return
            }
            let isThought: Bool
            if case .thought = update.kind {
                isThought = true
            } else {
                isThought = false
            }
            let targetID = update.messageID.map { "message-\($0)" }
            if let targetID,
               let index = blocks.firstIndex(where: { $0.id == targetID }) {
                appendText(text, toBlockAt: index)
            } else if targetID == nil,
                      let last = blocks.last,
                      last.isStreamText(isThought: isThought) {
                appendText(text, toBlockAt: blocks.count - 1)
            } else {
                let id = targetID ?? "stream-\(isThought ? "thought" : "agent")-\(seq)"
                let kind = isThought
                    ? ZZAgentThreadBlock.Kind.thought(messageID: update.messageID, text: text)
                    : ZZAgentThreadBlock.Kind.agentText(messageID: update.messageID, text: text)
                blocks.append(ZZAgentThreadBlock(id: id, kind: kind))
            }
            trim()
        case let .toolCall(delta), let .toolCallUpdate(delta):
            upsertTool(delta)
        case let .userText(text):
            applyUserText(text, messageID: update.messageID)
        case .ignored:
            break
        }
    }

    /// A prompt the journal replays. It is the same turn the submitting client
    /// echoed locally, so it adopts that bubble with its receipt and takes the
    /// journal's position instead of appending a second copy of the text.
    private mutating func applyUserText(_ text: String, messageID: String?) {
        guard !text.isEmpty else {
            return
        }
        if let index = openStreamTurnIndex(messageID: messageID) {
            appendText(text, toBlockAt: index)
            return
        }
        let echo = blocks.firstIndex { block in
            guard let turn = block.userTurn else {
                return false
            }
            return turn.source == .localEcho && turn.text == text
        }
        guard let echo else {
            appendTurn(text: text, source: .stream, receipt: nil, messageID: messageID)
            return
        }
        var block = blocks.remove(at: echo)
        if case var .user(turn) = block.kind {
            turn.source = .stream
            turn.messageID = messageID
            block.kind = .user(turn: turn)
        }
        blocks.append(block)
    }

    /// The bubble a further chunk of the same user message extends: the one
    /// carrying that `messageId`, or the last stream turn when the payload
    /// named none, matching how id-less agent chunks continue their run.
    private func openStreamTurnIndex(messageID: String?) -> Int? {
        guard let messageID else {
            guard let turn = blocks.last?.userTurn,
                  turn.source == .stream,
                  turn.messageID == nil else {
                return nil
            }
            return blocks.count - 1
        }
        return blocks.lastIndex { block in
            guard let turn = block.userTurn else {
                return false
            }
            return turn.source == .stream && turn.messageID == messageID
        }
    }

    private mutating func appendTurn(
        text: String,
        source: ZZAgentTurnSource,
        receipt: ZZAgentTurnReceipt?,
        messageID: String?
    ) {
        let turn = ZZAgentTurn(
            id: nextTurnID,
            text: text,
            source: source,
            receipt: receipt,
            messageID: messageID
        )
        nextTurnID &+= 1
        blocks.append(ZZAgentThreadBlock(id: "user-\(turn.id)", kind: .user(turn: turn)))
        trim()
    }

    private mutating func dropReplayableBlocks() {
        blocks.removeAll { !$0.isLocalEcho }
    }

    private mutating func appendText(_ text: String, toBlockAt index: Int) {
        switch blocks[index].kind {
        case let .agentText(messageID, existing):
            blocks[index].kind = .agentText(messageID: messageID, text: existing + text)
        case let .thought(messageID, existing):
            blocks[index].kind = .thought(messageID: messageID, text: existing + text)
        case var .user(turn):
            turn.text += text
            blocks[index].kind = .user(turn: turn)
        case .tool:
            break
        }
    }

    private mutating func upsertTool(_ delta: ZZAgentToolCallDelta) {
        let blockID = "tool-\(delta.id)"
        if let index = blocks.firstIndex(where: { $0.id == blockID }),
           case var .tool(call) = blocks[index].kind {
            call.title = delta.title ?? call.title
            call.kind = delta.kind ?? call.kind
            call.status = delta.status ?? call.status
            if let locations = delta.locations {
                call.location = locations.first
            }
            if let changedPaths = delta.changedPaths {
                call.changedPaths = changedPaths
            }
            blocks[index].kind = .tool(call)
            return
        }
        blocks.append(
            ZZAgentThreadBlock(
                id: blockID,
                kind: .tool(
                    ZZAgentToolCall(
                        id: delta.id,
                        title: delta.title ?? "Tool",
                        kind: delta.kind ?? .other,
                        status: delta.status ?? .pending,
                        location: delta.locations?.first,
                        changedPaths: delta.changedPaths ?? []
                    )
                )
            )
        )
        trim()
    }

    private mutating func trim() {
        while blocks.count > Self.maximumBlocks {
            if let index = blocks.firstIndex(where: { !$0.isLocalEcho }) {
                blocks.remove(at: index)
            } else {
                blocks.removeFirst()
            }
        }
    }
}

private extension ZZAgentThreadBlock {
    func isStreamText(isThought: Bool) -> Bool {
        switch kind {
        case let .agentText(messageID, _):
            return !isThought && messageID == nil
        case let .thought(messageID, _):
            return isThought && messageID == nil
        case .user, .tool:
            return false
        }
    }
}

/// One pane's transcript, published on its own object so a streamed batch
/// invalidates that pane's thread view instead of every view observing
/// `ZZStore`. Mirrors `TerminalFrameSlot`.
@MainActor
final class ZZAgentThreadSlot: ObservableObject {
    @Published private(set) var thread: ZZAgentThread

    init(thread: ZZAgentThread = ZZAgentThread()) {
        self.thread = thread
    }

    @discardableResult
    func mutate<T>(_ body: (inout ZZAgentThread) -> T) -> T {
        var next = thread
        let result = body(&next)
        if next.revision != thread.revision {
            thread = next
        }
        return result
    }
}

/// Alignment of one markdown table column.
enum ZZMarkdownAlignment: Equatable, Sendable {
    case leading
    case center
    case trailing
    case unspecified
}

struct ZZMarkdownTable: Equatable, Sendable {
    let alignments: [ZZMarkdownAlignment]
    let head: [String]
    let rows: [[String]]

    var columnCount: Int {
        max(head.count, rows.map(\.count).max() ?? 0)
    }

    func alignment(_ column: Int) -> ZZMarkdownAlignment {
        alignments.indices.contains(column) ? alignments[column] : .unspecified
    }

    func cell(row: [String], column: Int) -> String {
        row.indices.contains(column) ? row[column] : ""
    }
}

struct ZZMarkdownListItem: Equatable, Sendable {
    /// `nil` when the item is not a task-list item.
    let checked: Bool?
    let blocks: [ZZMarkdownBlock]
}

struct ZZMarkdownList: Equatable, Sendable {
    let ordered: Bool
    let start: Int
    let items: [ZZMarkdownListItem]
}

/// One block of agent output. Leaf text stays as markdown source so inline
/// styling (emphasis, code spans, links) can be applied at draw time.
indirect enum ZZMarkdownBlock: Equatable, Sendable {
    case paragraph(String)
    case heading(level: Int, text: String)
    case code(language: String?, code: String)
    case quote([ZZMarkdownBlock])
    case list(ZZMarkdownList)
    case table(ZZMarkdownTable)
    case thematicBreak
}

struct ZZMarkdownNode: Identifiable, Equatable, Sendable {
    let id: Int
    let block: ZZMarkdownBlock
}

/// Agent output parsed into blocks.
///
/// Block structure comes from `swift-markdown` (cmark-gfm), which is what the
/// desktop does with the `markdown` crate: take the parser's tree, render it
/// yourself. Hand-rolled line scanning cannot carry CommonMark — fence lengths,
/// nesting, lazy continuation, GFM tables — and drifts out of sync on exactly
/// the documents agents produce.
///
/// Inline syntax stays with `AttributedString`, which handles it natively.
enum ZZAgentMarkdown: Sendable {
    static func blocks(_ text: String) -> [ZZMarkdownNode] {
        let document = Document(parsing: text)
        return convert(Array(document.children))
            .enumerated()
            .map { ZZMarkdownNode(id: $0.offset, block: $0.element) }
    }

    private static func convert(_ markups: [Markup]) -> [ZZMarkdownBlock] {
        markups.compactMap(convert)
    }

    private static func convert(_ markup: Markup) -> ZZMarkdownBlock? {
        switch markup {
        case let heading as Heading:
            return .heading(level: heading.level, text: inlineSource(heading))
        case let code as CodeBlock:
            let language = code.language?.trimmingCharacters(in: .whitespaces)
            return .code(
                language: (language?.isEmpty == false) ? language : nil,
                // cmark keeps the block's trailing newline; it would render as
                // a blank final line.
                code: String(code.code.reversed().drop { $0 == "\n" }.reversed())
            )
        case let quote as BlockQuote:
            return .quote(convert(Array(quote.children)))
        case let list as UnorderedList:
            return .list(
                ZZMarkdownList(ordered: false, start: 1, items: items(in: list))
            )
        case let list as OrderedList:
            return .list(
                ZZMarkdownList(
                    ordered: true,
                    start: Int(list.startIndex),
                    items: items(in: list)
                )
            )
        case let table as Table:
            return .table(convert(table))
        case is ThematicBreak:
            return .thematicBreak
        case let paragraph as Paragraph:
            return .paragraph(inlineSource(paragraph))
        default:
            // HTML blocks, block directives, and anything else the parser knows
            // but the pane has no representation for: show the source rather
            // than dropping it.
            let source = markup.format()
                .trimmingCharacters(in: .whitespacesAndNewlines)
            return source.isEmpty ? nil : .paragraph(source)
        }
    }

    private static func items(in list: Markup) -> [ZZMarkdownListItem] {
        list.children.compactMap { $0 as? ListItem }.map { item in
            ZZMarkdownListItem(
                checked: item.checkbox.map { $0 == .checked },
                blocks: convert(Array(item.children))
            )
        }
    }

    private static func convert(_ table: Table) -> ZZMarkdownTable {
        ZZMarkdownTable(
            alignments: table.columnAlignments.map { alignment in
                switch alignment {
                case .left: .leading
                case .center: .center
                case .right: .trailing
                case nil: .unspecified
                }
            },
            head: table.head.children.map(inlineSource),
            rows: table.body.children.map { row in
                row.children.map(inlineSource)
            }
        )
    }

    /// The markdown source of a node's inline children, so heading hashes and
    /// list markers do not survive into the rendered text.
    ///
    /// `format()` is ancestor-aware: formatting a paragraph where it sits
    /// carries its block-quote or list prefix into the text. Formatting a
    /// detached copy of just the inline children yields the leaf source.
    private static func inlineSource(_ markup: Markup) -> String {
        let inlines = markup.children.compactMap { $0 as? InlineMarkup }
        guard !inlines.isEmpty else {
            return ""
        }
        return Paragraph(inlines)
            .format()
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }

    static func inline(_ text: String) -> AttributedString {
        (try? AttributedString(
            markdown: text,
            options: AttributedString.MarkdownParsingOptions(
                interpretedSyntax: .inlineOnlyPreservingWhitespace
            )
        )) ?? AttributedString(text)
    }
}

struct ZZAgentStreamUpdate {
    enum Kind {
        case agentText(String)
        case thought(String)
        case toolCall(ZZAgentToolCallDelta)
        case toolCallUpdate(ZZAgentToolCallDelta)
        case userText(String)
        case ignored
    }

    let kind: Kind
    let messageID: String?

    static func parse(_ dict: [String: Any]) -> ZZAgentStreamUpdate {
        guard let tag = dict["sessionUpdate"] as? String else {
            return ZZAgentStreamUpdate(kind: .ignored, messageID: nil)
        }
        let messageID = dict["messageId"] as? String
        switch tag {
        case "agent_message_chunk":
            return ZZAgentStreamUpdate(
                kind: .agentText(chunkText(dict)),
                messageID: messageID
            )
        case "agent_thought_chunk":
            return ZZAgentStreamUpdate(
                kind: .thought(chunkText(dict)),
                messageID: messageID
            )
        case "tool_call":
            guard let delta = ZZAgentToolCallDelta.parse(dict) else {
                return ZZAgentStreamUpdate(kind: .ignored, messageID: nil)
            }
            return ZZAgentStreamUpdate(kind: .toolCall(delta), messageID: nil)
        case "tool_call_update":
            guard let delta = ZZAgentToolCallDelta.parse(dict) else {
                return ZZAgentStreamUpdate(kind: .ignored, messageID: nil)
            }
            return ZZAgentStreamUpdate(kind: .toolCallUpdate(delta), messageID: nil)
        case "user_message_chunk":
            return ZZAgentStreamUpdate(kind: .userText(chunkText(dict)), messageID: messageID)
        default:
            return ZZAgentStreamUpdate(kind: .ignored, messageID: nil)
        }
    }

    /// `content` is one ACP `ContentBlock`, not an array. Non-text blocks
    /// degrade to the same placeholders the desktop uses so they leave a mark
    /// rather than vanishing.
    private static func chunkText(_ dict: [String: Any]) -> String {
        guard let content = dict["content"] as? [String: Any] else {
            return ""
        }
        switch content["type"] as? String {
        case "text":
            return content["text"] as? String ?? ""
        case "image":
            return "*[Image: \(content["mimeType"] as? String ?? "image")]*"
        case "audio":
            return "*[Audio: \(content["mimeType"] as? String ?? "audio")]*"
        case "resource_link":
            let uri = content["uri"] as? String ?? ""
            let label = content["title"] as? String
                ?? content["name"] as? String
                ?? uri
            return "[\(label)](\(uri))"
        case "resource":
            if let resource = content["resource"] as? [String: Any],
               let text = resource["text"] as? String {
                return text
            }
            return "*[Embedded resource]*"
        default:
            return ""
        }
    }
}

struct ZZAgentStreamEnvelope {
    enum Item {
        case sessionReset(restoring: Bool)
        case update(ZZAgentStreamUpdate)
        case ignored
    }

    let seq: UInt64
    let item: Item

    var restoringReset: Bool {
        if case let .sessionReset(restoring) = item {
            return restoring
        }
        return false
    }

    init?(data: Data) {
        guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let seq = (json["seq"] as? NSNumber)?.uint64Value,
              seq > 0,
              let tag = json["item"] as? String
        else {
            return nil
        }
        self.seq = seq
        switch tag {
        case "sessionReset":
            self.item = .sessionReset(restoring: json["restoring"] as? Bool ?? false)
        case "update":
            guard let update = json["update"] as? [String: Any] else {
                return nil
            }
            self.item = .update(ZZAgentStreamUpdate.parse(update))
        default:
            self.item = .ignored
        }
    }
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

    func snapshotTarget(
        selectedPane: UInt64?,
        activePane: UInt64?,
        replacingPane: UInt64? = nil,
        navigationPending: Bool
    ) -> UInt64? {
        guard !navigationPending,
              let selectedPane,
              owner.owns(selectedPane) || replacingPane == selectedPane,
              let activePane,
              activePane != selectedPane else {
            return nil
        }
        return activePane
    }

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

/// One daemon-published `prefix` table binding, refreshed from the
/// `zz_prefix_snapshot_*` FFI family after `ZZ_EVENT_PREFIX_ARMED` or
/// `ZZ_EVENT_KEY_TABLES_CHANGED`.
struct ZZPrefixBinding: Identifiable, Equatable, Sendable {
    /// The key in tmux-grammar spelling (`%`, `C-o`, `M-1`).
    let key: String
    /// First bound command as one line (`split-window -h`).
    let summary: String
    /// The `bind -N` annotation, when one was given.
    let note: String
    /// Whether the binding repeats without leaving its table (`bind -r`).
    let repeats: Bool

    var id: String { key }

    /// The key in desktop hint spelling (`cmd-`, `ctrl-`, `alt-`, `shift-`
    /// prefixes plus a lowercase base), mirroring `gpui_source` in
    /// `crates/zz/src/keymap.rs` so iOS hints read the same as desktop.
    var displayKey: String { ZZKeySpelling.display(key) }
}

/// Desktop key-spelling helpers shared by the prefix list, the hardware
/// keyboard discoverability titles, and the key-list sheet.
enum ZZKeySpelling: Sendable {
    /// Named tmux bases and their desktop hint forms, mirroring the
    /// `KEY_NAMES` table in `crates/zz/src/keymap.rs`.
    private static let namedBases: [String: String] = [
        "Enter": "enter",
        "Escape": "escape",
        "Tab": "tab",
        "BSpace": "backspace",
        "Up": "up",
        "Down": "down",
        "Left": "left",
        "Right": "right",
        "Home": "home",
        "End": "end",
        "PPage": "pageup",
        "NPage": "pagedown",
        "DC": "delete",
        "IC": "insert",
        "F1": "f1",
        "F2": "f2",
        "F3": "f3",
        "F4": "f4",
        "F5": "f5",
        "F6": "f6",
        "F7": "f7",
        "F8": "f8",
        "F9": "f9",
        "F10": "f10",
        "F11": "f11",
        "F12": "f12",
    ]

    /// Convert one tmux-grammar chord (`D-M-Right`, `C- `, `G`) to desktop
    /// hint spelling (`cmd-alt-right`, `ctrl-space`, `shift-g`).
    static func display(_ spelling: String) -> String {
        var rest = spelling
        var command = false
        var control = false
        var alt = false
        var shift = false
        var consumed = true
        while consumed {
            consumed = false
            if rest.hasPrefix("D-") {
                rest = String(rest.dropFirst(2))
                command = true
                consumed = true
            }
            if rest.hasPrefix("C-") {
                rest = String(rest.dropFirst(2))
                control = true
                consumed = true
            }
            if rest.hasPrefix("M-") {
                rest = String(rest.dropFirst(2))
                alt = true
                consumed = true
            }
            if rest.hasPrefix("S-") {
                rest = String(rest.dropFirst(2))
                shift = true
                consumed = true
            }
        }
        let base: String
        if rest == " " {
            base = "space"
        } else if let named = namedBases[rest] {
            base = named
        } else {
            base = rest
        }
        if base.count == 1,
           let scalar = base.unicodeScalars.first,
           scalar.value >= 65, scalar.value <= 90 {
            shift = true
        }
        var result = ""
        if command { result += "cmd-" }
        if control { result += "ctrl-" }
        if alt { result += "alt-" }
        if shift { result += "shift-" }
        result += base.lowercased()
        return result
    }
}

/// Minimal command-line splitting for the command-prompt sheet: the first
/// whitespace-separated token is the daemon command name, the rest are its
/// arguments. No shell quoting; multi-word arguments need the desktop client.
enum ZZCommandLine: Sendable {
    static func split(_ line: String) -> (name: String, args: [String])? {
        let parts = line.split(whereSeparator: \.isWhitespace).map(String.init)
        guard let name = parts.first else {
            return nil
        }
        return (name, Array(parts.dropFirst()))
    }
}

/// The executed commands whose reply the store still wants. The daemon answers
/// every command and the FFI keeps only the newest replies, so an id that is
/// not tracked here is dropped instead of matched against a stale intent.
struct ZZCommandRequests: Equatable, Sendable {
    enum Purpose: Equatable, Sendable {
        case lastOutput(pane: UInt64)
    }

    static let capacity = 8

    private var order: [UInt64] = []
    private var purposes: [UInt64: Purpose] = [:]

    var count: Int {
        order.count
    }

    /// `zz_client_execute_request` answers zero when the send failed, and that
    /// request will never produce a reply.
    mutating func register(_ request: UInt64, as purpose: Purpose) {
        guard request != 0 else {
            return
        }
        if purposes.updateValue(purpose, forKey: request) == nil {
            order.append(request)
        }
        while order.count > Self.capacity {
            purposes.removeValue(forKey: order.removeFirst())
        }
    }

    mutating func take(_ request: UInt64) -> Purpose? {
        guard let purpose = purposes.removeValue(forKey: request) else {
            return nil
        }
        order.removeAll { $0 == request }
        return purpose
    }
}

/// `show-last-output`: the daemon replays the last OSC 133 command block from a
/// pane. It needs shell integration, so the rejection is the common answer and
/// carries the text that explains it.
enum ZZLastOutput {
    enum Result: Equatable, Sendable {
        case copy(String)
        case failure(String)
    }

    static func arguments(pane: UInt64) -> [String] {
        ["-t", "%\(pane)"]
    }

    static func result(ok: Bool, output: String, error: String) -> Result {
        guard ok else {
            let rendered = error.trimmingCharacters(in: .whitespacesAndNewlines)
            return .failure(rendered.isEmpty ? "zz couldn’t read that pane’s last output." : rendered)
        }
        let text = output.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else {
            return .failure("That pane hasn’t finished a command yet.")
        }
        return .copy(text)
    }
}

/// A short-lived confirmation for an action that finishes without changing the
/// visible workspace. Failures stay up longer because they carry the reason.
struct ZZActionNotice: Identifiable, Equatable, Sendable {
    enum Tone: Equatable, Sendable {
        case success
        case failure

        var symbol: String {
            switch self {
            case .success: "checkmark.circle.fill"
            case .failure: "exclamationmark.triangle.fill"
            }
        }

        var seconds: TimeInterval {
            switch self {
            case .success: 2.5
            case .failure: 6
            }
        }
    }

    let id: UInt64
    let tone: Tone
    let message: String
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
