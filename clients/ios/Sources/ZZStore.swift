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
    @Published private(set) var terminalInput = TerminalInputState()
    @Published private(set) var sceneIsActive = true
    @Published private(set) var terminalModifiers: UInt8 = 0
    @Published private(set) var actionError: String?
    @Published private(set) var isCreatingSession = false

    private var client: OpaquePointer?
    private var eventSource: DispatchSourceRead?
    private var terminalGeometries: [UInt64: TerminalGeometryState] = [:]
    private var pendingSessionIDs: Set<UInt64>?
    private var attachedSessionID: UInt64?
    private var pendingAttachmentSessionID: UInt64?
    private var hasEstablishedAttachment = false
    private var sessionCreationTimeout: Task<Void, Never>?

    private static let controlModifier: UInt8 = 1 << 1
    private static let altModifier: UInt8 = 1 << 2

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
        disconnect(preservingTerminalState: true)
        start()
    }

    func stop() {
        disconnect(preservingTerminalState: false)
    }

    func setSceneActive(_ active: Bool) {
        guard sceneIsActive != active else {
            if active {
                start()
            }
            return
        }
        sceneIsActive = active
        terminalModifiers = 0
        if active {
            let wasConnected = client != nil
            start()
            if wasConnected, let client {
                _ = zz_client_set_focused(client, true)
            }
            if let pane = terminalInput.owner.pane {
                focus(pane: pane, focused: true)
            }
        } else {
            if let client {
                _ = zz_client_set_focused(client, false)
            }
            if let pane = terminalInput.owner.pane {
                focus(pane: pane, focused: false)
                restoreStableGeometryAfterTransientInput(for: pane)
            }
        }
    }

    private func disconnect(preservingTerminalState: Bool) {
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
        if preservingTerminalState {
            invalidateSentGeometries()
        } else {
            terminalFontSizeSteps = [:]
            terminalGeometries = [:]
        }
        selectedSessionID = nil
        selectedPaneID = nil
        terminalInput = TerminalInputState()
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
        releaseTerminalInput()
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
        terminalModifiers = 0
        selectedPaneID = pane.id
        if pane.kind == .terminal {
            acquireTerminalInput(pane.id)
        } else {
            releaseTerminalInput()
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
        releaseTerminalInput()
        selectedPaneID = nil
        terminalModifiers = 0
    }

    func requestKeyboard(for pane: UInt64) {
        guard selectedPaneID == pane, selectedPane?.kind == .terminal else {
            return
        }
        acquireTerminalInput(pane)
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

    func resize(pane: UInt64, layout: TerminalLayout, stable: Bool) {
        var geometry = terminalGeometries[pane] ?? TerminalGeometryState()
        let shouldSend = geometry.observe(layout, stable: stable)
        terminalGeometries[pane] = geometry
        guard shouldSend, let client else {
            return
        }
        if send(layout, to: pane, client: client) {
            geometry.markSent(layout)
            terminalGeometries[pane] = geometry
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
        if terminalInput.owner.owns(pane) {
            releaseTerminalInput()
        }
        terminalGeometries.removeValue(forKey: pane)
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
        for pane in session.panes where pane.kind == .terminal {
            terminalGeometries[pane.id]?.invalidateSent()
        }
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
            case ZZ_EVENT_ATTACHED:
                _ = zz_client_set_focused(client, sceneIsActive)
                refreshMux = true
            case ZZ_EVENT_SNAPSHOT_CHANGED:
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
                terminalGeometries.removeValue(forKey: event.pane)
                terminalFontSizeSteps.removeValue(forKey: event.pane)
                if terminalInput.owner.owns(event.pane) {
                    terminalInput.release()
                }
                refreshMux = true
            case ZZ_EVENT_DETACHED:
                invalidateSentGeometries()
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
            if terminalInput.owner.owns(selectedPaneID) {
                terminalInput.release()
            }
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
        terminalGeometries = terminalGeometries.filter { knownTerminalPanes.contains($0.key) }
        terminalFontSizeSteps = terminalFontSizeSteps.filter { knownTerminalPanes.contains($0.key) }
        frames = frames.filter { attachedPanes.contains($0.key) }
        for pane in attachedPanes {
            restoreStableGeometry(for: pane, client: client)
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

    private func acquireTerminalInput(_ pane: UInt64) {
        if let previous = terminalInput.owner.pane, previous != pane {
            focus(pane: previous, focused: false)
            restoreStableGeometryAfterTransientInput(for: previous)
        }
        terminalInput.acquire(pane)
        if sceneIsActive {
            focus(pane: pane, focused: true)
        }
    }

    private func releaseTerminalInput() {
        guard let pane = terminalInput.owner.pane else {
            return
        }
        focus(pane: pane, focused: false)
        restoreStableGeometryAfterTransientInput(for: pane)
        terminalInput.release()
    }

    private func send(_ layout: TerminalLayout, to pane: UInt64, client: OpaquePointer) -> Bool {
        zz_client_resize_terminal(
            client,
            pane,
            layout.columns,
            layout.rows,
            layout.cellWidth,
            layout.cellHeight
        )
    }

    private func restoreStableGeometry(for pane: UInt64, client: OpaquePointer) {
        guard var geometry = terminalGeometries[pane],
              let layout = geometry.reconnectLayout,
              send(layout, to: pane, client: client) else {
            return
        }
        geometry.markSent(layout)
        terminalGeometries[pane] = geometry
    }

    private func restoreStableGeometryAfterTransientInput(for pane: UInt64) {
        guard let client, var geometry = terminalGeometries[pane],
              let layout = geometry.stableLayoutToRestore,
              send(layout, to: pane, client: client) else {
            return
        }
        geometry.markSent(layout)
        terminalGeometries[pane] = geometry
    }

    private func invalidateSentGeometries() {
        for pane in terminalGeometries.keys {
            terminalGeometries[pane]?.invalidateSent()
        }
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
