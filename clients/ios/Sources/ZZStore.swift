import Darwin
import Foundation

@MainActor
final class ZZStore: ObservableObject {
    @Published private(set) var connectionState: ZZConnectionState = .idle
    @Published private(set) var sessions: [ZZSession] = []
    @Published private(set) var frames: [UInt64: TerminalFrame] = [:]
    @Published private(set) var terminalFontSizeSteps: [UInt64: Int] = [:]
    @Published var selectedSessionID: UInt64?
    @Published var selectedPaneID: UInt64?
    @Published private(set) var keyboardRevision: UInt64 = 0
    @Published private(set) var terminalModifiers: UInt8 = 0
    @Published private(set) var actionError: String?
    @Published private(set) var isCreatingSession = false

    private var client: OpaquePointer?
    private var eventSource: DispatchSourceRead?
    private var terminalLayouts: [UInt64: TerminalLayout] = [:]
    private var pendingSessionIDs: Set<UInt64>?
    private var attachedSessionID: UInt64?
    private var pendingAttachmentSessionID: UInt64?
    private var hasEstablishedAttachment = false
    private var sessionCreationTimeout: Task<Void, Never>?

    private static let controlModifier: UInt8 = 1 << 1
    private static let altModifier: UInt8 = 1 << 2

    private struct TerminalLayout: Equatable {
        let columns: UInt16
        let rows: UInt16
        let cellWidth: UInt32
        let cellHeight: UInt32
    }

    var selectedSession: ZZSession? {
        guard let selectedSessionID else {
            return nil
        }
        return sessions.first { $0.id == selectedSessionID }
    }

    var selectedPane: ZZPane? {
        guard let selectedPaneID else {
            return nil
        }
        return sessions.lazy.flatMap(\.panes).first { $0.id == selectedPaneID }
    }

    var controlModifierEnabled: Bool {
        terminalModifiers & Self.controlModifier != 0
    }

    var altModifierEnabled: Bool {
        terminalModifiers & Self.altModifier != 0
    }

    func start() {
        guard client == nil else {
            return
        }
        guard let socket = ProcessInfo.processInfo.environment["ZZ_SOCKET"], !socket.isEmpty else {
            connectionState = .failed("Launch with `just ios` so zz can reach the daemon on your Mac.")
            return
        }
        connectionState = .connecting
        guard let connected = socket.withCString({ zz_client_connect($0) }) else {
            connectionState = .failed("Couldn’t connect to the zz daemon at \(socket).")
            return
        }
        client = connected
        let fileDescriptor = zz_client_event_fd(connected)
        guard fileDescriptor >= 0 else {
            zz_client_free(connected)
            client = nil
            connectionState = .failed("The zz client opened without an event channel.")
            return
        }
        let source = DispatchSource.makeReadSource(fileDescriptor: fileDescriptor, queue: .main)
        source.setEventHandler { [weak self] in
            self?.drainEvents()
        }
        source.resume()
        eventSource = source
        connectionState = .connected
        _ = "".withCString { zz_client_attach(connected, $0) }
        drainEvents()
        refreshSnapshot()
    }

    func retry() {
        stop()
        start()
    }

    func stop() {
        sessionCreationTimeout?.cancel()
        sessionCreationTimeout = nil
        pendingSessionIDs = nil
        attachedSessionID = nil
        pendingAttachmentSessionID = nil
        hasEstablishedAttachment = false
        isCreatingSession = false
        eventSource?.cancel()
        eventSource = nil
        if let client {
            zz_client_free(client)
        }
        client = nil
        sessions = []
        frames = [:]
        terminalFontSizeSteps = [:]
        terminalLayouts = [:]
        selectedSessionID = nil
        selectedPaneID = nil
        terminalModifiers = 0
        connectionState = .idle
    }

    func selectSession(_ session: ZZSession) {
        guard let client else {
            return
        }
        if attachedSessionID == session.id && pendingAttachmentSessionID == nil {
            selectedSessionID = session.id
            return
        }
        if let pane = selectedPane, pane.kind == .terminal {
            focus(pane: pane.id, focused: false)
        }
        let previous = selectedSessionID
        let previousPendingAttachment = pendingAttachmentSessionID
        selectedSessionID = session.id
        selectedPaneID = nil
        terminalModifiers = 0
        if !requestAttachment(to: session, client: client) {
            selectedSessionID = previous
            pendingAttachmentSessionID = previousPendingAttachment
            actionError = "zz couldn’t switch to that session."
        }
    }

