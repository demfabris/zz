import AppKit
import CZZClient
import Observation
import UniformTypeIdentifiers

public struct NativeAgentImage: Codable, Equatable {
    public var format: String
    public var data: Data
    public var image: NSImage? { NSImage(data: data) }
}

public struct NativeAgentPayload: Decodable {
    public let kind: String
    public let text: String?
    public let diff: Diff?
    public struct Diff: Decodable {
        public let path: String
        public let old: String?
        public let new: String
    }
    private enum CodingKeys: CodingKey { case kind, value }
    public init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        kind = try values.decode(String.self, forKey: .kind)
        text = try? values.decode(String.self, forKey: .value)
        diff = try? values.decode(Diff.self, forKey: .value)
    }
}

public struct NativeAgentEntry: Decodable, Identifiable {
    public let id: UInt64
    public let type: String
    public let markdown: String?
    public let images: [NativeAgentImage]?
    public let label: String?
    public let kind: String?
    public let status: String?
    public let location: String?
    public let input: NativeAgentPayload?
    public let output: [NativeAgentPayload]?
    public let default_expanded: Bool?
}

public struct NativeAgentOption: Decodable, Identifiable {
    public let id: String
    public let name: String
    public let description: String?
    public let category: String
    public let current_value: String
    public let choices: [Choice]
    public struct Choice: Decodable {
        public let value: String
        public let name: String
        public let description: String?
    }
}

public struct NativeAgentNamedValue: Decodable, Identifiable {
    public let id: String
    public let name: String
    public let description: String?
}

public struct NativeAgentPermission: Decodable, Identifiable {
    public var id: UInt64 { request_id }
    public let request_id: UInt64
    public let title: String
    public let options: [Option]
    public struct Option: Decodable, Identifiable {
        public let id: String
        public let name: String
        public let kind: String
    }
}

public struct NativeAgentSession: Decodable, Identifiable {
    public var id: String { sessionId }
    public let sessionId: String
    public let cwd: String
    public let additionalDirectories: [String]
    public let title: String?
    public let updatedAt: String?
}

public struct NativeAgentSnapshot: Decodable {
    public let epoch: UInt64
    public let revision: UInt64
    public let reset: Bool
    public let entry_count: Int
    let entries: [Change]
    struct Change: Decodable { let index: Int; let revision: UInt64; let entry: NativeAgentEntry }
    public let provider: String
    public let cwd: String
    public let agent_name: String?
    public let phase: String
    public let session_id: String?
    public let title: String?
    public let capabilities: Capabilities
    public let options: [NativeAgentOption]
    public let modes: [NativeAgentNamedValue]
    public let mode: String?
    public let auth_methods: [NativeAgentNamedValue]
    public let usage: [UInt64]?
    public let git: Git?
    public let queued_prompts: UInt32
    public let permissions: [NativeAgentPermission]
    public let history: History
    public let error: String?
    public let busy: Bool
    public let cancelling: Bool
    public struct Capabilities: Decodable {
        public let load: Bool
        public let list: Bool
        public let delete: Bool
        public let images: Bool
        public let additionalDirectories: Bool
    }
    public struct Git: Decodable {
        public let branch: String?
        public let changed_files: UInt32
        public let additions: UInt32
        public let deletions: UInt32
    }
    public struct History: Decodable {
        public let sessions: [NativeAgentSession]
        public let loading: Bool
        public let error: String?
        public let next_cursor: String?
    }
}

public struct NativeAgentCompletion: Decodable, Identifiable {
    public var id: String { command.name }
    public let command: Command
    public let start: Int
    public let end: Int
    public let insertion: String
    public struct Command: Decodable {
        public let name: String
        public let description: String
        public let input_hint: String?
    }
}

private final class AgentHandle {
    let pointer: OpaquePointer
    init(_ pointer: OpaquePointer) { self.pointer = pointer }
    deinit { zz_agent_model_free(pointer) }
}

@Observable @MainActor
public final class NativeAgentPane {
    public let id: UInt64
    public var draft = ""
    public var focusRequest = 0
    public var attachments: [NativeAgentImage] = []
    public private(set) var snapshot: NativeAgentSnapshot?
    public private(set) var entries: [NativeAgentEntry] = []
    public private(set) var localError: String?
    public private(set) var completions: [NativeAgentCompletion] = []
    public private(set) var commandHint: String?
    public private(set) var selectedCompletion = 0
    public private(set) var selectedPermission = 0
    public var running: Bool { snapshot?.phase == "running" || snapshot?.phase == "permission" }
    public var canSend: Bool {
        client?.connected == true && (snapshot?.phase == "ready" || running) && snapshot?.busy != true
            && hasPromptContent
    }
    private var hasPromptContent: Bool {
        !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !attachments.isEmpty
    }
    @ObservationIgnored private var storage: AgentHandle?
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var restoredIDs = Set<UInt64>()
    @ObservationIgnored private var provider: String?
    @ObservationIgnored weak var client: NativeClient?
    var handle: OpaquePointer? { storage?.pointer }

