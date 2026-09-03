import Darwin
import Foundation
import Network
import UIKit

private struct ZZClientConnectionResult: @unchecked Sendable {
    let client: OpaquePointer?
    let error: String
    let failure: ZZConnectFailure

    func release() {
        if let client {
            zz_client_free(client)
        }
    }
}

@MainActor
final class ZZStore: ObservableObject {
    @Published private(set) var connectionState: ZZConnectionState = .idle
    @Published private(set) var sessions: [ZZSession] = []
    @Published private(set) var terminalFontSizeSteps: [UInt64: Int] = [:]
    @Published var selectedSessionID: UInt64?
    @Published var selectedPaneID: UInt64?
    @Published private(set) var terminalInput = TerminalInputState()
    @Published private(set) var sceneIsActive = true
    @Published private(set) var terminalModifierState = TerminalModifierLatchState()
    @Published private(set) var actionError: String?
    @Published private(set) var actionNotice: ZZActionNotice?
    @Published private(set) var isCreatingSession = false
    @Published private(set) var hostEndpoint = ""
    @Published private(set) var sshPublicKey: String?
    @Published private(set) var sshPrompt: ZZSSHPromptRequest?
    @Published private(set) var agentStates: [UInt64: ZZAgentState] = [:]
    @Published private(set) var prefixArmed = false
    @Published private(set) var prefixBindings: [ZZPrefixBinding] = []
    @Published private(set) var tmuxImportPhase = ZZTMuxImportPhase.hidden

    private var client: OpaquePointer?
    private var eventSource: DispatchSourceRead?
    private var frameSlots: [UInt64: TerminalFrameSlot] = [:]
    private var terminalGeometries: [UInt64: TerminalGeometryState] = [:]
    private var pendingSessionIDs: Set<UInt64>?
    private var attachedSessionID: UInt64?
    private var pendingAttachmentSessionID: UInt64?
    private var hasEstablishedAttachment = false
    private var sessionCreationTimeout: Task<Void, Never>?
    private var tmuxImportTask: Task<Void, Never>?
    private var connectionTask: Task<Void, Never>?
    private var reconnectTask: Task<Void, Never>?
    private var publicKeyTask: Task<Void, Never>?
    private var connectionAttempt: UInt64 = 0
    private var reconnectAttempt = 0
    private var backgroundTask = UIBackgroundTaskIdentifier.invalid
    private var thawGraceDeadline: TimeInterval = 0
    private var connectionEndpoint: String?
    private var connectionSavesHost = false
    private var connectionReturnsToHostSetup = false
    private var connectionPromptBroker: ZZSSHPromptBroker?
    private var rememberedSessionName: String?
    private var rememberedPaneID: UInt64?
    private var pendingNavigation: ZZNavigationTarget?
    private var navigationCommandSent = false
    private var terminalPreviewRequested = false
    private var unseenAgentCompletions: Set<UInt64> = []
    private var agentDrafts = ZZAgentDrafts()
    private var agentThreadSlots: [UInt64: ZZAgentThreadSlot] = [:]
    @Published private(set) var agentSessionLists: [UInt64: ZZAgentSessionList] = [:]
    private var clipboardRequestID: UInt64 = 1
    private var commandRequests = ZZCommandRequests()
    private var noticeSequence: UInt64 = 1
    private var noticeDismissal: Task<Void, Never>?
    private let networkMonitor = NWPathMonitor()
    private let networkQueue = DispatchQueue(label: "zz-ios-network")
    private var networkAvailable = true
    private let agentNotifications = ZZAgentNotifications()
    nonisolated(unsafe) private var observers: [NSObjectProtocol] = []

    private static let shiftModifier: UInt8 = 1 << 0
    private static let controlModifier: UInt8 = 1 << 1
    private static let altModifier: UInt8 = 1 << 2
    private static let savedHostKey = "zz.saved-host"
    private static let backgroundTaskName = "zz.connection"

    init() {
        networkMonitor.pathUpdateHandler = { [weak self] path in
            Task { @MainActor [weak self] in
                self?.networkPathChanged(path.status == .satisfied)
            }
        }
        networkMonitor.start(queue: networkQueue)
        observers.append(
            NotificationCenter.default.addObserver(
                forName: .zzNotificationRoute,
                object: nil,
                queue: .main
            ) { [weak self] notification in
                guard let session = notification.userInfo?["session"] as? NSNumber,
                      let pane = notification.userInfo?["pane"] as? NSNumber else {
                    return
                }
                Task { @MainActor [weak self] in
                    self?.open(
                        ZZNavigationTarget(
                            session: session.uint64Value,
                            pane: pane.uint64Value
                        )
                    )
                }
            }
        )
        observers.append(
            NotificationCenter.default.addObserver(
                forName: .zzShortcutCommand,
                object: nil,
                queue: .main
            ) { [weak self] _ in
                Task { @MainActor [weak self] in
                    self?.consumeShortcutCommand()
                }
            }
        )
    }

    deinit {
        networkMonitor.cancel()
        observers.forEach(NotificationCenter.default.removeObserver)
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
        terminalModifierState.contains(Self.controlModifier)
    }

    var altModifierEnabled: Bool {
        terminalModifierState.contains(Self.altModifier)
    }

    var shiftModifierEnabled: Bool {
        terminalModifierState.contains(Self.shiftModifier)
    }

    var controlModifierLocked: Bool {
        terminalModifierState.isLocked(Self.controlModifier)
    }

    var altModifierLocked: Bool {
        terminalModifierState.isLocked(Self.altModifier)
    }

    var shiftModifierLocked: Bool {
        terminalModifierState.isLocked(Self.shiftModifier)
    }

    var isConnected: Bool {
        client != nil && connectionState == .connected
    }

    var agentAttention: [ZZAgentAttention] {
        agentStates.values.compactMap { state in
            let kind: ZZAgentAttentionKind?
            if state.status == .needsInput {
                kind = .blocked
            } else if state.status == .failed {
                kind = .failed
            } else if unseenAgentCompletions.contains(state.pane) {
                kind = .done
            } else if state.status == .working {
                kind = .working
            } else {
                kind = nil
            }
            guard let kind else {
                return nil
            }
            return ZZAgentAttention(
                pane: state.pane,
                session: attachedSessionID,
                title: state.title ?? paneTitle(state.pane) ?? "Agent",
                kind: kind
            )
        }
        .sorted {
            if $0.kind != $1.kind {
                return $0.kind > $1.kind
            }
            return $0.pane < $1.pane
        }
    }

    var canConfigureHost: Bool {
        localSocket == nil
    }

    var hasSavedHost: Bool {
        UserDefaults.standard.string(forKey: Self.savedHostKey) != nil
    }

    func start() {
        guard sceneIsActive, client == nil, connectionTask == nil else {
            return
        }
        if case .needsHost = connectionState {
            return
        }
        if let localSocket {
            beginConnection(
                endpoint: localSocket,
                password: nil,
                savesHost: false,
                returnsToHostSetup: false,
                reconnecting: reconnectAttempt > 0
            )
            return
        }
        if let saved = UserDefaults.standard.string(forKey: Self.savedHostKey), !saved.isEmpty {
            hostEndpoint = saved
            beginConnection(
                endpoint: saved,
                password: nil,
                savesHost: true,
                returnsToHostSetup: true,
                reconnecting: reconnectAttempt > 0
            )
            return
        }
        presentHostSetup(message: nil)
    }