    func selectAdjacentSession(offset: Int) {
        guard offset != 0, !sessions.isEmpty else {
            return
        }
        let current = selectedSessionID.flatMap { id in
            sessions.firstIndex { $0.id == id }
        } ?? 0
        let next = current + offset
        guard sessions.indices.contains(next) else {
            return
        }
        selectSession(sessions[next])
    }

    func openPane(_ pane: ZZPane) {
        if let current = selectedPane, current.id != pane.id, current.kind == .terminal {
            focus(pane: current.id, focused: false)
        }
        terminalModifiers = 0
        selectedPaneID = pane.id
        if pane.kind == .terminal {
            focus(pane: pane.id, focused: true)
            requestKeyboard()
        }
    }

    func selectAdjacentPane(from pane: UInt64, offset: Int) {
        guard offset != 0, let panes = selectedSession?.panes,
              let current = panes.firstIndex(where: { $0.id == pane }) else {
            return
        }
        let next = current + offset
        guard panes.indices.contains(next) else {
            return
        }
        openPane(panes[next])
    }

    func showOverview() {
        if let pane = selectedPane, pane.kind == .terminal {
            focus(pane: pane.id, focused: false)
        }
        selectedPaneID = nil
        terminalModifiers = 0
    }

    func requestKeyboard() {
        keyboardRevision &+= 1
    }

    func frame(for pane: UInt64) -> TerminalFrame? {
        frames[pane]
    }

    func terminalFontSizeStep(for pane: UInt64) -> Int {
        terminalFontSizeSteps[pane] ?? 0
    }

    func setTerminalFontSizeStep(_ step: Int, for pane: UInt64) {
        let step = TerminalFontZoom.clamped(step)
        guard terminalFontSizeSteps[pane] != step else {
            return
        }
        terminalFontSizeSteps[pane] = step
    }

    func sendText(_ text: String, to pane: UInt64) {
        guard let client, !text.isEmpty else {
            return
        }
        if terminalModifiers != 0,
           text.unicodeScalars.count == 1,
           let scalar = text.unicodeScalars.first {
            sendKey(
                UInt32(ZZ_KEY_CHARACTER.rawValue),
                to: pane,
                codepoint: scalar.value,
                modifiers: terminalModifiers
            )
            terminalModifiers = 0
            return
        }
        _ = text.withCString { zz_client_send_text(client, pane, $0) }
        terminalModifiers = 0
    }

    func sendKey(
        _ code: UInt32,
        to pane: UInt64,
        codepoint: UInt32 = 0,
        function: UInt8 = 0,
        action: UInt32 = UInt32(ZZ_KEY_PRESS.rawValue),
        modifiers: UInt8 = 0,
        textFollows: Bool = false
    ) {
        guard let client else {
            return
        }
        _ = zz_client_send_key(
            client,
            pane,
            code,
            codepoint,
            function,
            action,
            modifiers,
            nil,
            textFollows
        )
    }

    func resize(pane: UInt64, columns: Int, rows: Int, cell: CGSize) {
        guard let client, columns > 0, rows > 0 else {
            return
        }
        let layout = TerminalLayout(
            columns: UInt16(clamping: columns),
            rows: UInt16(clamping: rows),
            cellWidth: UInt32(clamping: Int(cell.width.rounded(.up))),
            cellHeight: UInt32(clamping: Int(cell.height.rounded(.up)))
        )
        guard terminalLayouts[pane] != layout else {
            return
        }
        if zz_client_resize_terminal(
            client,
            pane,
            layout.columns,
            layout.rows,
            layout.cellWidth,
            layout.cellHeight
        ) {
            terminalLayouts[pane] = layout
        }
    }

    func scroll(pane: UInt64, lines: Int) {
        guard let client, lines != 0 else {
            return
        }
        _ = zz_client_scroll_lines(client, pane, Int32(clamping: lines))
    }

    func sendPrefix(to pane: UInt64) {
        _ = execute("send-prefix", args: ["-t", "%\(pane)"])
        terminalModifiers = 0
    }

    func sendShortcutKey(_ code: UInt32, to pane: UInt64) {
        sendKey(code, to: pane, modifiers: terminalModifiers)
        terminalModifiers = 0
    }

    func toggleControlModifier() {
        terminalModifiers ^= Self.controlModifier
    }

    func toggleAltModifier() {
        terminalModifiers ^= Self.altModifier
    }