    init(id: UInt64) { self.id = id }

    func connect(_ client: NativeClient) {
        self.client = client
        guard let connection = client.handle else { return }
        let nextProvider = client.sessions.flatMap(\.windows).flatMap(\.panes).first(where: { $0.id == id })?.agent?
            .provider
        if generation != client.connectionGeneration || provider != nextProvider {
            storage = nil
            snapshot = nil
            entries = []
            generation = client.connectionGeneration
            provider = nextProvider
        }
        if storage == nil, let pointer = zz_agent_model_new(connection, id) {
            storage = AgentHandle(pointer)
            _ = zz_agent_model_replay(pointer, connection)
        }
        refresh()
    }

    func refresh() {
        guard let handle else { return }
        if let state = client?.handle.flatMap({ zz_client_agent_state_acquire($0, id) }) {
            zz_agent_model_sync(handle, state)
            zz_agent_state_release(state)
        }
        if let connection = client?.handle { zz_agent_model_reconcile_preferences(handle, connection) }
        if let next: NativeAgentSnapshot = Self.decode(zz_agent_model_snapshot(handle, snapshot?.revision ?? 0)) {
            if next.reset || next.epoch != snapshot?.epoch { entries = [] }
            for change in next.entries {
                if change.index < entries.count {
                    entries[change.index] = change.entry
                } else if change.index == entries.count {
                    entries.append(change.entry)
                }
            }
            if entries.count > next.entry_count { entries.removeLast(entries.count - next.entry_count) }
            snapshot = next
        }
        struct Restored: Decodable { let reclaim_id: UInt64; let text: String; let images: [NativeAgentImage] }
        if let restored: [Restored] = Self.decode(zz_agent_model_restored(handle)) {
            for prompt in restored where prompt.reclaim_id == 0 || !restoredIDs.contains(prompt.reclaim_id) {
                if !prompt.text.isEmpty { draft += (draft.isEmpty ? "" : "\n") + prompt.text }
                attachments.append(contentsOf: prompt.images)
                if prompt.reclaim_id != 0 { restoredIDs.insert(prompt.reclaim_id) }
            }
            if let connection = client?.handle { zz_agent_model_acknowledge_restored(handle, connection) }
        }
    }

    @discardableResult public func action(_ name: String, _ values: [String: Any] = [:]) -> Bool {
        guard let handle, let connection = client?.handle else { return false }
        var value = values
        value["action"] = name
        guard let data = try? JSONSerialization.data(withJSONObject: value),
            let json = String(data: data, encoding: .utf8)
        else { return false }
        let ok = json.withCString { zz_agent_model_action(handle, connection, $0) }
        refresh()
        return ok
    }

    public func poll() { refresh() }

    public func send() {
        let images = attachments.map { ["format": $0.format, "data": $0.data.base64EncodedString()] }
        if action("prompt", ["text": draft, "images": images]) {
            draft = ""
            attachments = []
            completions = []
        }
    }

    public func attachImages(_ urls: [URL]) {
        for url in urls {
            do {
                let data = try Data(contentsOf: url, options: .mappedIfSafe)
                guard NSImage(data: data) != nil,
                    let mime = UTType(filenameExtension: url.pathExtension)?.preferredMIMEType
                else {
                    localError = "Choose an image file."
                    continue
                }
                attachments.append(NativeAgentImage(format: mime, data: data))
            } catch { localError = error.localizedDescription }
        }
    }

    public func pasteImages(from pasteboard: NSPasteboard = .general) -> Bool {
        guard snapshot?.capabilities.images == true else { return false }
        let types = [UTType.png, .jpeg, .gif, .webP, .tiff]
        var images: [NativeAgentImage] = []
        for item in pasteboard.pasteboardItems ?? [] {
            for type in types {
                guard let data = item.data(forType: NSPasteboard.PasteboardType(type.identifier)),
                    NSImage(data: data) != nil
                else { continue }
                if type == .tiff {
                    if let png = NSBitmapImageRep(data: data)?.representation(using: .png, properties: [:]) {
                        images.append(NativeAgentImage(format: "image/png", data: png))
                    }
                } else if let mime = type.preferredMIMEType {
                    images.append(NativeAgentImage(format: mime, data: data))
                }
                break
            }
        }
        guard !images.isEmpty else { return false }
        attachments.append(contentsOf: images)
        return true
    }