    func connectHost(_ value: String, password: String?) {
        guard let endpoint = ZZHostEndpoint.normalized(value) else {
            presentHostSetup(message: "Enter a host as user@hostname or ssh://user@hostname.")
            return
        }
        disconnect(preservingTerminalState: true)
        reconnectAttempt = 0
        hostEndpoint = endpoint
        beginConnection(
            endpoint: endpoint,
            password: password,
            savesHost: true,
            returnsToHostSetup: true,
            reconnecting: false
        )
    }

    func showHostSetup() {
        guard canConfigureHost else {
            return
        }
        disconnect(preservingTerminalState: true)
        if hostEndpoint.isEmpty {
            hostEndpoint = UserDefaults.standard.string(forKey: Self.savedHostKey) ?? ""
        }
        presentHostSetup(message: nil)
    }

    func forgetHost() {
        disconnect(preservingTerminalState: false)
        UserDefaults.standard.removeObject(forKey: Self.savedHostKey)
        hostEndpoint = ""
        presentHostSetup(message: nil)
    }

    func retry() {
        let preservesPresentation = !sessions.isEmpty
        tearDownConnection(preservingPresentation: preservesPresentation)
        reconnectAttempt = 0
        start()
    }

    func stop() {
        publicKeyTask?.cancel()
        publicKeyTask = nil
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
        terminalModifierState.reset()
        if active {
            endBackgroundGrace()
            thawGraceDeadline = Date.timeIntervalSinceReferenceDate
                + ZZReconnectPolicy.thawGraceSeconds
            let wasConnected = client != nil
            start()
            if wasConnected, let client {
                _ = zz_client_set_focused(client, true)
            }
            if let pane = terminalInput.owner.pane {
                focus(pane: pane, focused: true)
            }
            consumeShortcutCommand()
        } else {
            beginBackgroundGrace()
            if let client {
                _ = zz_client_set_focused(client, false)
            }
            if let pane = terminalInput.owner.pane {
                focus(pane: pane, focused: false)
                restoreStableGeometryAfterTransientInput(for: pane)
            }
        }
    }

    private var isWithinThawGrace: Bool {
        !sceneIsActive || Date.timeIntervalSinceReferenceDate < thawGraceDeadline
    }

    private func beginBackgroundGrace() {
        guard backgroundTask == .invalid,
              client != nil || connectionTask != nil || reconnectTask != nil else {
            return
        }
        backgroundTask = UIApplication.shared.beginBackgroundTask(
            withName: Self.backgroundTaskName
        ) { [weak self] in
            self?.suspendConnection()
        }
        if backgroundTask == .invalid {
            suspendConnection()
        }
    }

    private func endBackgroundGrace() {
        guard backgroundTask != .invalid else {
            return
        }
        let identifier = backgroundTask
        backgroundTask = .invalid
        UIApplication.shared.endBackgroundTask(identifier)
    }

    private func suspendConnection() {
        endBackgroundGrace()
        guard !sceneIsActive else {
            return
        }
        reconnectTask?.cancel()
        reconnectTask = nil
        if connectionTask != nil {
            cancelConnectionAttempt()
            connectionState = .disconnected
        }
    }

    private func disconnect(preservingTerminalState: Bool) {
        tearDownConnection(preservingPresentation: false)
        rememberedSessionName = nil
        rememberedPaneID = nil
        pendingNavigation = nil
        navigationCommandSent = false
        reconnectAttempt = 0
        if preservingTerminalState {
            invalidateSentGeometries()
        } else {
            terminalFontSizeSteps = [:]
            terminalGeometries = [:]
        }
    }

    private func tearDownConnection(preservingPresentation: Bool) {
        if preservingPresentation {
            rememberedSessionName = selectedSession?.name ?? rememberedSessionName
            rememberedPaneID = selectedPaneID ?? rememberedPaneID
        }
        connectionAttempt &+= 1
        reconnectTask?.cancel()
        reconnectTask = nil
        connectionPromptBroker?.cancel()
        connectionPromptBroker = nil
        sshPrompt = nil
        connectionTask?.cancel()
        connectionTask = nil
        sessionCreationTimeout?.cancel()
        sessionCreationTimeout = nil
        pendingSessionIDs = nil
        attachedSessionID = nil
        pendingAttachmentSessionID = nil
        hasEstablishedAttachment = false
        isCreatingSession = false
        commandRequests = ZZCommandRequests()
        eventSource?.cancel()
        eventSource = nil
        if let client {
            zz_client_free(client)
        }
        client = nil
        if preservingPresentation {
            invalidateSentGeometries()
        } else {
            sessions = []
            clearFrameSlots()
            agentStates = [:]
            agentDrafts = ZZAgentDrafts()
            agentThreadSlots = [:]
            agentSessionLists = [:]
            unseenAgentCompletions = []
            selectedSessionID = nil
            selectedPaneID = nil
            terminalInput = TerminalInputState()
        }
        terminalModifierState.reset()
        prefixArmed = false
        prefixBindings = []
        tmuxImportTask?.cancel()
        tmuxImportTask = nil
        tmuxImportPhase = .hidden
        connectionState = .idle
    }

    private func cancelConnectionAttempt() {
        connectionAttempt &+= 1
        connectionPromptBroker?.cancel()
        connectionPromptBroker = nil
        sshPrompt = nil
        connectionTask?.cancel()
        connectionTask = nil
    }

    private var localSocket: String? {
        ProcessInfo.processInfo.environment["ZZ_SOCKET"].flatMap { $0.isEmpty ? nil : $0 }
    }

    private func beginConnection(
        endpoint: String,
        password: String?,
        savesHost: Bool,
        returnsToHostSetup: Bool,
        reconnecting: Bool
    ) {
        guard client == nil, connectionTask == nil else {
            return
        }
        reconnectTask?.cancel()
        reconnectTask = nil
        connectionEndpoint = endpoint
        connectionSavesHost = savesHost
        connectionReturnsToHostSetup = returnsToHostSetup
        let reconnectError: String?
        if case let .reconnecting(_, _, error) = connectionState {
            reconnectError = error
        } else {
            reconnectError = nil
        }
        connectionState = reconnecting
            ? .reconnecting(attempt: max(reconnectAttempt, 1), delay: 0, error: reconnectError)
            : .connecting
        connectionAttempt &+= 1
        let attempt = connectionAttempt
        let broker = ZZSSHPromptBroker(initialSecret: password) { [weak self] prompt in
            Task { @MainActor [weak self] in
                guard let self, self.connectionAttempt == attempt else {
                    return
                }
                self.sshPrompt = prompt
            }
        }
        connectionPromptBroker = broker
        connectionTask = Task { [weak self] in
            let result = await Task.detached(priority: .userInitiated) {
                Self.openClient(endpoint: endpoint, broker: broker)
            }.value
            guard let self else {
                result.release()
                return
            }
            guard !Task.isCancelled, self.connectionAttempt == attempt else {
                result.release()
                return
            }
            self.connectionTask = nil
            self.connectionPromptBroker = nil
            self.sshPrompt = nil
            self.finishConnection(
                result,
                endpoint: endpoint,
                savesHost: savesHost,
                returnsToHostSetup: returnsToHostSetup
            )
        }
    }

