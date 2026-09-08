import AppKit
import CZZClient
import Observation

public struct NativePane: Identifiable, Equatable {
    public let id: UInt64
    public let title: String
    public let kind: UInt32
    public let rect: CGRect
    public let active: Bool
    public var browser: NativeBrowserDescriptor?
    public var agent: NativeAgentDescriptor?
}

public struct NativeBrowserDescriptor: Codable, Equatable {
    public var tabs: [String]
    public var active_tab: Int
    public var profile: String
}

public struct NativeAgentDescriptor: Codable, Equatable {
    public var provider: String
    public var cwd: String?
    public var session_id: String?
}

private struct PaneDescriptor: Decodable {
    var browser: NativeBrowserDescriptor?
    var agent: NativeAgentDescriptor?
}

public struct NativeWindow: Identifiable, Equatable {
    public let id: UInt64
    public let index: UInt32
    public let name: String
    public let current: Bool
    public let panes: [NativePane]
}

public struct NativeSession: Identifiable, Equatable {
    public let id: UInt64
    public let name: String
    public let attached: Bool
    public let windows: [NativeWindow]
}

private final class ConnectionResult: @unchecked Sendable {
    let handle: OpaquePointer?
    let error: String
    let failure: zz_connect_failure
    var source: DispatchSourceRead?

    init(options: String) {
        var error = [CChar](repeating: 0, count: 4096)
        var failure = ZZ_CONNECT_FAILURE_NONE
        handle = options.withCString {
            zz_client_connect_native($0, nativeSSHCallback, nil, &failure, &error, error.count)
        }
        self.error = String(decoding: error.prefix(while: { $0 != 0 }).map { UInt8(bitPattern: $0) }, as: UTF8.self)
        self.failure = failure
    }

    deinit {
        source?.cancel()
        if let handle { zz_client_free(handle) }
    }
}

@Observable @MainActor
public final class NativeClient {
    public let browsers = NativeBrowserEngine()
    public let settings: NativeSettings
    public private(set) var appearance = NativeTerminalAppearance()
    public private(set) var agents: [UInt64: NativeAgentPane] = [:]
    public var endpoint: String
    public var sessionTarget: String
    public private(set) var sessions: [NativeSession] = []
    public private(set) var connected = false
    public private(set) var connecting = false
    public private(set) var message = "Connect to a zz daemon"
    public private(set) var framesPerSecond = 0
    @ObservationIgnored private var renderedFrames = 0
    @ObservationIgnored private var lastFrameSample = Date()
    public private(set) var commandError: String?
    public private(set) var prefixArmed = false
    public private(set) var prefixHints: [String] = []
    public private(set) var connectionGeneration = 0
    @ObservationIgnored private var connection: ConnectionResult?
    var handle: OpaquePointer? { connection?.handle }
    @ObservationIgnored private var connectionTask: Task<Void, Never>?
    @ObservationIgnored private var retryTask: Task<Void, Never>?
    @ObservationIgnored private var attempt = 0
    @ObservationIgnored private var retries = 0
    @ObservationIgnored private var slots: [UInt64: TerminalSlot] = [:]
    @ObservationIgnored private var wantsConnection = false
    @ObservationIgnored private var focused = true
    @ObservationIgnored private var copyRequest: UInt64 = 0
    @ObservationIgnored private var commandRequests = Set<UInt64>()
    @ObservationIgnored private let defaultEndpoint: String
    @ObservationIgnored private let muxConfigPath: String?
    @ObservationIgnored private var effectiveDark = true
    @ObservationIgnored private let preferencesPath: String?
    @ObservationIgnored private var agentEndpoint: String?

    public init(endpoint: String? = nil, session: String = "", config: String? = nil, muxConfig: String? = nil) {
        settings = NativeSettings(config: config, mux: muxConfig)
        preferencesPath = config.map { $0 + ".agent-preferences/preferences.json" }
        var buffer = [CChar](repeating: 0, count: 4096)
        _ = zz_client_default_endpoint(&buffer, buffer.count)
        defaultEndpoint = String(
            decoding: buffer.prefix(while: { $0 != 0 }).map { UInt8(bitPattern: $0) }, as: UTF8.self)
        self.endpoint = endpoint ?? defaultEndpoint
        muxConfigPath = muxConfig
        sessionTarget = session
    }