    public func clearError() { localError = nil; action("clear-error") }

    public func updateCompletions() {
        guard let handle else { return }
        struct Result: Decodable { let entries: [NativeAgentCompletion]; let hint: String? }
        let editor = NSApp?.keyWindow?.firstResponder as? NSTextView
        let selection = editor?.string == draft ? editor?.selectedRange().location : nil
        let cursor =
            selection.flatMap { Range(NSRange(location: 0, length: $0), in: draft) }.map { draft[$0].utf8.count }
            ?? draft.utf8.count
        let value: Result? = draft.withCString { Self.decode(zz_agent_model_completions(handle, $0, cursor)) }
        completions = value?.entries ?? []
        selectedCompletion = min(selectedCompletion, max(0, completions.count - 1))
        commandHint = value?.hint
    }

    public func handleKey(_ event: NSEvent, editingText: Bool) -> Bool {
        let modifiers = event.modifierFlags.intersection([.command, .control, .option, .shift])
        if modifiers == .command && (event.keyCode == 36 || event.keyCode == 76) {
            if !event.isARepeat {
                if canSend {
                    send()
                } else if client?.connected == true && running && !hasPromptContent {
                    action("cancel")
                }
            }
            return true
        }
        guard modifiers.isEmpty else { return false }
        let composerEngaged = editingText && !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        if !composerEngaged, let permission = snapshot?.permissions.first {
            if let text = event.charactersIgnoringModifiers, let index = Int(text),
                (1...permission.options.count).contains(index)
            {
                action(
                    "permission", ["request_id": permission.request_id, "option_id": permission.options[index - 1].id])
                return true
            }
            switch event.keyCode {
            case 125: selectedPermission = min(permission.options.count - 1, selectedPermission + 1); return true
            case 126: selectedPermission = max(0, selectedPermission - 1); return true
            case 36 where !permission.options.isEmpty:
                let index = min(selectedPermission, permission.options.count - 1)
                action("permission", ["request_id": permission.request_id, "option_id": permission.options[index].id]);
                return true
            case 53: action("permission", ["request_id": permission.request_id, "option_id": NSNull()]); return true
            default: break
            }
        }
        if editingText && !completions.isEmpty {
            switch event.keyCode {
            case 125: selectedCompletion = min(completions.count - 1, selectedCompletion + 1); return true
            case 126: selectedCompletion = max(0, selectedCompletion - 1); return true
            case 36, 48: complete(completions[selectedCompletion]); return true
            case 53: completions = []; return true
            default: break
            }
        }
        return false
    }

    public func complete(_ completion: NativeAgentCompletion) {
        let bytes = Array(draft.utf8)
        guard completion.start <= completion.end, completion.end <= bytes.count else { return }
        draft =
            String(decoding: bytes[..<completion.start], as: UTF8.self) + completion.insertion
            + String(decoding: bytes[completion.end...], as: UTF8.self)
        let insertionEnd =
            String(decoding: bytes[..<completion.start], as: UTF8.self).utf16.count + completion.insertion.utf16.count
        DispatchQueue.main.async { [weak self] in
            guard let self, let editor = NSApp?.keyWindow?.firstResponder as? NSTextView, editor.string == self.draft
            else { return }
            editor.setSelectedRange(NSRange(location: insertionEnd, length: 0))
            self.updateCompletions()
        }
        updateCompletions()
    }

    func guiCommand(_ value: [String: Any]) {
        guard let handle, let client, let connection = client.handle,
            let request = (value["request_id"] as? NSNumber)?.uint64Value,
            let command = value["command"], let data = try? JSONSerialization.data(withJSONObject: command),
            let json = String(data: data, encoding: .utf8)
        else { return }
        struct Append: Decodable { let text: String; let request_id: UInt64 }
        let append: Append? = json.withCString {
            Self.decode(zz_agent_model_gui_command(handle, connection, request, $0))
        }
        if let append {
            draft += append.text
            client.guiResponse(append.request_id, ok: true, text: "")
        }
        refresh()
    }

    static func decode<T: Decodable>(_ pointer: OpaquePointer?) -> T? {
        guard let pointer else { return nil }
        defer { zz_json_free(pointer) }
        let bytes = zz_json_bytes(pointer)
        guard let data = bytes.ptr else { return nil }
        return try? JSONDecoder().decode(T.self, from: Data(bytes: data, count: bytes.len))
    }
}