    private func finishConnection(
        _ result: ZZClientConnectionResult,
        endpoint: String,
        savesHost: Bool,
        returnsToHostSetup: Bool
    ) {
        guard let connected = result.client else {
            let message = result.error.isEmpty
                ? "zz couldn’t connect to \(endpoint)."
                : result.error
            if result.failure.shouldRetry {
                scheduleReconnect(message: message)
            } else if returnsToHostSetup {
                hostEndpoint = endpoint
                presentHostSetup(message: message)
            } else {
                connectionState = .failed(message)
            }
            return
        }
        let fileDescriptor = zz_client_event_fd(connected)
        guard fileDescriptor >= 0 else {
            zz_client_free(connected)
            if returnsToHostSetup {
                presentHostSetup(message: "The zz client opened without an event channel.")
            } else {
                connectionState = .failed("The zz client opened without an event channel.")
            }
            return
        }
        client = connected
        let source = DispatchSource.makeReadSource(fileDescriptor: fileDescriptor, queue: .main)
        source.setEventHandler { [weak self] in
            self?.drainEvents()
        }
        source.resume()
        eventSource = source
        if savesHost {
            hostEndpoint = endpoint
            UserDefaults.standard.set(endpoint, forKey: Self.savedHostKey)
        }
        reconnectAttempt = 0
        connectionState = .connected
        if pendingNavigation == nil, let rememberedPaneID {
            pendingNavigation = ZZNavigationTarget(session: nil, pane: rememberedPaneID)
        }
        let attachment = rememberedSessionName ?? ""
        _ = attachment.withCString { zz_client_attach(connected, $0) }
        drainEvents()
        refreshSnapshot()
    }