    public var attachedSession: NativeSession? { sessions.first(where: \.attached) }
    public var currentWindow: NativeWindow? { attachedSession?.windows.first(where: \.current) }
    public var activePane: NativePane? { currentWindow?.panes.first(where: \.active) }

    public func slot(for pane: UInt64) -> TerminalSlot {
        if let slot = slots[pane] { return slot }
        let slot = TerminalSlot()
        slots[pane] = slot
        return slot
    }

    public func connect() {
        disconnect()
        if agentEndpoint != endpoint { agents.removeAll(); agentEndpoint = endpoint }
        wantsConnection = true
        retries = 0
        openConnection()
    }

    public func disconnect() {
        wantsConnection = false
        retryTask?.cancel()
        retryTask = nil
        attempt += 1
        connectionTask?.cancel()
        connectionTask = nil
        closeConnection()
        connecting = false
        message = "Disconnected"
    }

    private func closeConnection() {
        connection = nil
        connected = false
        commandRequests.removeAll()
        prefixArmed = false
    }

    private func openConnection() {
        attempt += 1
        let token = attempt
        let helper =
            Bundle.main.privateFrameworksURL?.appendingPathComponent(
                "ZZNative Helper.app/Contents/MacOS/ZZNative Helper"
            ).path ?? ""
        let options: [String: Any] = [
            "endpoint": endpoint, "helper_path": helper,
            "start_if_missing": endpoint == defaultEndpoint,
            "restart_incompatible": settings.bool("auto-restart-stale-daemon"),
            "dark": effectiveDark, "working_directory": FileManager.default.currentDirectoryPath,
            "mux_config_path": muxConfigPath.map { $0 as Any } ?? NSNull(),
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: options),
            let optionsJSON = String(data: data, encoding: .utf8)
        else { return }
        connecting = true
        message = retries == 0 ? "Connecting…" : "Reconnecting…"
        connectionTask = Task { [weak self] in
            let result = await Task.detached(priority: .userInitiated) {
                ConnectionResult(options: optionsJSON)
            }.value
            guard let self, !Task.isCancelled, self.attempt == token else {
                return
            }
            self.connectionTask = nil
            self.connecting = false
            guard let handle = result.handle else {
                self.message = result.error
                if result.failure == ZZ_CONNECT_FAILURE_RETRYABLE { self.scheduleRetry() }
                return
            }
            self.connection = result
            if let path = self.preferencesPath {
                _ = path.withCString { zz_client_load_agent_preferences(handle, $0) }
            } else {
                _ = zz_client_load_agent_preferences(handle, nil)
            }
            let fd = zz_client_event_fd(handle)
            guard fd >= 0 else {
                self.closeConnection()
                self.message = "The connection has no event channel."
                return
            }
            self.connected = true
            self.settings.connect(self)
            self.connectionGeneration += 1
            for slot in self.slots.values { slot.frame = nil }
            self.sessions = []
            self.message = "Connected"
            let source = DispatchSource.makeReadSource(fileDescriptor: fd, queue: .main)
            source.setEventHandler { [weak self] in self?.drainEvents() }
            result.source = source
            source.resume()
            _ = zz_client_set_focused(handle, self.focused)
            if !self.sessionTarget.withCString({ zz_client_attach(handle, $0) }) {
                self.lostConnection()
                return
            }
            self.drainEvents()
        }
    }

    private func scheduleRetry() {
        guard wantsConnection else { return }
        retries += 1
        let delay = min(10, 1 << min(retries - 1, 3))
        retryTask?.cancel()
        retryTask = Task { [weak self] in
            do { try await Task.sleep(for: .seconds(delay)) } catch { return }
            guard let self, self.wantsConnection else { return }
            self.openConnection()
        }
    }

    private func lostConnection() {
        closeConnection()
        message = "Connection lost. Reconnecting…"
        scheduleRetry()
    }