    func newSession() {
        guard !isCreatingSession else {
            return
        }
        actionError = nil
        isCreatingSession = true
        pendingSessionIDs = Set(sessions.map(\.id))
        let previousPendingAttachment = pendingAttachmentSessionID
        pendingAttachmentSessionID = nil
        guard execute("new-session", args: []) else {
            pendingAttachmentSessionID = previousPendingAttachment
            finishSessionCreation(error: "zz couldn’t send the new session request.")
            return
        }
        sessionCreationTimeout = Task { [weak self] in
            try? await Task.sleep(for: .seconds(3))
            guard !Task.isCancelled, let self else {
                return
            }
            self.refreshSnapshot()
            guard self.pendingSessionIDs != nil else {
                return
            }
            self.finishSessionCreation(error: "zz didn’t create a session. Try again.")
        }
    }

    func newPane() {
        guard let session = selectedSession else {
            actionError = "Select a session before creating a pane."
            return
        }
        let terminal = session.panes.first { $0.isActive && $0.kind == .terminal }
            ?? session.panes.first { $0.kind == .terminal }
        let created = if let terminal {
            execute("split-window", args: ["-t", "%\(terminal.id)"])
        } else {
            execute("new-window", args: [])
        }
        if !created {
            actionError = "zz couldn’t create a pane."
        }
    }

    func closePane(_ pane: UInt64) {
        if selectedPaneID == pane {
            selectedPaneID = nil
        }
        terminalLayouts.removeValue(forKey: pane)
        terminalFontSizeSteps.removeValue(forKey: pane)
        if !execute("kill-pane", args: ["-t", "%\(pane)"]) {
            actionError = "zz couldn’t close that pane."
        }
    }

    func dismissActionError() {
        actionError = nil
    }

    private func focus(pane: UInt64, focused: Bool) {
        guard let client else {
            return
        }
        _ = zz_client_focus_terminal(client, pane, focused)
    }

    private func requestAttachment(to session: ZZSession, client: OpaquePointer) -> Bool {
        pendingAttachmentSessionID = session.id
        guard session.name.withCString({ zz_client_attach(client, $0) }) else {
            pendingAttachmentSessionID = nil
            return false
        }
        return true
    }

    private func execute(_ name: String, args: [String]) -> Bool {
        guard let client else {
            return false
        }
        let allocated = args.map { argument in
            argument.withCString { strdup($0) }
        }
        guard allocated.allSatisfy({ $0 != nil }) else {
            allocated.forEach { free($0) }
            return false
        }
        defer { allocated.forEach { free($0) } }
        let pointers = allocated.map { pointer in
            pointer.map { UnsafePointer<CChar>($0) }
        }
        return name.withCString { namePointer in
            pointers.withUnsafeBufferPointer { arguments in
                zz_client_execute(client, namePointer, arguments.baseAddress, arguments.count)
            }
        }
    }

    private func drainEvents() {
        guard let client else {
            return
        }
        var event = zz_client_event()
        var refreshMux = false
        while zz_client_next_event(client, &event) {
            switch event.kind {
            case ZZ_EVENT_HELLO:
                connectionState = .connected
            case ZZ_EVENT_ATTACHED, ZZ_EVENT_SNAPSHOT_CHANGED:
                refreshMux = true
            case ZZ_EVENT_VIEWPORT_CHANGED:
                let damage = TerminalDamage(
                    all: event.flags & UInt32(ZZ_EVENT_DAMAGE_ALL) != 0,
                    firstRow: Int(event.row_start),
                    lastRow: Int(event.row_end)
                )
                refreshFrame(pane: event.pane, damage: damage)
            case ZZ_EVENT_PANE_REMOVED:
                frames.removeValue(forKey: event.pane)
                terminalLayouts.removeValue(forKey: event.pane)
                terminalFontSizeSteps.removeValue(forKey: event.pane)
                refreshMux = true
            case ZZ_EVENT_DETACHED:
                refreshMux = true
            case ZZ_EVENT_SERVER_STOPPING, ZZ_EVENT_DISCONNECTED:
                connectionState = .disconnected
            default:
                break
            }
        }
        if refreshMux {
            refreshSnapshot()
        }
    }