    private func scheduleReconnect(message: String) {
        guard let endpoint = connectionEndpoint else {
            connectionState = .failed(message)
            return
        }
        let thawing = isWithinThawGrace
        reconnectAttempt = ZZReconnectPolicy.nextAttempt(
            after: reconnectAttempt,
            thawing: thawing
        )
        let delay = ZZReconnectPolicy.delaySeconds(
            attempt: reconnectAttempt,
            thawing: thawing
        )
        connectionState = .reconnecting(attempt: reconnectAttempt, delay: delay, error: message)
        guard sceneIsActive, networkAvailable else {
            return
        }
        reconnectTask?.cancel()
        reconnectTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(delay))
            guard !Task.isCancelled, let self, self.sceneIsActive,
                  self.networkAvailable,
                  self.client == nil, self.connectionTask == nil else {
                return
            }
            self.reconnectTask = nil
            self.beginConnection(
                endpoint: endpoint,
                password: nil,
                savesHost: self.connectionSavesHost,
                returnsToHostSetup: self.connectionReturnsToHostSetup,
                reconnecting: true
            )
        }
    }

    private func networkPathChanged(_ available: Bool) {
        let restored = available && !networkAvailable
        networkAvailable = available
        guard restored, sceneIsActive, client == nil, connectionTask == nil,
              case .reconnecting = connectionState,
              let endpoint = connectionEndpoint else {
            return
        }
        reconnectTask?.cancel()
        reconnectTask = nil
        beginConnection(
            endpoint: endpoint,
            password: nil,
            savesHost: connectionSavesHost,
            returnsToHostSetup: connectionReturnsToHostSetup,
            reconnecting: true
        )
    }

    func respondToSSHPrompt(_ answer: ZZSSHPromptAnswer) {
        guard let prompt = sshPrompt else {
            return
        }
        sshPrompt = nil
        connectionPromptBroker?.respond(to: prompt.id, with: answer)
    }

    private func presentHostSetup(message: String?) {
        connectionState = .needsHost(message)
        loadSSHPublicKey()
    }

    private func loadSSHPublicKey() {
        guard sshPublicKey == nil, publicKeyTask == nil else {
            return
        }
        publicKeyTask = Task { [weak self] in
            let publicKey = await Task.detached(priority: .utility) {
                Self.readSSHPublicKey()
            }.value
            guard let self, !Task.isCancelled else {
                return
            }
            self.publicKeyTask = nil
            self.sshPublicKey = publicKey
        }
    }

    private nonisolated static func openClient(
        endpoint: String,
        broker: ZZSSHPromptBroker
    ) -> ZZClientConnectionResult {
        var errorBuffer = [CChar](repeating: 0, count: 2_048)
        var rawFailure = ZZ_CONNECT_FAILURE_NONE
        let retainedBroker = Unmanaged.passRetained(broker)
        defer { retainedBroker.release() }
        let connected = errorBuffer.withUnsafeMutableBufferPointer { error in
            endpoint.withCString { endpoint in
                zz_client_connect_endpoint_interactive(
                    endpoint,
                    zzSSHPromptCallback,
                    retainedBroker.toOpaque(),
                    &rawFailure,
                    error.baseAddress,
                    error.count
                )
            }
        }
        let message = errorBuffer.withUnsafeBufferPointer { error in
            error.baseAddress.map(String.init(cString:)) ?? ""
        }
        return ZZClientConnectionResult(
            client: connected,
            error: message,
            failure: ZZConnectFailure(rawValue: UInt32(rawFailure.rawValue)) ?? .configuration
        )
    }

    private nonisolated static func readSSHPublicKey() -> String? {
        let length = zz_client_ssh_public_key(nil, 0)
        guard length > 0 else {
            return nil
        }
        var buffer = [CChar](repeating: 0, count: length + 1)
        let returned = buffer.withUnsafeMutableBufferPointer { buffer in
            zz_client_ssh_public_key(buffer.baseAddress, buffer.count)
        }
        guard returned > 0 else {
            return nil
        }
        return buffer.withUnsafeBufferPointer { buffer in
            buffer.baseAddress.map(String.init(cString:))
        }
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
        terminalModifierState.reset()
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
        terminalModifierState.reset()
        selectedPaneID = pane.id
        unseenAgentCompletions.remove(pane.id)
        agentNotifications.clear(pane: pane.id)
        if pane.kind == .terminal {
            acquireTerminalInput(pane.id)
        } else {
            releaseTerminalInput()
        }
    }

    func selectPane(_ pane: ZZPane, in session: ZZSession) {
        guard attachedSessionID == session.id,
              session.panes.contains(where: { $0.id == pane.id }) else {
            open(ZZNavigationTarget(session: session.id, pane: pane.id))
            return
        }
        if !pane.isActive,
           !execute("select-pane", args: ["-t", "%\(pane.id)"]) {
            actionError = "zz couldn’t select that pane."
            return
        }
        openPane(pane)
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
        terminalModifierState.reset()
    }

    func setTerminalPreview(_ enabled: Bool) {
        guard terminalPreviewRequested != enabled else {
            return
        }
        terminalPreviewRequested = enabled
        if let client, attachedSessionID != nil {
            _ = zz_client_set_terminal_preview(client, enabled)
        }
        if !enabled {
            let foregroundPanes = Set(
                sessions
                    .first(where: \.isAttached)?
                    .panes
                    .filter { $0.kind == .terminal && $0.layout != nil }
                    .map(\.id) ?? []
            )
            for (pane, slot) in frameSlots where !foregroundPanes.contains(pane) {
                slot.update(nil)
            }
        }
    }

    func requestKeyboard(for paneID: UInt64) {
        guard let session = selectedSession,
              let pane = session.panes.first(where: {
                  $0.id == paneID && $0.kind == .terminal
              }) else {
            return
        }
        selectPane(pane, in: session)
    }

    func frame(for pane: UInt64) -> TerminalFrame? {
        frameSlots[pane]?.frame
    }

    func frameSlot(for pane: UInt64) -> TerminalFrameSlot {
        if let slot = frameSlots[pane] {
            return slot
        }
        let slot = TerminalFrameSlot()
        frameSlots[pane] = slot
        return slot
    }

    func terminalFontSizeStep(for pane: UInt64) -> Int {
        terminalFontSizeSteps[pane] ?? 0
    }

    func setTerminalFontSizeStep(_ step: Int, for pane: UInt64) {
        guard terminalFontSizeSteps[pane] != step else {
            return
        }
        terminalFontSizeSteps[pane] = step
    }

    func sendText(_ text: String, to pane: UInt64) {
        guard let client, !text.isEmpty else {
            return
        }
        if terminalModifierState.active != 0,
           text.unicodeScalars.count == 1,
           let scalar = text.unicodeScalars.first {
            sendKey(
                UInt32(ZZ_KEY_CHARACTER.rawValue),
                to: pane,
                codepoint: scalar.value,
                modifiers: 0
            )
            return
        }
        _ = text.withCString { zz_client_send_text(client, pane, $0) }
        terminalModifierState.consumeOneShot()
    }

    /// Pasting is not typing: the daemon wraps this text for bracketed-paste
    /// mode, translates its newlines, and keeps it away from the key tables,
    /// so a multi-line paste cannot run as a sequence of commands.
    func paste(_ text: String, to pane: UInt64) {
        guard let client, !text.isEmpty else {
            return
        }
        _ = text.withCString { zz_client_paste(client, pane, $0) }
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
        let combinedModifiers = modifiers | terminalModifierState.active
        _ = zz_client_send_key(
            client,
            pane,
            code,
            codepoint,
            function,
            action,
            combinedModifiers,
            nil,
            textFollows
        )
        if action != UInt32(ZZ_KEY_RELEASE.rawValue) {
            terminalModifierState.consumeOneShot()
        }
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
        terminalModifierState.reset()
    }

    /// Run one daemon command typed into the command-prompt sheet. The first
    /// whitespace-separated token names the command; the rest are arguments.
    /// Returns false when the line is blank or no client is connected.
    @discardableResult
    func submitCommand(_ line: String) -> Bool {
        guard let parsed = ZZCommandLine.split(line) else {
            return false
        }
        return execute(parsed.name, args: parsed.args)
    }

    /// Ask the daemon for its full key list (`list-keys`). The daemon answers
    /// through command output, which iOS cannot render yet (see the key-list
    /// sheet): the published `prefixBindings` below cover the prefix table.
    @discardableResult
    func requestKeyList() -> Bool {
        execute("list-keys", args: [])
    }

    func maybeOfferTmuxImport() {
        guard tmuxImportPhase == .hidden else {
            return
        }
        let offered = ZZTMuxImport.offeredHosts(in: .standard)
        guard ZZTMuxImport.shouldOffer(endpoint: hostEndpoint, offered: offered) else {
            return
        }
        tmuxImportPhase = .prompting(endpoint: hostEndpoint)
    }

    func declineTmuxImport() {
        guard case .prompting(let endpoint) = tmuxImportPhase else {
            return
        }
        ZZTMuxImport.markOffered(endpoint: endpoint, in: .standard)
        tmuxImportPhase = .hidden
    }

    func runTmuxImportManually() {
        ZZTMuxImport.markOffered(endpoint: hostEndpoint, in: .standard)
        beginTmuxImport()
    }

    func dismissTmuxImport() {
        tmuxImportTask?.cancel()
        tmuxImportTask = nil
        tmuxImportPhase = .hidden
    }

    func acknowledgeTmuxImport() {
        guard tmuxImportPhase.needsAlert else {
            return
        }
        if tmuxImportPhase.promptEndpoint != nil {
            declineTmuxImport()
        } else {
            dismissTmuxImport()
        }
    }

    private func beginTmuxImport() {
        tmuxImportTask?.cancel()
        let baseline = prefixBindings
        _ = execute("import-tmux-config", args: [])
        tmuxImportPhase = .working(baseline: baseline)
        tmuxImportTask = Task { @MainActor [weak self] in
            do {
                try await Task.sleep(nanoseconds: ZZTMuxImport.settleDelayNanoseconds)
            } catch {
                return
            }
            self?.settleTmuxImportUnchanged()
        }
    }

    private func settleTmuxImport(after current: [ZZPrefixBinding]) {
        guard case .working(let baseline) = tmuxImportPhase else {
            return
        }
        guard let message = ZZTMuxImport.resultMessage(baseline: baseline, current: current) else {
            return
        }
        tmuxImportTask?.cancel()
        tmuxImportTask = nil
        tmuxImportPhase = .done(message: message)
    }

    private func settleTmuxImportUnchanged() {
        guard case .working = tmuxImportPhase else {
            return
        }
        tmuxImportTask = nil
        tmuxImportPhase = .done(message: ZZTMuxImport.unchangedMessage)
    }

    /// Open the daemon's `choose-buffer` overlay. Interactive selection needs
    /// `ChooseBufferState` FFI that does not exist yet; until then the daemon
    /// overlay is driven from the attached terminal like on desktop.
    @discardableResult
    func requestChooseBuffer() -> Bool {
        execute("choose-buffer", args: ZZCommandLine.chooseBufferArgs)
    }

    /// Flash the daemon's `display-panes` overlay. Interactive selection needs
    /// `DisplayPanesState` FFI that does not exist yet; until then the overlay
    /// is driven from the attached terminal like on desktop.
    @discardableResult
    func requestDisplayPanes() -> Bool {
        execute("display-panes", args: ZZCommandLine.displayPanesArgs)
    }

    private func refreshPrefixState() {
        guard let client,
              let snapshot = zz_prefix_snapshot_acquire(client) else {
            return
        }
        defer { zz_prefix_snapshot_release(snapshot) }
        prefixArmed = zz_prefix_snapshot_armed(snapshot)
        let count = Int(zz_prefix_binding_count(snapshot))
        var next: [ZZPrefixBinding] = []
        next.reserveCapacity(count)
        for index in 0..<count {
            next.append(
                ZZPrefixBinding(
                    key: string(zz_prefix_binding_key(snapshot, index)),
                    summary: string(zz_prefix_binding_summary(snapshot, index)),
                    note: string(zz_prefix_binding_note(snapshot, index)),
                    repeats: zz_prefix_binding_repeat(snapshot, index)
                )
            )
        }
        prefixBindings = next
        settleTmuxImport(after: next)
    }

    func sendShortcutKey(_ code: UInt32, to pane: UInt64) {
        sendKey(code, to: pane)
    }

    func toggleControlModifier() {
        terminalModifierState.tap(Self.controlModifier, at: Date.timeIntervalSinceReferenceDate)
    }

    func toggleAltModifier() {
        terminalModifierState.tap(Self.altModifier, at: Date.timeIntervalSinceReferenceDate)
    }

    func toggleShiftModifier() {
        terminalModifierState.tap(Self.shiftModifier, at: Date.timeIntervalSinceReferenceDate)
    }

    func updateSelection(
        pane: UInt64,
        phase: UInt32,
        column: UInt16,
        row: UInt16,
        clickCount: UInt8 = 1,
        rectangle: Bool = false
    ) {
        guard let client else {
            return
        }
        _ = zz_client_terminal_selection(
            client,
            pane,
            phase,
            column,
            row,
            clickCount,
            rectangle
        )
    }

    func copySelection(pane: UInt64) {
        guard let client else {
            return
        }
        let request = clipboardRequestID
        clipboardRequestID &+= 1
        _ = zz_client_copy_selection(client, pane, request)
    }

    /// Ask the daemon for the pane's last OSC 133 command block and put it on
    /// the clipboard once the reply lands.
    func copyLastOutput(pane: UInt64) {
        let request = executeRequest("show-last-output", args: ZZLastOutput.arguments(pane: pane))
        guard request != 0 else {
            post(.failure, "zz couldn’t ask for that pane’s last output.")
            return
        }
        commandRequests.register(request, as: .lastOutput(pane: pane))
    }

    /// Any command this client runs that prints something opens a daemon-side
    /// output view, which switches the client to the copy-mode key table and
    /// swallows its terminal input until the view is dismissed. This client
    /// never renders that view, so a pane would go silently deaf. Escape is
    /// what leaves it, and a reply carrying text is proof one was opened.
    private func dismissCommandOutputView(pane: UInt64) {
        sendShortcutKey(UInt32(ZZ_KEY_ESCAPE.rawValue), to: pane)
    }

    func dismissNotice() {
        noticeDismissal?.cancel()
        noticeDismissal = nil
        actionNotice = nil
    }

    private func post(_ tone: ZZActionNotice.Tone, _ message: String) {
        noticeSequence &+= 1
        let notice = ZZActionNotice(id: noticeSequence, tone: tone, message: message)
        actionNotice = notice
        noticeDismissal?.cancel()
        noticeDismissal = Task { [weak self] in
            try? await Task.sleep(for: .seconds(tone.seconds))
            guard !Task.isCancelled else {
                return
            }
            guard let self, actionNotice?.id == notice.id else {
                return
            }
            actionNotice = nil
        }
    }

    func agentState(for pane: UInt64) -> ZZAgentState? {
        agentStates[pane]
    }

    func agentDraft(for pane: UInt64) -> String {
        agentDrafts.text(for: pane)
    }

    func saveAgentDraft(_ text: String, for pane: UInt64) {
        agentDrafts.save(text, for: pane)
    }

    /// The pane's transcript slot. Views observe this instead of the store so
    /// a streamed batch redraws one pane, not the whole workspace.
    func agentThreadSlot(for pane: UInt64) -> ZZAgentThreadSlot {
        if let slot = agentThreadSlots[pane] {
            return slot
        }
        let slot = ZZAgentThreadSlot()
        agentThreadSlots[pane] = slot
        return slot
    }

    func agentThread(for pane: UInt64) -> ZZAgentThread {
        agentThreadSlots[pane]?.thread ?? ZZAgentThread()
    }

    func ensureAgentStream(for pane: UInt64) {
        _ = agentThreadSlot(for: pane)
        requestAgentReplay(for: pane)
    }

    func requestAgentReplay(for pane: UInt64) {
        guard let client else {
            return
        }
        let slot = agentThreadSlot(for: pane)
        guard !slot.thread.replayPending else {
            return
        }
        guard zz_client_agent_replay(client, pane, slot.thread.cursor) else {
            return
        }
        slot.mutate { $0.markReplayPending() }
    }

    func drainAgentUpdates() {
        guard let client else {
            return
        }
        while let batch = zz_client_agent_updates_next(client) {
            defer { zz_agent_updates_release(batch) }
            let pane = zz_agent_updates_pane(batch)
            let firstSeq = zz_agent_updates_first_seq(batch)
            let count = Int(zz_agent_updates_item_count(batch))
            var items: [Data] = []
            items.reserveCapacity(count)
            for index in 0..<count {
                let bytes = zz_agent_updates_item_bytes(batch, index)
                guard let pointer = bytes.ptr, bytes.len > 0 else {
                    continue
                }
                items.append(Data(buffer: UnsafeBufferPointer(start: pointer, count: bytes.len)))
            }
            let effect = agentThreadSlot(for: pane).mutate {
                $0.applyBatch(firstSeq: firstSeq, items: items)
            }
            if effect == .needsReplay {
                requestAgentReplay(for: pane)
            }
        }
    }

    func drainAgentLagged() {
        guard let client else {
            return
        }
        var pane: UInt64 = 0
        var nextSeq: UInt64 = 0
        while zz_client_agent_lagged_next(client, &pane, &nextSeq) {
            requestAgentReplay(for: pane)
        }
    }

    func agentSessionList(for pane: UInt64) -> ZZAgentSessionList {
        agentSessionLists[pane] ?? ZZAgentSessionList()
    }

    func ensureAgentSessions(for pane: UInt64) {
        guard agentSessionLists[pane] == nil else {
            return
        }
        listAgentSessions(pane: pane)
    }

    func listAgentSessions(pane: UInt64) {
        guard let client else {
            return
        }
        var list = agentSessionLists[pane] ?? ZZAgentSessionList()
        list.loading = true
        list.error = nil
        agentSessionLists[pane] = list
        if !zz_client_agent_list_sessions(client, pane) {
            var failed = agentSessionLists[pane] ?? ZZAgentSessionList()
            failed.loading = false
            failed.error = "zz couldn’t load those sessions."
            agentSessionLists[pane] = failed
        }
    }

    func drainAgentSessions() {
        guard let client else {
            return
        }
        while let reply = zz_client_agent_sessions_next(client) {
            defer { zz_agent_sessions_release(reply) }
            let pane = zz_agent_sessions_pane(reply)
            let bytes = zz_agent_sessions_result(reply)
            guard let pointer = bytes.ptr, bytes.len > 0 else {
                continue
            }
            let data = Data(buffer: UnsafeBufferPointer(start: pointer, count: bytes.len))
            var list = agentSessionLists[pane] ?? ZZAgentSessionList()
            list.loading = false
            if let sessions = ZZAgentSessionSummary.parseList(data) {
                list.sessions = sessions
                list.error = nil
            } else if let message = ZZAgentSessionSummary.parseListFailure(data) {
                list.error = message
            } else {
                list.error = "zz couldn’t read those sessions."
            }
            agentSessionLists[pane] = list
        }
    }

    func setAgentConfigOption(pane: UInt64, option: String, value: String) {
        guard let client else {
            actionError = "zz couldn’t change that setting."
            return
        }
        let ok = option.withCString { optionPointer in
            value.withCString { valuePointer in
                zz_client_agent_set_config_option(client, pane, optionPointer, valuePointer)
            }
        }
        if !ok {
            actionError = "zz couldn’t change that setting."
        }
    }

    func setAgentMode(pane: UInt64, mode: String) {
        guard let client else {
            actionError = "zz couldn’t change that mode."
            return
        }
        let ok = mode.withCString { modePointer in
            zz_client_agent_set_mode(client, pane, modePointer)
        }
        if !ok {
            actionError = "zz couldn’t change that mode."
        }
    }

    func startAgentSession(pane: UInt64, cwd: String) {
        let path = cwd.trimmingCharacters(in: .whitespacesAndNewlines)
        guard path.hasPrefix("/") else {
            actionError = "Use an absolute path for the new working directory."
            return
        }
        guard let client else {
            actionError = "zz couldn’t start a session there."
            return
        }
        let ok = path.withCString { pathPointer in
            zz_client_agent_new_session(client, pane, pathPointer)
        }
        if !ok {
            actionError = "zz couldn’t start a session there."
        }
    }

    func switchAgentSession(pane: UInt64, session: ZZAgentSessionSummary) {
        guard let client else {
            actionError = "zz couldn’t switch to that session."
            return
        }
        let ok = session.sessionID.withCString { sessionPointer in
            session.cwd.withCString { cwdPointer in
                session.additionalDirectoriesJSON().withCString { dirsPointer in
                    zz_client_agent_switch_session(
                        client,
                        pane,
                        sessionPointer,
                        cwdPointer,
                        dirsPointer
                    )
                }
            }
        }
        if !ok {
            actionError = "zz couldn’t switch to that session."
        }
    }

    func deleteAgentSession(pane: UInt64, session: ZZAgentSessionSummary) {
        guard let client else {
            actionError = "zz couldn’t delete that session."
            return
        }
        let ok = session.sessionID.withCString { sessionPointer in
            zz_client_agent_delete_session(client, pane, sessionPointer)
        }
        if !ok {
            actionError = "zz couldn’t delete that session."
            return
        }
        listAgentSessions(pane: pane)
    }

    func primeAgentState(for pane: UInt64) {
        guard agentStates[pane] == nil else {
            return
        }
        refreshAgentState(pane: pane, flags: 0)
    }

    @discardableResult
    func submitAgentPrompt(_ text: String, pane: UInt64) -> Bool {
        guard let args = ZZAgentPromptCommand.arguments(pane: pane, text: text) else {
            return false
        }
        guard execute("agent-send", args: args) else {
            actionError = "zz couldn’t send that Agent prompt."
            return false
        }
        agentDrafts.remove(pane: pane)
        agentThreadSlot(for: pane).mutate { $0.appendUserTurn(text) }
        return true
    }

    func respondToPermission(pane: UInt64, request: UInt64, option: String?) {
        guard let client else {
            return
        }
        let sent = if let option {
            option.withCString {
                zz_client_agent_respond_permission(client, pane, request, $0)
            }
        } else {
            zz_client_agent_respond_permission(client, pane, request, nil)
        }
        if !sent {
            actionError = "zz couldn’t send that approval response."
        }
    }

    func cancelAgent(pane: UInt64) {
        guard let client, zz_client_agent_cancel(client, pane) else {
            actionError = "zz couldn’t cancel that Agent turn."
            return
        }
    }

    func open(_ url: URL) {
        guard let target = ZZNavigationTarget(url: url) else {
            return
        }
        if target.attention {
            openHighestAttention()
        } else {
            open(target)
        }
    }

    func open(_ target: ZZNavigationTarget) {
        pendingNavigation = target
        navigationCommandSent = false
        resolvePendingNavigation()
    }

    func openHighestAttention() {
        guard let attention = agentAttention.first else {
            return
        }
        open(ZZNavigationTarget(session: attention.session, pane: attention.pane))
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

    func newPane(kind: ZZPaneKind = .terminal) {
        guard let session = selectedSession else {
            actionError = "Select a session before creating a pane."
            return
        }
        let terminal = session.panes.first { $0.isActive && $0.kind == .terminal }
            ?? session.panes.first { $0.kind == .terminal }
        let created: Bool
        switch kind {
        case .terminal:
            created = if let terminal {
                execute("split-window", args: ["-t", "%\(terminal.id)"])
            } else {
                execute("new-window", args: [])
            }
        case .agent:
            guard let target = terminal?.id
                ?? session.activeWindow?.activePane
                ?? session.panes.first?.id else {
                actionError = "Create a terminal before adding an Agent pane."
                return
            }
            created = execute(
                "if-shell",
                args: [
                    "-F",
                    "1",
                    "split-picker -t %\(target) ; select-pane-kind agent",
                ]
            )
        case .picker, .browser, .editor:
            actionError = "That pane type isn’t available in the iPad app yet."
            return
        }
        if !created {
            actionError = "zz couldn’t create that pane."
        }
    }

    func closePane(_ pane: UInt64) {
        if !execute("kill-pane", args: ["-t", "%\(pane)"]) {
            actionError = "zz couldn’t close that pane."
        }
    }

    func closeWindow(_ window: UInt64) {
        if !execute("kill-window", args: ["-t", "@\(window)"]) {
            actionError = "zz couldn’t close that window."
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
        executeRequest(name, args: args) != 0
    }

    /// The request id the daemon's reply will carry, or zero when the command
    /// could not be sent.
    private func executeRequest(_ name: String, args: [String]) -> UInt64 {
        guard let client else {
            return 0
        }
        let allocated = args.map { argument in
            argument.withCString { strdup($0) }
        }
        guard allocated.allSatisfy({ $0 != nil }) else {
            allocated.forEach { free($0) }
            return 0
        }
        defer { allocated.forEach { free($0) } }
        let pointers = allocated.map { pointer in
            pointer.map { UnsafePointer<CChar>($0) }
        }
        return name.withCString { namePointer in
            pointers.withUnsafeBufferPointer { arguments in
                zz_client_execute_request(
                    client,
                    namePointer,
                    arguments.baseAddress,
                    arguments.count
                )
            }
        }
    }

    private func drainEvents() {
        guard let client else {
            return
        }
        var event = zz_client_event()
        var refreshMux = false
        var disconnected = false
        var replacingInputPane: UInt64?
        while !disconnected, zz_client_next_event(client, &event) {
            switch event.kind {
            case ZZ_EVENT_HELLO:
                connectionState = .connected
            case ZZ_EVENT_ATTACHED:
                _ = zz_client_set_focused(client, sceneIsActive)
                if terminalPreviewRequested {
                    _ = zz_client_set_terminal_preview(client, true)
                }
                agentStates = [:]
                unseenAgentCompletions = []
                navigationCommandSent = false
                refreshMux = true
                maybeOfferTmuxImport()
                for window in sessions.flatMap(\.windows) {
                    for pane in window.panes where pane.kind == .agent {
                        agentThreadSlot(for: pane.id).mutate { $0.resetStream() }
                        requestAgentReplay(for: pane.id)
                    }
                }
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
                removeFrameSlot(for: event.pane)
                terminalGeometries.removeValue(forKey: event.pane)
                terminalFontSizeSteps.removeValue(forKey: event.pane)
                agentStates.removeValue(forKey: event.pane)
                agentDrafts.remove(pane: event.pane)
                agentThreadSlots.removeValue(forKey: event.pane)
                agentSessionLists.removeValue(forKey: event.pane)
                unseenAgentCompletions.remove(event.pane)
                agentNotifications.clear(pane: event.pane)
                if terminalInput.owner.owns(event.pane) {
                    replacingInputPane = event.pane
                    terminalInput.release()
                }
                refreshMux = true
            case ZZ_EVENT_DETACHED:
                invalidateSentGeometries()
                refreshMux = true
            case ZZ_EVENT_AGENT_STATE_CHANGED:
                refreshAgentState(pane: event.pane, flags: event.flags)
            case ZZ_EVENT_AGENT_UPDATES:
                drainAgentUpdates()
            case ZZ_EVENT_AGENT_LAGGED:
                drainAgentLagged()
            case ZZ_EVENT_AGENT_SESSIONS:
                drainAgentSessions()
            case ZZ_EVENT_CLIPBOARD:
                drainClipboard(client)
            case ZZ_EVENT_COMMAND_REPLY:
                drainCommandReplies(client)
            case ZZ_EVENT_PREFIX_ARMED,
                 ZZ_EVENT_KEY_TABLES_CHANGED,
                 ZZ_EVENT_COMMAND_PROMPT_CHANGED,
                 ZZ_EVENT_CHOOSE_BUFFER_CHANGED,
                 ZZ_EVENT_DISPLAY_PANES_CHANGED:
                refreshPrefixState()
            case ZZ_EVENT_SERVER_STOPPING, ZZ_EVENT_DISCONNECTED:
                disconnected = true
            default:
                break
            }
        }
        if disconnected {
            handleUnexpectedDisconnect()
            return
        }
        if refreshMux {
            refreshSnapshot(replacingInputFor: replacingInputPane)
        }
    }

    private func refreshSnapshot(replacingInputFor replacingPane: UInt64? = nil) {
        guard let client, let snapshot = zz_client_snapshot_acquire(client) else {
            return
        }
        defer { zz_snapshot_release(snapshot) }

        let sessionCount = Int(zz_snapshot_session_count(snapshot))
        var nextSessions: [ZZSession] = []
        nextSessions.reserveCapacity(sessionCount)
        for sessionIndex in 0..<sessionCount {
            let windowCount = Int(zz_snapshot_session_window_count(snapshot, sessionIndex))
            var windows: [ZZWindow] = []
            windows.reserveCapacity(windowCount)
            for windowIndex in 0..<windowCount {
                let paneCount = Int(
                    zz_snapshot_session_window_pane_count(snapshot, sessionIndex, windowIndex)
                )
                var panes: [ZZPane] = []
                panes.reserveCapacity(paneCount)
                for paneIndex in 0..<paneCount {
                    let rawKind = zz_snapshot_session_window_pane_kind(
                        snapshot,
                        sessionIndex,
                        windowIndex,
                        paneIndex
                    )
                    var rawLayout = zz_pane_rect()
                    let layout = zz_snapshot_session_window_pane_rect(
                        snapshot,
                        sessionIndex,
                        windowIndex,
                        paneIndex,
                        &rawLayout
                    )
                        ? ZZPaneLayout(
                            x: rawLayout.x,
                            y: rawLayout.y,
                            width: rawLayout.width,
                            height: rawLayout.height
                        )
                        : nil
                    panes.append(
                        ZZPane(
                            id: zz_snapshot_session_window_pane_id(
                                snapshot,
                                sessionIndex,
                                windowIndex,
                                paneIndex
                            ),
                            title: string(
                                zz_snapshot_session_window_pane_title(
                                    snapshot,
                                    sessionIndex,
                                    windowIndex,
                                    paneIndex
                                )
                            ),
                            kind: ZZPaneKind(rawValue: UInt32(rawKind.rawValue)) ?? .picker,
                            isActive: zz_snapshot_session_window_pane_is_active(
                                snapshot,
                                sessionIndex,
                                windowIndex,
                                paneIndex
                            ),
                            hasBell: zz_snapshot_session_window_pane_has_bell(
                                snapshot,
                                sessionIndex,
                                windowIndex,
                                paneIndex
                            ),
                            layout: layout
                        )
                    )
                }
                var rawZoomedPane: UInt64 = 0
                let zoomedPane = zz_snapshot_session_window_zoomed_pane(
                    snapshot,
                    sessionIndex,
                    windowIndex,
                    &rawZoomedPane
                ) ? rawZoomedPane : nil
                windows.append(
                    ZZWindow(
                        id: zz_snapshot_session_window_id(snapshot, sessionIndex, windowIndex),
                        index: zz_snapshot_session_window_index(snapshot, sessionIndex, windowIndex),
                        name: string(
                            zz_snapshot_session_window_name(snapshot, sessionIndex, windowIndex)
                        ),
                        isCurrent: zz_snapshot_session_window_is_current(
                            snapshot,
                            sessionIndex,
                            windowIndex
                        ),
                        activePane: zz_snapshot_session_window_active_pane(
                            snapshot,
                            sessionIndex,
                            windowIndex
                        ),
                        zoomedPane: zoomedPane,
                        panes: panes
                    )
                )
            }
            nextSessions.append(
                ZZSession(
                    id: zz_snapshot_session_id(snapshot, sessionIndex),
                    name: string(zz_snapshot_session_name(snapshot, sessionIndex)),
                    activeWindowID: zz_snapshot_session_active_window(snapshot, sessionIndex),
                    windows: windows,
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
        if let targetPaneID = terminalInput.snapshotTarget(
            selectedPane: selectedPaneID,
            activePane: attached?.activeWindow?.activePane,
            replacingPane: replacingPane,
            navigationPending: pendingNavigation != nil || pendingAttachmentSessionID != nil
        ), let targetPane = attached?.panes.first(where: { $0.id == targetPaneID }) {
            openPane(targetPane)
        }
        if let selectedPaneID,
           !nextSessions.lazy.flatMap(\.panes).contains(where: { $0.id == selectedPaneID }) {
            self.selectedPaneID = nil
            if terminalInput.owner.owns(selectedPaneID) {
                terminalInput.release()
            }
        }

        let foregroundPanes = Set(
            nextSessions
                .first(where: \.isAttached)?
                .panes
                .filter { $0.kind == .terminal && $0.layout != nil }
                .map(\.id) ?? []
        )
        let previewPanes = terminalPreviewRequested
            ? Set(
                attached?.allPanes
                    .filter { $0.kind == .terminal }
                    .map(\.id) ?? []
            )
            : []
        let retainedPanes = foregroundPanes.union(previewPanes)
        let knownTerminalPanes = Set(
            nextSessions
                .flatMap(\.allPanes)
                .filter { $0.kind == .terminal }
                .map(\.id)
        )
        terminalGeometries = terminalGeometries.filter { knownTerminalPanes.contains($0.key) }
        terminalFontSizeSteps = terminalFontSizeSteps.filter { knownTerminalPanes.contains($0.key) }
        pruneFrameSlots(
            keepingFramesFor: retainedPanes,
            keepingSlotsFor: knownTerminalPanes
        )
        for pane in foregroundPanes {
            restoreStableGeometry(for: pane, client: client)
        }
        for pane in retainedPanes {
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
        refreshPrefixState()
        resolvePendingNavigation()
    }

    private func handleUnexpectedDisconnect() {
        tearDownConnection(preservingPresentation: true)
        scheduleReconnect(message: "The connection to zz closed.")
    }

    private func refreshAgentState(pane: UInt64, flags: UInt32) {
        guard let client, let snapshot = zz_client_agent_state_acquire(client, pane) else {
            return
        }
        defer { zz_agent_state_release(snapshot) }
        let rawPhase = zz_agent_state_phase(snapshot)
        let rawStatus = zz_agent_attention_status(snapshot)
        let phase = ZZAgentPhase(rawValue: UInt32(rawPhase.rawValue)) ?? .starting
        let status = ZZAgentStatus(rawValue: UInt32(rawStatus.rawValue)) ?? .idle
        let permission: ZZAgentPermission?
        if zz_agent_has_permission(snapshot) {
            let count = Int(zz_agent_permission_option_count(snapshot))
            let options = (0..<count).map { index in
                let rawKind = zz_agent_permission_option_kind(snapshot, index)
                let kind: ZZAgentPermissionKind = switch UInt32(rawKind.rawValue) {
                case 1: .allowOnce
                case 2: .allowAlways
                case 3: .rejectOnce
                case 4: .rejectAlways
                default: .unknown
                }
                return ZZAgentPermissionOption(
                    id: string(zz_agent_permission_option_id(snapshot, index)),
                    name: string(zz_agent_permission_option_name(snapshot, index)),
                    kind: kind
                )
            }
            permission = ZZAgentPermission(
                requestID: zz_agent_permission_request_id(snapshot),
                title: string(zz_agent_permission_title(snapshot)),
                options: options
            )
        } else {
            permission = nil
        }
        let git = zz_agent_has_git(snapshot)
            ? ZZAgentGitSummary(
                branch: optionalString(zz_agent_git_branch(snapshot)),
                changedFiles: zz_agent_git_changed_files(snapshot),
                additions: zz_agent_git_additions(snapshot),
                deletions: zz_agent_git_deletions(snapshot)
            )
            : nil
        let configOptions = blobData(zz_agent_config_options(snapshot))
            .map(ZZAgentConfigOption.parseAll) ?? []
        let modeState = blobData(zz_agent_modes(snapshot)).flatMap(ZZAgentModeState.parse)
        let state = ZZAgentState(
            pane: pane,
            phase: phase,
            status: status,
            queuedPrompts: zz_agent_queued_prompts(snapshot),
            sessionID: optionalString(zz_agent_session_id(snapshot)),
            title: optionalString(zz_agent_title(snapshot)),
            error: optionalString(zz_agent_error(snapshot)),
            permission: permission,
            git: git,
            configOptions: configOptions,
            modeState: modeState
        )
        if let previous = agentStates[pane],
           let nextSession = state.sessionID,
           previous.sessionID != nil,
           previous.sessionID != nextSession,
           let slot = agentThreadSlots[pane] {
            slot.mutate { $0.resetStream() }
        }
        agentStates[pane] = state

        if flags & UInt32(ZZ_EVENT_AGENT_DONE) != 0 {
            agentThreadSlot(for: pane).mutate { $0.settleOldestWorkingTurn(.done) }
        }
        if flags & UInt32(ZZ_EVENT_AGENT_FAILED) != 0 {
            agentThreadSlot(for: pane).mutate { $0.settleOldestWorkingTurn(.failed) }
        }

        let hidden = !sceneIsActive || selectedPaneID != pane
        let session = attachedSessionID
        if flags & UInt32(ZZ_EVENT_AGENT_REQUEST) != 0, permission != nil {
            if hidden, let session {
                notifyAgent(.blocked, state: state, session: session)
            }
        }
        if flags & UInt32(ZZ_EVENT_AGENT_DONE) != 0 {
            if hidden {
                unseenAgentCompletions.insert(pane)
                if let session {
                    notifyAgent(.done, state: state, session: session)
                }
            }
        }
        if flags & UInt32(ZZ_EVENT_AGENT_FAILED) != 0,
           status == .failed, hidden, let session {
            notifyAgent(.failed, state: state, session: session)
        }
    }

    private func notifyAgent(
        _ kind: ZZAgentAttentionKind,
        state: ZZAgentState,
        session: UInt64
    ) {
        agentNotifications.schedule(
            kind: kind,
            pane: state.pane,
            session: session,
            title: state.title ?? paneTitle(state.pane) ?? "zz Agent",
            permission: state.permission?.requestID
        )
    }

    private func drainClipboard(_ client: OpaquePointer) {
        while let clipboard = zz_client_clipboard_next(client) {
            UIPasteboard.general.string = string(zz_clipboard_text(clipboard))
            zz_clipboard_release(clipboard)
        }
    }

    /// Every executed command answers here, so the queue is drained whole and
    /// the untracked replies are discarded. The reply's bytes belong to the
    /// handle, so they are copied into Swift strings before it is released.
    private func drainCommandReplies(_ client: OpaquePointer) {
        while let reply = zz_client_command_reply_next(client) {
            guard let purpose = commandRequests.take(zz_command_reply_request_id(reply)) else {
                zz_command_reply_release(reply)
                continue
            }
            let result = ZZLastOutput.result(
                ok: zz_command_reply_ok(reply),
                output: string(zz_command_reply_output(reply)),
                error: string(zz_command_reply_error(reply))
            )
            zz_command_reply_release(reply)
            switch purpose {
            case let .lastOutput(pane):
                switch result {
                case let .copy(text):
                    UIPasteboard.general.string = text
                    dismissCommandOutputView(pane: pane)
                    post(.success, "Copied the last command’s output.")
                case let .failure(message):
                    post(.failure, message)
                }
            }
        }
    }

    private func resolvePendingNavigation() {
        guard let target = pendingNavigation, client != nil else {
            return
        }
        if let sessionID = target.session, attachedSessionID != sessionID {
            guard let session = sessions.first(where: { $0.id == sessionID }) else {
                return
            }
            if pendingAttachmentSessionID != sessionID {
                selectSession(session)
            }
            return
        }
        guard let paneID = target.pane else {
            pendingNavigation = nil
            showOverview()
            return
        }
        if let session = sessions.first(where: { session in
            session.panes.contains { $0.id == paneID }
        }), let pane = session.panes.first(where: { $0.id == paneID }) {
            pendingNavigation = nil
            navigationCommandSent = false
            rememberedPaneID = nil
            selectPane(pane, in: session)
            return
        }
        guard !navigationCommandSent else {
            return
        }
        navigationCommandSent = true
        let paneTarget = "%\(paneID)"
        if !execute("select-window", args: ["-t", paneTarget])
            || !execute("select-pane", args: ["-t", paneTarget]) {
            navigationCommandSent = false
                actionError = "zz couldn’t open that pane."
        }
    }

    private func consumeShortcutCommand() {
        guard let raw = UserDefaults.standard.string(forKey: ZZShortcutCommand.key),
              let command = ZZShortcutCommand(rawValue: raw) else {
            return
        }
        UserDefaults.standard.removeObject(forKey: ZZShortcutCommand.key)
        switch command {
        case .reconnect: retry()
        case .attention: openHighestAttention()
        }
    }

    private func paneTitle(_ pane: UInt64) -> String? {
        sessions.lazy.flatMap(\.allPanes).first { $0.id == pane }?.title
    }

    private func optionalString(_ bytes: zz_bytes) -> String? {
        let value = string(bytes)
        return value.isEmpty ? nil : value
    }

    private func blobData(_ bytes: zz_bytes) -> Data? {
        guard let pointer = bytes.ptr, bytes.len > 0 else {
            return nil
        }
        return Data(buffer: UnsafeBufferPointer(start: pointer, count: bytes.len))
    }

    private func refreshFrame(pane: UInt64, damage: TerminalDamage) {
        guard let client, let frame = TerminalFrame(client: client, pane: pane, damage: damage) else {
            return
        }
        frameSlot(for: pane).update(frame)
    }

    private func pruneFrameSlots(
        keepingFramesFor attachedPanes: Set<UInt64>,
        keepingSlotsFor knownTerminalPanes: Set<UInt64>
    ) {
        for (pane, slot) in frameSlots where !attachedPanes.contains(pane) {
            slot.update(nil)
        }
        frameSlots = frameSlots.filter { knownTerminalPanes.contains($0.key) }
    }

    private func removeFrameSlot(for pane: UInt64) {
        frameSlots.removeValue(forKey: pane)?.update(nil)
    }

    private func clearFrameSlots() {
        frameSlots.values.forEach { $0.update(nil) }
        frameSlots.removeAll()
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