    public func claimsPrefix(code: UInt32, scalar: UInt32, function: UInt8, modifiers: UInt8) -> Bool {
        guard let handle else { return false }
        return zz_client_claims_prefix_key(handle, code, scalar, function, modifiers)
    }
    public func cancelPrefix() {
        copyRequest &+= 1
        if let handle { _ = zz_client_cancel_prefix(handle, copyRequest) }
    }
    public func setDarkAppearance(_ dark: Bool) {
        effectiveDark = dark
        if let handle { _ = zz_client_set_color_scheme(handle, dark) }
    }

    public func setFocused(_ value: Bool) {
        focused = value
        if let handle { _ = zz_client_set_focused(handle, value) }
    }

    public func focusTerminal(_ pane: UInt64, focused: Bool) {
        guard attachedSession?.windows.contains(where: { $0.panes.contains(where: { $0.id == pane }) }) == true else {
            return
        }
        if let handle { _ = zz_client_focus_terminal(handle, pane, focused) }
    }

    public func attach(_ session: NativeSession) {
        guard let handle else { return }
        sessionTarget = session.name
        _ = session.name.withCString { zz_client_attach(handle, $0) }
    }

    @discardableResult public func execute(_ name: String, _ arguments: [String] = []) -> UInt64 {
        guard let handle else { return 0 }
        let strings = arguments.map { strdup($0) }
        defer { for string in strings { free(string) } }
        let pointers = strings.map { UnsafePointer<CChar>($0) }
        let sent = name.withCString { name in
            pointers.withUnsafeBufferPointer {
                zz_client_execute_request(handle, name, $0.baseAddress, $0.count)
            }
        }
        if sent == 0 {
            commandError = "Could not send the command."
        } else {
            commandRequests.insert(sent)
        }
        return sent
    }

    public func guiResponse(_ request: UInt64, ok: Bool, text: String) {
        if let handle { _ = text.withCString { zz_client_gui_response(handle, request, ok, $0) } }
    }

    public func browserText(_ text: String, pane: UInt64) {
        if let handle { _ = text.withCString { zz_client_send_browser_text(handle, pane, $0) } }
    }

    public func browserKey(
        _ pane: UInt64, code: UInt32, scalar: UInt32, function: UInt8,
        action: UInt32, modifiers: UInt8, text: String, textFollows: Bool
    ) {
        if let handle {
            _ = text.withCString {
                zz_client_send_browser_key(handle, pane, code, scalar, function, action, modifiers, $0, textFollows)
            }
        }
    }

    public func selectPane(_ pane: UInt64) { execute("select-pane", ["-t", "%\(pane)"]) }
    public func selectWindow(_ window: UInt64) { execute("select-window", ["-t", "@\(window)"]) }
    public func clearError() { commandError = nil }
    func trackRequest(_ request: UInt64) { if request != 0 { commandRequests.insert(request) } }
    public func recordFrame() { if settings.bool("show-fps") { renderedFrames += 1 } }
    func sampleFrameRate() {
        let elapsed = Date().timeIntervalSince(lastFrameSample)
        guard elapsed >= 1 else { return }
        framesPerSecond = Int((Double(renderedFrames) / elapsed).rounded())
        renderedFrames = 0
        lastFrameSample = Date()
    }
    public func resize(_ pane: UInt64, columns: UInt16, rows: UInt16, cell: CGSize) {
        guard let handle else { return }
        _ = zz_client_resize_terminal(
            handle, pane, columns, rows, UInt32(max(1, cell.width.rounded())), UInt32(max(1, cell.height.rounded())))
    }
    public func text(_ text: String, pane: UInt64, paste: Bool = false) {
        guard let handle else { return }
        _ = text.withCString { paste ? zz_client_paste(handle, pane, $0) : zz_client_send_text(handle, pane, $0) }
    }
    public func key(
        _ pane: UInt64, code: UInt32, scalar: UInt32 = 0, function: UInt8 = 0, action: UInt32 = 0, modifiers: UInt8 = 0,
        text: String = ""
    ) {
        guard let handle else { return }
        _ = text.withCString { zz_client_send_key(handle, pane, code, scalar, function, action, modifiers, $0, false) }
    }
    public func scroll(_ pane: UInt64, lines: Int32) {
        if let handle { _ = zz_client_scroll_lines(handle, pane, lines) }
    }
    public func selection(_ pane: UInt64, phase: UInt32, column: UInt16, row: UInt16, clicks: UInt8, rectangle: Bool) {
        if let handle { _ = zz_client_terminal_selection(handle, pane, phase, column, row, clicks, rectangle) }
    }
    public func copy(_ pane: UInt64) {
        guard let handle else { return }
        copyRequest &+= 1
        _ = zz_client_copy_selection(handle, pane, copyRequest)
    }