    private func refreshSnapshot() {
        guard let client, let snapshot = zz_client_snapshot_acquire(client) else {
            return
        }
        defer { zz_snapshot_release(snapshot) }

        let sessionCount = Int(zz_snapshot_session_count(snapshot))
        var nextSessions: [ZZSession] = []
        nextSessions.reserveCapacity(sessionCount)
        for sessionIndex in 0..<sessionCount {
            let paneCount = Int(zz_snapshot_session_pane_count(snapshot, sessionIndex))
            var panes: [ZZPane] = []
            panes.reserveCapacity(paneCount)
            for paneIndex in 0..<paneCount {
                let rawKind = zz_snapshot_session_pane_kind(snapshot, sessionIndex, paneIndex)
                panes.append(
                    ZZPane(
                        id: zz_snapshot_session_pane_id(snapshot, sessionIndex, paneIndex),
                        title: string(zz_snapshot_session_pane_title(snapshot, sessionIndex, paneIndex)),
                        kind: ZZPaneKind(rawValue: UInt32(rawKind.rawValue)) ?? .picker,
                        isActive: zz_snapshot_session_pane_is_active(snapshot, sessionIndex, paneIndex),
                        hasBell: zz_snapshot_session_pane_has_bell(snapshot, sessionIndex, paneIndex)
                    )
                )
            }
            nextSessions.append(
                ZZSession(
                    id: zz_snapshot_session_id(snapshot, sessionIndex),
                    name: string(zz_snapshot_session_name(snapshot, sessionIndex)),
                    activeWindow: zz_snapshot_session_active_window(snapshot, sessionIndex),
                    panes: panes,
                    isAttached: zz_snapshot_session_is_attached(snapshot, sessionIndex)
                )
            )
        }

        sessions = nextSessions
        if let pendingSessionIDs,
           nextSessions.contains(where: { !pendingSessionIDs.contains($0.id) && $0.isAttached }) {
            finishSessionCreation()
        }
        let attached = nextSessions.first(where: \.isAttached)
        attachedSessionID = attached?.id
        if attached != nil {
            hasEstablishedAttachment = true
        }
        if let pendingAttachmentSessionID {
            if attached?.id == pendingAttachmentSessionID {
                self.pendingAttachmentSessionID = nil
                selectedSessionID = pendingAttachmentSessionID
            } else if nextSessions.contains(where: { $0.id == pendingAttachmentSessionID }) {
                selectedSessionID = pendingAttachmentSessionID
            } else {
                self.pendingAttachmentSessionID = nil
            }
        }
        if pendingAttachmentSessionID == nil {
            if let attached {
                selectedSessionID = attached.id
            } else if selectedSessionID == nil
                        || !nextSessions.contains(where: { $0.id == selectedSessionID }) {
                selectedSessionID = nextSessions.first?.id
            }
        }
        if let selectedPaneID,
           !nextSessions.lazy.flatMap(\.panes).contains(where: { $0.id == selectedPaneID }) {
            self.selectedPaneID = nil
        }

        let attachedPanes = Set(
            nextSessions
                .first(where: \.isAttached)?
                .panes
                .filter { $0.kind == .terminal }
                .map(\.id) ?? []
        )
        let knownTerminalPanes = Set(
            nextSessions
                .flatMap(\.panes)
                .filter { $0.kind == .terminal }
                .map(\.id)
        )
        terminalLayouts = terminalLayouts.filter { knownTerminalPanes.contains($0.key) }
        terminalFontSizeSteps = terminalFontSizeSteps.filter { knownTerminalPanes.contains($0.key) }
        frames = frames.filter { attachedPanes.contains($0.key) }
        for pane in attachedPanes {
            refreshFrame(pane: pane, damage: .full)
        }
        if attached == nil,
           pendingAttachmentSessionID == nil,
           hasEstablishedAttachment,
           !isCreatingSession,
           let session = selectedSession ?? nextSessions.first {
            if !requestAttachment(to: session, client: client) {
                actionError = "zz couldn’t recover after that session closed."
            }
        }
    }

    private func refreshFrame(pane: UInt64, damage: TerminalDamage) {
        guard let client, let frame = TerminalFrame(client: client, pane: pane, damage: damage) else {
            return
        }
        frames[pane] = frame
    }

    private func string(_ bytes: zz_bytes) -> String {
        guard let pointer = bytes.ptr, bytes.len > 0 else {
            return ""
        }
        return String(
            decoding: UnsafeBufferPointer(start: pointer, count: bytes.len),
            as: UTF8.self
        )
    }

    private func finishSessionCreation(error: String? = nil) {
        sessionCreationTimeout?.cancel()
        sessionCreationTimeout = nil
        pendingSessionIDs = nil
        isCreatingSession = false
        if let error {
            actionError = error
        }
    }
}
