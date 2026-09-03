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
    static func delay(for attempt: Int) -> Int {
        1 << min(max(attempt - 1, 0), 4)
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

struct ZZAgentTurn: Identifiable, Equatable, Sendable {
    let id: UInt64
    let text: String
    let sentAt: Date
    var status: ZZAgentTurnStatus
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
        case tool(id: String, title: String, status: ZZAgentToolStatus)
    }

    let id: String
    var kind: Kind

    var isUserTurn: Bool {
        if case .user = kind {
            return true
        }
        return false
    }
}

struct ZZAgentThread: Equatable, Sendable {
    static let maximumBlocks = 300

    enum BatchEffect: Equatable {
        case applied
        case needsReplay
    }

    var cursor: UInt64 = 0
    var replayPending = false
    var blocks: [ZZAgentThreadBlock] = []
    private var nextLocalID: UInt64 = 1

    mutating func appendUserTurn(_ text: String, at date: Date = Date()) {
        let turn = ZZAgentTurn(id: nextLocalID, text: text, sentAt: date, status: .working)
        nextLocalID += 1
        blocks.append(ZZAgentThreadBlock(id: "user-\(turn.id)", kind: .user(turn: turn)))
        trim()
    }

    mutating func settleOldestWorkingTurn(_ status: ZZAgentTurnStatus) {
        for index in blocks.indices {
            if case var .user(turn) = blocks[index].kind, turn.status == .working {
                turn.status = status
                blocks[index].kind = .user(turn: turn)
                return
            }
        }
    }

    mutating func resetStream() {
        blocks.removeAll { !$0.isUserTurn }
        cursor = 0
        replayPending = false
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
        if gapped {
            return .needsReplay
        }
        replayPending = false
        return .applied
    }

    private mutating func applyItem(_ envelope: ZZAgentStreamEnvelope) {
        switch envelope.item {
        case .sessionReset:
            blocks.removeAll { !$0.isUserTurn }
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
                appendText(text, toTextBlockAt: index)
            } else if targetID == nil,
                      let last = blocks.last,
                      last.isStreamText(isThought: isThought) {
                appendText(text, toTextBlockAt: blocks.count - 1)
            } else {
                let id = targetID ?? "stream-\(isThought ? "thought" : "agent")-\(seq)"
                let kind = isThought
                    ? ZZAgentThreadBlock.Kind.thought(messageID: update.messageID, text: text)
                    : ZZAgentThreadBlock.Kind.agentText(messageID: update.messageID, text: text)
                blocks.append(ZZAgentThreadBlock(id: id, kind: kind))
            }
            trim()
        case let .toolCall(id, title, status):
            upsertTool(id: id, title: title, status: status)
        case let .toolCallUpdate(id, title, status):
            upsertTool(id: id, title: title, status: status)
        case .userText, .ignored:
            break
        }
    }

    private mutating func appendText(_ text: String, toTextBlockAt index: Int) {
        switch blocks[index].kind {
        case let .agentText(messageID, existing):
            blocks[index].kind = .agentText(messageID: messageID, text: existing + text)
        case let .thought(messageID, existing):
            blocks[index].kind = .thought(messageID: messageID, text: existing + text)
        case .user, .tool:
            break
        }
    }

    private mutating func upsertTool(id: String, title: String?, status: ZZAgentToolStatus?) {
        let blockID = "tool-\(id)"
        if let index = blocks.firstIndex(where: { $0.id == blockID }) {
            if case let .tool(_, existingTitle, existingStatus) = blocks[index].kind {
                let resolvedTitle = (title?.isEmpty == false) ? title! : existingTitle
                blocks[index].kind = .tool(
                    id: id,
                    title: resolvedTitle,
                    status: status ?? existingStatus
                )
            }
            return
        }
        let resolvedTitle = (title?.isEmpty == false) ? title! : "Tool"
        blocks.append(
            ZZAgentThreadBlock(
                id: blockID,
                kind: .tool(id: id, title: resolvedTitle, status: status ?? .pending)
            )
        )
        trim()
    }

    private mutating func trim() {
        while blocks.count > Self.maximumBlocks {
            if let index = blocks.firstIndex(where: { !$0.isUserTurn }) {
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

struct ZZAgentStreamUpdate {
    enum Kind {
        case agentText(String)
        case thought(String)
        case toolCall(id: String, title: String?, status: ZZAgentToolStatus?)
        case toolCallUpdate(id: String, title: String?, status: ZZAgentToolStatus?)
        case userText
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
            guard let id = dict["toolCallId"] as? String else {
                return ZZAgentStreamUpdate(kind: .ignored, messageID: nil)
            }
            return ZZAgentStreamUpdate(
                kind: .toolCall(
                    id: id,
                    title: dict["title"] as? String,
                    status: (dict["status"] as? String).map(ZZAgentToolStatus.init(wireValue:))
                ),
                messageID: nil
            )
        case "tool_call_update":
            guard let id = dict["toolCallId"] as? String else {
                return ZZAgentStreamUpdate(kind: .ignored, messageID: nil)
            }
            return ZZAgentStreamUpdate(
                kind: .toolCallUpdate(
                    id: id,
                    title: dict["title"] as? String,
                    status: (dict["status"] as? String).map(ZZAgentToolStatus.init(wireValue:))
                ),
                messageID: nil
            )
        case "user_message_chunk":
            return ZZAgentStreamUpdate(kind: .userText, messageID: messageID)
        default:
            return ZZAgentStreamUpdate(kind: .ignored, messageID: nil)
        }
    }

    private static func chunkText(_ dict: [String: Any]) -> String {
        guard let content = dict["content"] as? [String: Any],
              content["type"] as? String == "text",
              let text = content["text"] as? String
        else {
            return ""
        }
        return text
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

    /// Arguments that open the daemon overlays without any daemon changes:
    /// the daemon publishes the resulting state and every client renders it.
    static let chooseBufferArgs: [String] = []
    static let displayPanesArgs: [String] = ["-d", "0"]
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