    private func drainEvents() {
        guard let handle else { return }
        var event = zz_client_event()
        var changed = false
        var panes = Set<UInt64>()
        var disconnected = false
        var agentChanged = false
        var guiCommands: [[String: Any]] = []
        while zz_client_next_event(handle, &event) {
            switch event.kind {
            case ZZ_EVENT_ATTACHED:
                retries = 0
                changed = true
            case ZZ_EVENT_SNAPSHOT_CHANGED, ZZ_EVENT_HELLO, ZZ_EVENT_DETACHED:
                changed = true
            case ZZ_EVENT_PANE_REMOVED:
                agents.removeValue(forKey: event.pane)
                changed = true
            case ZZ_EVENT_APPEARANCE_CHANGED:
                if let json = zz_client_appearance_json(handle) {
                    let bytes = zz_json_bytes(json)
                    if let pointer = bytes.ptr,
                        let appearance = try? JSONDecoder().decode(
                            NativeTerminalAppearance.self, from: Data(bytes: pointer, count: bytes.len))
                    {
                        self.appearance = appearance
                    }
                    zz_json_free(json)
                }
            case ZZ_EVENT_VIEWPORT_CHANGED:
                panes.insert(event.pane)
            case ZZ_EVENT_AGENT_STATE_CHANGED, ZZ_EVENT_AGENT_UPDATES, ZZ_EVENT_AGENT_LAGGED, ZZ_EVENT_AGENT_SESSIONS:
                agentChanged = true
            case ZZ_EVENT_PREFIX_ARMED, ZZ_EVENT_KEY_TABLES_CHANGED:
                refreshPrefix(handle)
                settings.refresh()
            case ZZ_EVENT_CLIPBOARD:
                while let clipboard = zz_client_clipboard_next(handle) {
                    let request = zz_clipboard_request_id(clipboard)
                    if request != 0, request == copyRequest {
                        NSPasteboard.general.clearContents()
                        NSPasteboard.general.setString(Self.string(zz_clipboard_text(clipboard)), forType: .string)
                    }
                    zz_clipboard_release(clipboard)
                }
            case ZZ_EVENT_COMMAND_REPLY:
                while let reply = zz_client_command_reply_next(handle) {
                    browsers.commandReply(zz_command_reply_request_id(reply), ok: zz_command_reply_ok(reply))
                    if commandRequests.remove(zz_command_reply_request_id(reply)) != nil, !zz_command_reply_ok(reply) {
                        commandError = Self.string(zz_command_reply_error(reply))
                    }
                    zz_command_reply_release(reply)
                }
            case ZZ_EVENT_GUI_COMMAND:
                while let command = zz_client_gui_command_next(handle) {
                    let bytes = zz_json_bytes(command)
                    if let pointer = bytes.ptr {
                        let data = Data(bytes: pointer, count: bytes.len)
                        if let value = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
                            guiCommands.append(value)
                        }
                    }
                    zz_json_free(command)
                }
            case ZZ_EVENT_DISCONNECTED, ZZ_EVENT_SERVER_STOPPING:
                disconnected = true
            default: break
            }
        }
        if disconnected {
            lostConnection()
            return
        }
        if changed { refreshSnapshot(handle) }
        if agentChanged || changed { drainAgents(handle) }
        for value in guiCommands {
            if value["kind"] as? String == "agent", let pane = (value["pane"] as? NSNumber)?.uint64Value {
                agents[pane]?.guiCommand(value)
            } else {
                browsers.handle(value, client: self)
            }
        }
        for pane in panes { slot(for: pane).frame = TerminalFrame(client: handle, pane: pane) }
    }

    private func drainAgents(_ handle: OpaquePointer) {
        while let batch = zz_client_agent_updates_next(handle) {
            if let model = agents[zz_agent_updates_pane(batch)]?.handle {
                _ = zz_agent_model_apply_updates(model, handle, batch)
            }
            zz_agent_updates_release(batch)
        }
        var pane: UInt64 = 0
        var sequence: UInt64 = 0
        while zz_client_agent_lagged_next(handle, &pane, &sequence) {
            if let model = agents[pane]?.handle { _ = zz_agent_model_replay(model, handle) }
        }
        while let reply = zz_client_agent_sessions_next(handle) {
            if let model = agents[zz_agent_sessions_pane(reply)]?.handle { zz_agent_model_apply_sessions(model, reply) }
            zz_agent_sessions_release(reply)
        }
        for model in agents.values { model.refresh() }
    }

    private func refreshPrefix(_ handle: OpaquePointer) {
        guard let prefix = zz_prefix_snapshot_acquire(handle) else { return }
        defer { zz_prefix_snapshot_release(prefix) }
        prefixArmed = zz_prefix_snapshot_armed(prefix)
        prefixHints = (0..<zz_prefix_binding_count(prefix)).map {
            "\(Self.string(zz_prefix_binding_key(prefix, $0)))  \(Self.string(zz_prefix_binding_summary(prefix, $0)))"
        }
    }

    private func refreshSnapshot(_ handle: OpaquePointer) {
        guard let snapshot = zz_client_snapshot_acquire(handle) else { return }
        defer { zz_snapshot_release(snapshot) }
        let next = (0..<zz_snapshot_session_count(snapshot)).map { s in
            NativeSession(
                id: zz_snapshot_session_id(snapshot, s), name: Self.string(zz_snapshot_session_name(snapshot, s)),
                attached: zz_snapshot_session_is_attached(snapshot, s),
                windows: (0..<zz_snapshot_session_window_count(snapshot, s)).map { w in
                    NativeWindow(
                        id: zz_snapshot_session_window_id(snapshot, s, w),
                        index: zz_snapshot_session_window_index(snapshot, s, w),
                        name: Self.string(zz_snapshot_session_window_name(snapshot, s, w)),
                        current: zz_snapshot_session_window_is_current(snapshot, s, w),
                        panes: (0..<zz_snapshot_session_window_pane_count(snapshot, s, w)).compactMap { p in
                            var rect = zz_pane_rect()
                            guard zz_snapshot_session_window_pane_rect(snapshot, s, w, p, &rect) else { return nil }
                            let id = zz_snapshot_session_window_pane_id(snapshot, s, w, p)
                            var descriptor: PaneDescriptor?
                            if let json = zz_snapshot_pane_descriptor(snapshot, id) {
                                let bytes = zz_json_bytes(json)
                                if let pointer = bytes.ptr {
                                    descriptor = try? JSONDecoder().decode(
                                        PaneDescriptor.self, from: Data(bytes: pointer, count: bytes.len))
                                }
                                zz_json_free(json)
                            }
                            return NativePane(
                                id: id,
                                title: Self.string(zz_snapshot_session_window_pane_title(snapshot, s, w, p)),
                                kind: zz_snapshot_session_window_pane_kind(snapshot, s, w, p).rawValue,
                                rect: CGRect(
                                    x: Double(rect.x), y: Double(rect.y), width: Double(rect.width),
                                    height: Double(rect.height)),
                                active: zz_snapshot_session_window_pane_is_active(snapshot, s, w, p),
                                browser: descriptor?.browser, agent: descriptor?.agent)
                        })
                })
        }
        if sessions != next { sessions = next }
        if let session = attachedSession { sessionTarget = session.name }
        let ids = Set(next.flatMap(\.windows).flatMap(\.panes).map(\.id))
        slots = slots.filter { ids.contains($0.key) }
        browsers.reconcile(attachedSession?.windows.flatMap(\.panes) ?? [], client: self)
        let agentPanes = next.flatMap(\.windows).flatMap(\.panes).filter { $0.agent != nil }
        for pane in agentPanes {
            let model = agents[pane.id] ?? NativeAgentPane(id: pane.id)
            agents[pane.id] = model
            model.connect(self)
        }
    }

    nonisolated static func string(_ bytes: zz_bytes) -> String {
        guard let ptr = bytes.ptr else { return "" }
        return String(decoding: UnsafeBufferPointer(start: ptr, count: bytes.len), as: UTF8.self)
    }
}
