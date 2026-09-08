import AppKit
import CZZClient
import Observation

public struct NativePane: Identifiable, Equatable {
    public let id: UInt64
    public let title: String
    public let kind: UInt32
    public let rect: CGRect
    public let active: Bool
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

    init(endpoint: String) {
        var error = [CChar](repeating: 0, count: 4096)
        var failure = ZZ_CONNECT_FAILURE_NONE
        handle = endpoint.withCString {
            zz_client_connect_endpoint_interactive($0, nil, nil, &failure, &error, error.count)
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
    public var endpoint: String
    public var sessionTarget: String
    public private(set) var sessions: [NativeSession] = []
    public private(set) var connected = false
    public private(set) var connecting = false
    public private(set) var message = "Connect to a zz daemon"
    public private(set) var commandError: String?
    public private(set) var prefixArmed = false
    public private(set) var prefixHints: [String] = []
    public private(set) var connectionGeneration = 0
    @ObservationIgnored private var connection: ConnectionResult?
    private var handle: OpaquePointer? { connection?.handle }
    @ObservationIgnored private var connectionTask: Task<Void, Never>?
    @ObservationIgnored private var retryTask: Task<Void, Never>?
    @ObservationIgnored private var attempt = 0
    @ObservationIgnored private var retries = 0
    @ObservationIgnored private var slots: [UInt64: TerminalSlot] = [:]
    @ObservationIgnored private var wantsConnection = false
    @ObservationIgnored private var focused = true
    @ObservationIgnored private var copyRequest: UInt64 = 0
    @ObservationIgnored private var commandRequests = Set<UInt64>()

    public init(endpoint: String? = nil, session: String = "") {
        var buffer = [CChar](repeating: 0, count: 4096)
        _ = zz_client_default_endpoint(&buffer, buffer.count)
        self.endpoint =
            endpoint ?? String(decoding: buffer.prefix(while: { $0 != 0 }).map { UInt8(bitPattern: $0) }, as: UTF8.self)
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
        let endpoint = endpoint
        connecting = true
        message = retries == 0 ? "Connecting…" : "Reconnecting…"
        connectionTask = Task { [weak self] in
            let result = await Task.detached(priority: .userInitiated) {
                ConnectionResult(endpoint: endpoint)
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
            let fd = zz_client_event_fd(handle)
            guard fd >= 0 else {
                self.closeConnection()
                self.message = "The connection has no event channel."
                return
            }
            self.connected = true
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

    public func execute(_ name: String, _ arguments: [String] = []) {
        guard let handle else { return }
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
    }

    public func selectPane(_ pane: UInt64) { execute("select-pane", ["-t", "%\(pane)"]) }
    public func selectWindow(_ window: UInt64) { execute("select-window", ["-t", "@\(window)"]) }
    public func clearError() { commandError = nil }
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
        while zz_client_next_event(handle, &event) {
            switch event.kind {
            case ZZ_EVENT_ATTACHED:
                retries = 0
                changed = true
            case ZZ_EVENT_SNAPSHOT_CHANGED, ZZ_EVENT_HELLO, ZZ_EVENT_DETACHED, ZZ_EVENT_PANE_REMOVED:
                changed = true
            case ZZ_EVENT_VIEWPORT_CHANGED:
                panes.insert(event.pane)
            case ZZ_EVENT_PREFIX_ARMED, ZZ_EVENT_KEY_TABLES_CHANGED:
                refreshPrefix(handle)
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
                    if commandRequests.remove(zz_command_reply_request_id(reply)) != nil, !zz_command_reply_ok(reply) {
                        commandError = Self.string(zz_command_reply_error(reply))
                    }
                    zz_command_reply_release(reply)
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
        for pane in panes { slot(for: pane).frame = TerminalFrame(client: handle, pane: pane) }
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
                            return NativePane(
                                id: zz_snapshot_session_window_pane_id(snapshot, s, w, p),
                                title: Self.string(zz_snapshot_session_window_pane_title(snapshot, s, w, p)),
                                kind: zz_snapshot_session_window_pane_kind(snapshot, s, w, p).rawValue,
                                rect: CGRect(
                                    x: Double(rect.x), y: Double(rect.y), width: Double(rect.width),
                                    height: Double(rect.height)),
                                active: zz_snapshot_session_window_pane_is_active(snapshot, s, w, p))
                        })
                })
        }
        if sessions != next { sessions = next }
        if let session = attachedSession { sessionTarget = session.name }
        let ids = Set(next.flatMap(\.windows).flatMap(\.panes).map(\.id))
        slots = slots.filter { ids.contains($0.key) }
    }

    private static func string(_ bytes: zz_bytes) -> String {
        guard let ptr = bytes.ptr else { return "" }
        return String(decoding: UnsafeBufferPointer(start: ptr, count: bytes.len), as: UTF8.self)
    }
}
