import SwiftUI
import UIKit

struct ContentView: View {
    @EnvironmentObject private var store: ZZStore
    @Namespace private var paneTransition

    var body: some View {
        Group {
            switch store.connectionState {
            case .idle, .connecting:
                ProgressView("Connecting to zz")
                    .controlSize(.large)
            case let .needsHost(message):
                HostSetup(message: message)
            case let .failed(message):
                ConnectionFailure(message: message)
            case .disconnected:
                ConnectionFailure(message: "The daemon disconnected.")
            case .connected:
                workspace
            case let .reconnecting(attempt, delay):
                if store.sessions.isEmpty {
                    ReconnectingView(attempt: attempt, delay: delay)
                } else {
                    workspace
                        .overlay(alignment: .top) {
                            ReconnectBanner(attempt: attempt, delay: delay)
                                .padding(.horizontal, 14)
                                .padding(.top, 8)
                        }
                }
            }
        }
        .background(Color.zzCanvas.ignoresSafeArea())
        .alert(
            "Action failed",
            isPresented: Binding(
                get: { store.actionError != nil },
                set: { if !$0 { store.dismissActionError() } }
            )
        ) {
            Button("OK") {
                store.dismissActionError()
            }
        } message: {
            Text(store.actionError ?? "zz couldn’t complete that action.")
        }
        .sheet(
            item: Binding(
                get: { store.sshPrompt },
                set: { prompt in
                    if prompt == nil, store.sshPrompt != nil {
                        store.respondToSSHPrompt(.cancel)
                    }
                }
            )
        ) { prompt in
            SSHPromptSheet(prompt: prompt)
        }
    }

    @ViewBuilder
    private var workspace: some View {
        if let pane = store.selectedPane {
            FullscreenPane(pane: pane, namespace: paneTransition)
        } else {
            PaneOverview(namespace: paneTransition)
        }
    }
}

private struct ReconnectingView: View {
    @EnvironmentObject private var store: ZZStore
    let attempt: Int
    let delay: Int

    var body: some View {
        VStack(spacing: 18) {
            ProgressView()
                .controlSize(.large)
            Text("Reconnecting to zz")
                .font(.title2.weight(.semibold))
            Text(delay > 0 ? "Attempt \(attempt) retries in \(delay) seconds." : "Attempt \(attempt) is starting now.")
                .foregroundStyle(.secondary)
            Button("Retry Now") {
                store.retry()
            }
            .buttonStyle(.glassProminent)
            if store.canConfigureHost {
                Button("Change Host") {
                    store.showHostSetup()
                }
                .buttonStyle(.glass)
            }
        }
        .padding(32)
    }
}

private struct ReconnectBanner: View {
    @EnvironmentObject private var store: ZZStore
    let attempt: Int
    let delay: Int

    var body: some View {
        HStack(spacing: 12) {
            ProgressView()
                .controlSize(.small)
            VStack(alignment: .leading, spacing: 1) {
                Text("Reconnecting")
                    .font(.subheadline.weight(.semibold))
                Text(delay > 0 ? "Attempt \(attempt) · retry in \(delay)s" : "Attempt \(attempt) · connecting")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 8)
            Button("Now") {
                store.retry()
            }
            .font(.caption.weight(.semibold))
            if store.canConfigureHost {
                Button("Host") {
                    store.showHostSetup()
                }
                .font(.caption.weight(.semibold))
            }
        }
        .padding(.horizontal, 14)
        .frame(minHeight: 54)
        .glassEffect(.regular, in: Capsule())
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Reconnecting to zz, attempt \(attempt)")
    }
}

private struct SSHPromptSheet: View {
    @EnvironmentObject private var store: ZZStore
    let prompt: ZZSSHPromptRequest
    @State private var response = ""
    @FocusState private var responseFocused: Bool

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 20) {
                Image(systemName: prompt.kind == .hostKey ? "key.viewfinder" : "lock.shield")
                    .font(.system(size: 34, weight: .medium))
                    .foregroundStyle(Color.accentColor)
                Text(prompt.title)
                    .font(.title2.weight(.bold))
                Text(prompt.message)
                    .font(prompt.kind == .hostKey ? .caption.monospaced() : .body)
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)

                switch prompt.kind {
                case .hostKey:
                    Button("Trust and Save") {
                        store.respondToSSHPrompt(.trustAndSave)
                    }
                    .buttonStyle(.glassProminent)
                    .frame(maxWidth: .infinity)
                    Button("Trust Once") {
                        store.respondToSSHPrompt(.trustOnce)
                    }
                    .buttonStyle(.glass)
                    .frame(maxWidth: .infinity)
                case .secret:
                    Group {
                        if prompt.echo {
                            TextField("Response", text: $response)
                        } else {
                            SecureField("Password or code", text: $response)
                        }
                    }
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .focused($responseFocused)
                    .submitLabel(.continue)
                    .onSubmit(submit)
                    .padding(16)
                    .background(Color.white.opacity(0.08), in: RoundedRectangle(cornerRadius: 16))
                    Button("Continue", action: submit)
                        .buttonStyle(.glassProminent)
                        .disabled(response.isEmpty)
                case .confirmation:
                    Button("Continue") {
                        store.respondToSSHPrompt(.answer("yes"))
                    }
                    .buttonStyle(.glassProminent)
                }
                Spacer()
            }
            .padding(24)
            .frame(maxWidth: .infinity, alignment: .leading)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel", role: .cancel) {
                        store.respondToSSHPrompt(.cancel)
                    }
                }
            }
        }
        .presentationDetents([.medium, .large])
        .interactiveDismissDisabled()
        .onAppear {
            responseFocused = prompt.kind == .secret
        }
    }

    private func submit() {
        guard !response.isEmpty else {
            return
        }
        let answer = response
        response = ""
        store.respondToSSHPrompt(.answer(answer))
    }
}

private struct HostSetup: View {
    @EnvironmentObject private var store: ZZStore
    let message: String?
    @State private var endpoint = ""
    @State private var password = ""
    @State private var copiedKey = false
    @FocusState private var focusedField: Field?

    private enum Field {
        case endpoint
        case password
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                VStack(alignment: .leading, spacing: 8) {
                    Image(systemName: "terminal.fill")
                        .font(.system(size: 38, weight: .medium))
                        .foregroundStyle(Color.accentColor)
                    Text("Connect your zz host")
                        .font(.largeTitle.weight(.bold))
                    Text("Your sessions stay on the computer. The iPhone app connects to them over SSH.")
                        .foregroundStyle(.secondary)
                }

                VStack(alignment: .leading, spacing: 14) {
                    TextField("user@hostname", text: $endpoint)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .keyboardType(.URL)
                        .textContentType(.URL)
                        .submitLabel(.next)
                        .focused($focusedField, equals: .endpoint)
                        .onSubmit {
                            focusedField = .password
                        }
                        .padding(16)
                        .background(Color.white.opacity(0.08), in: RoundedRectangle(cornerRadius: 16))

                    SecureField("Password (optional)", text: $password)
                        .textContentType(.password)
                        .submitLabel(.go)
                        .focused($focusedField, equals: .password)
                        .onSubmit(connect)
                        .padding(16)
                        .background(Color.white.opacity(0.08), in: RoundedRectangle(cornerRadius: 16))

                    Text("The password is used for this connection only and is never saved.")
                        .font(.caption)
                        .foregroundStyle(.secondary)

                    if let message, !message.isEmpty {
                        Text(message)
                            .font(.callout)
                            .foregroundStyle(.red)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(14)
                            .background(Color.red.opacity(0.1), in: RoundedRectangle(cornerRadius: 14))
                    }

                    Button(action: connect) {
                        Text("Connect")
                            .frame(maxWidth: .infinity)
                    }
                    .buttonStyle(.glassProminent)
                    .disabled(endpoint.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }

                VStack(alignment: .leading, spacing: 12) {
                    Text("Use the app’s SSH key")
                        .font(.headline)
                    Text("Add this public key to ~/.ssh/authorized_keys on the host, then connect without a password.")
                        .font(.callout)
                        .foregroundStyle(.secondary)

                    if let publicKey = store.sshPublicKey {
                        Text(publicKey)
                            .font(.caption.monospaced())
                            .textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(14)
                            .background(Color.black.opacity(0.28), in: RoundedRectangle(cornerRadius: 14))

                        Button {
                            UIPasteboard.general.string = publicKey
                            copiedKey = true
                        } label: {
                            Label(copiedKey ? "Copied" : "Copy Public Key", systemImage: copiedKey ? "checkmark" : "doc.on.doc")
                        }
                        .buttonStyle(.glass)
                    } else {
                        ProgressView("Preparing SSH key")
                            .foregroundStyle(.secondary)
                    }
                }
                .padding(18)
                .background(Color.white.opacity(0.05), in: RoundedRectangle(cornerRadius: 20))

                Text("zz must be installed on the host and available to its login shell. The app starts the remote daemon when needed.")
                    .font(.caption)
                    .foregroundStyle(.secondary)

                if store.hasSavedHost {
                    Button("Forget Saved Host", role: .destructive) {
                        store.forgetHost()
                        endpoint = ""
                        password = ""
                    }
                    .frame(maxWidth: .infinity)
                }
            }
            .frame(maxWidth: 560)
            .padding(24)
            .frame(maxWidth: .infinity)
        }
        .scrollDismissesKeyboard(.interactively)
        .onAppear {
            if endpoint.isEmpty {
                endpoint = store.hostEndpoint
            }
        }
        .onChange(of: store.hostEndpoint) { _, value in
            endpoint = value
        }
        .sensoryFeedback(.success, trigger: copiedKey)
    }

    private func connect() {
        let submittedPassword = password.isEmpty ? nil : password
        store.connectHost(endpoint, password: submittedPassword)
        password = ""
    }
}

private struct ConnectionFailure: View {
    @EnvironmentObject private var store: ZZStore
    let message: String

    var body: some View {
        VStack(spacing: 18) {
            Image(systemName: "bolt.horizontal.circle")
                .font(.system(size: 42, weight: .medium))
                .foregroundStyle(.secondary)
            Text("Can’t reach zz")
                .font(.title2.weight(.semibold))
            Text(message)
                .font(.body)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
            Button("Try Again") {
                store.retry()
            }
            .buttonStyle(.glassProminent)
            if store.canConfigureHost {
                Button("Change Host") {
                    store.showHostSetup()
                }
                .buttonStyle(.glass)
            }
        }
        .padding(32)
    }
}

private struct PaneOverview: View {
    @EnvironmentObject private var store: ZZStore
    let namespace: Namespace.ID
    @State private var paneToClose: ZZPane?

    private let columns = [
        GridItem(.flexible(), spacing: 12),
        GridItem(.flexible(), spacing: 12),
    ]

    var body: some View {
        VStack(spacing: 0) {
            header
            if !store.agentAttention.isEmpty {
                AgentAttentionStrip()
                    .padding(.bottom, 10)
            }
            if let session = store.selectedSession {
                ScrollView {
                    LazyVGrid(columns: columns, spacing: 18) {
                        ForEach(session.panes) { pane in
                            PaneCard(
                                pane: pane,
                                frame: store.frame(for: pane.id),
                                namespace: namespace,
                                onOpen: {
                                    withAnimation(.snappy(duration: 0.32)) {
                                        store.openPane(pane)
                                    }
                                },
                                onClose: {
                                    paneToClose = pane
                                }
                            )
                        }
                    }
                    .padding(.horizontal, 14)
                    .padding(.bottom, 18)
                }
                .scrollIndicators(.hidden)
            } else if store.sessions.isEmpty {
                ContentUnavailableView(
                    "No Sessions",
                    systemImage: "rectangle.stack.badge.plus",
                    description: Text("Create a session to start working.")
                )
            }
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            SessionRail()
        }
        .alert(
            "Close pane?",
            isPresented: Binding(
                get: { paneToClose != nil },
                set: { if !$0 { paneToClose = nil } }
            ),
            presenting: paneToClose
        ) { pane in
            Button("Close Pane", role: .destructive) {
                store.closePane(pane.id)
                paneToClose = nil
            }
            Button("Cancel", role: .cancel) {
                paneToClose = nil
            }
        } message: { pane in
            Text("zz will stop the process running in “\(pane.title)”.")
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(store.selectedSession?.name ?? "zz")
                .font(.title2.weight(.bold))
                .lineLimit(1)
            Text("Active window")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
    }
}

private struct AgentAttentionStrip: View {
    @EnvironmentObject private var store: ZZStore

    var body: some View {
        ScrollView(.horizontal) {
            HStack(spacing: 8) {
                ForEach(store.agentAttention) { item in
                    Button {
                        store.open(ZZNavigationTarget(session: item.session, pane: item.pane))
                    } label: {
                        HStack(spacing: 7) {
                            Image(systemName: item.kind.symbol)
                                .symbolEffect(.pulse, isActive: item.kind == .working)
                            VStack(alignment: .leading, spacing: 0) {
                                Text(item.title)
                                    .font(.caption.weight(.semibold))
                                    .lineLimit(1)
                                Text(item.kind.label)
                                    .font(.caption2)
                                    .foregroundStyle(.secondary)
                            }
                        }
                        .padding(.horizontal, 12)
                        .frame(height: 42)
                        .foregroundStyle(attentionColor(item.kind))
                        .background(Color.white.opacity(0.07), in: Capsule())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("\(item.title), \(item.kind.label)")
                }
            }
            .padding(.horizontal, 14)
        }
        .scrollIndicators(.hidden)
        .accessibilityIdentifier("agent-attention")
    }

    private func attentionColor(_ kind: ZZAgentAttentionKind) -> Color {
        switch kind {
        case .blocked: .orange
        case .failed: .red
        case .done: .green
        case .working: .accentColor
        }
    }
}

private struct PaneCard: View {
    @EnvironmentObject private var store: ZZStore
    let pane: ZZPane
    let frame: TerminalFrame?
    let namespace: Namespace.ID
    let onOpen: () -> Void
    let onClose: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 9) {
            ZStack(alignment: .topTrailing) {
                Button(action: onOpen) {
                    Group {
                        if pane.kind == .terminal {
                            TerminalSurface(
                                store: store,
                                pane: pane.id,
                                frame: frame,
                                interactive: false,
                                preview: true
                            )
                        } else if pane.kind == .agent {
                            AgentPaneSummary(pane: pane)
                        } else {
                            PanePlaceholder(pane: pane)
                        }
                    }
                    .allowsHitTesting(false)
                    .frame(height: 252)
                    .frame(maxWidth: .infinity)
                    .background(Color.zzCard)
                    .clipShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
                    .overlay {
                        RoundedRectangle(cornerRadius: 22, style: .continuous)
                            .stroke(pane.isActive ? Color.accentColor.opacity(0.8) : .white.opacity(0.1), lineWidth: pane.isActive ? 2 : 1)
                    }
                    .matchedGeometryEffect(id: pane.id, in: namespace)
                    .contentShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Open \(pane.title.isEmpty ? pane.kind.label : pane.title)")
                .accessibilityIdentifier("open-pane-\(pane.id)")

                Button(action: onClose) {
                    Image(systemName: "xmark")
                        .font(.system(size: 13, weight: .bold))
                        .frame(width: 30, height: 30)
                        .glassEffect(.regular.interactive(), in: Circle())
                        .frame(width: 44, height: 44)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .zIndex(1)
                .padding(1)
                .accessibilityLabel("Close \(pane.title.isEmpty ? pane.kind.label : pane.title)")
                .accessibilityIdentifier("close-pane-\(pane.id)")
            }

            HStack(spacing: 6) {
                Image(systemName: pane.kind.symbol)
                    .foregroundStyle(pane.hasBell ? Color.orange : .secondary)
                Text(pane.title.isEmpty ? pane.kind.label : pane.title)
                    .font(.subheadline.weight(.semibold))
                    .lineLimit(1)
                Spacer(minLength: 0)
                if let attention = store.agentAttention.first(where: { $0.pane == pane.id }) {
                    Label(attention.kind.label, systemImage: attention.kind.symbol)
                        .labelStyle(.iconOnly)
                        .font(.caption)
                        .foregroundStyle(attention.kind == .failed ? Color.red : Color.orange)
                        .accessibilityLabel(attention.kind.label)
                }
            }
            .padding(.horizontal, 4)
        }
    }
}

private struct PanePlaceholder: View {
    let pane: ZZPane

    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: pane.kind.symbol)
                .font(.system(size: 34, weight: .medium))
                .foregroundStyle(.secondary)
            Text(pane.kind.label)
                .font(.headline)
            Text("Open on desktop")
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

private struct AgentPaneSummary: View {
    @EnvironmentObject private var store: ZZStore
    let pane: ZZPane

    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: state?.status == .needsInput ? "hand.raised.fill" : "sparkles")
                .font(.system(size: 34, weight: .medium))
                .foregroundStyle(state?.status == .failed ? Color.red : Color.accentColor)
            Text(state?.title ?? (pane.title.isEmpty ? "Agent" : pane.title))
                .font(.headline)
                .lineLimit(2)
                .multilineTextAlignment(.center)
            Text(state?.phase.label ?? "Waiting for state")
                .font(.caption)
                .foregroundStyle(.secondary)
            if let permission = state?.permission {
                Text(permission.title)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.orange)
                    .lineLimit(2)
                    .multilineTextAlignment(.center)
            }
        }
        .padding(18)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var state: ZZAgentState? {
        store.agentState(for: pane.id)
    }
}

private struct AgentPaneView: View {
    @EnvironmentObject private var store: ZZStore
    let pane: ZZPane

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                if let state = store.agentState(for: pane.id) {
                    header(state)
                    if let permission = state.permission {
                        permissionCard(permission)
                    }
                    if let git = state.git {
                        gitCard(git)
                    }
                    if let error = state.error, !error.isEmpty {
                        Label(error, systemImage: "exclamationmark.triangle.fill")
                            .font(.callout)
                            .foregroundStyle(.red)
                            .padding(16)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .background(Color.red.opacity(0.1), in: RoundedRectangle(cornerRadius: 16))
                    }
                    if state.status == .working || state.status == .needsInput {
                        Button("Cancel Turn", role: .destructive) {
                            store.cancelAgent(pane: pane.id)
                        }
                        .buttonStyle(.glass)
                    }
                } else {
                    ProgressView("Waiting for Agent state")
                        .frame(maxWidth: .infinity, minHeight: 260)
                }
            }
            .padding(20)
            .padding(.bottom, 82)
        }
        .scrollIndicators(.hidden)
    }

    private func header(_ state: ZZAgentState) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 10) {
                Image(systemName: attentionSymbol(state.status))
                    .font(.title2)
                    .foregroundStyle(attentionColor(state.status))
                VStack(alignment: .leading, spacing: 2) {
                    Text(state.title ?? (pane.title.isEmpty ? "Agent" : pane.title))
                        .font(.title2.weight(.bold))
                    Text(state.phase.label)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                Spacer()
            }
            if state.queuedPrompts > 0 {
                Text("\(state.queuedPrompts) queued \(state.queuedPrompts == 1 ? "prompt" : "prompts")")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }

    private func permissionCard(_ permission: ZZAgentPermission) -> some View {
        VStack(alignment: .leading, spacing: 14) {
            Label("Approval needed", systemImage: "hand.raised.fill")
                .font(.headline)
                .foregroundStyle(.orange)
            Text(permission.title)
                .font(.body.weight(.medium))
            ForEach(permission.options) { option in
                if option.kind.isApproval {
                    Button(option.name) {
                        store.respondToPermission(
                            pane: pane.id,
                            request: permission.requestID,
                            option: option.id
                        )
                    }
                    .buttonStyle(.glassProminent)
                    .frame(maxWidth: .infinity)
                } else {
                    Button(option.name, role: .destructive) {
                        store.respondToPermission(
                            pane: pane.id,
                            request: permission.requestID,
                            option: option.id
                        )
                    }
                    .buttonStyle(.glass)
                    .frame(maxWidth: .infinity)
                }
            }
        }
        .padding(18)
        .background(Color.orange.opacity(0.09), in: RoundedRectangle(cornerRadius: 20))
        .overlay {
            RoundedRectangle(cornerRadius: 20)
                .stroke(Color.orange.opacity(0.32))
        }
    }

    private func gitCard(_ git: ZZAgentGitSummary) -> some View {
        HStack(spacing: 14) {
            Image(systemName: "arrow.triangle.branch")
                .foregroundStyle(Color.accentColor)
            VStack(alignment: .leading, spacing: 3) {
                Text(git.branch ?? "Working tree")
                    .font(.subheadline.weight(.semibold))
                Text("\(git.changedFiles) changed · +\(git.additions) −\(git.deletions)")
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.white.opacity(0.06), in: RoundedRectangle(cornerRadius: 16))
    }

    private func attentionSymbol(_ status: ZZAgentStatus) -> String {
        switch status {
        case .idle: "checkmark.circle"
        case .working: "sparkles"
        case .needsInput: "hand.raised.fill"
        case .failed: "exclamationmark.triangle.fill"
        }
    }

    private func attentionColor(_ status: ZZAgentStatus) -> Color {
        switch status {
        case .idle: .secondary
        case .working: .accentColor
        case .needsInput: .orange
        case .failed: .red
        }
    }
}

private struct SessionRail: View {
    @EnvironmentObject private var store: ZZStore
    @State private var visibleSessionID: UInt64?

    var body: some View {
        HStack(spacing: 10) {
            Button {
                store.newSession()
            } label: {
                Group {
                    if store.isCreatingSession {
                        ProgressView()
                            .controlSize(.small)
                    } else {
                        Image(systemName: "plus")
                            .font(.system(size: 19, weight: .semibold))
                    }
                }
                .frame(width: 48, height: 48)
                .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .glassEffect(.regular.interactive(), in: Circle())
            .disabled(store.isCreatingSession)
            .accessibilityLabel("New Session")
            .accessibilityIdentifier("new-session")

            if store.sessions.isEmpty {
                Text("No Session")
                    .font(.headline)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, minHeight: 48)
                    .glassEffect(.regular, in: Capsule())
            } else {
                ScrollView(.horizontal) {
                    LazyHStack(spacing: 10) {
                        ForEach(store.sessions) { session in
                            sessionPill(session)
                                .containerRelativeFrame(.horizontal)
                                .id(session.id)
                                .scrollTransition(.interactive, axis: .horizontal) { content, phase in
                                    content
                                        .opacity(phase.isIdentity ? 1 : 0.72)
                                        .scaleEffect(phase.isIdentity ? 1 : 0.92)
                                }
                        }
                    }
                    .scrollTargetLayout()
                }
                .scrollPosition(id: $visibleSessionID)
                .scrollTargetBehavior(.viewAligned(limitBehavior: .alwaysByOne))
                .scrollIndicators(.hidden)
                .scrollDisabled(store.sessions.count < 2)
                .frame(maxWidth: .infinity)
                .frame(height: 48)
                .accessibilityHint("Swipe left or right to switch sessions")
                .accessibilityIdentifier("session-rail")
                .accessibilityAction(named: "Previous Session") {
                    store.selectAdjacentSession(offset: -1)
                }
                .accessibilityAction(named: "Next Session") {
                    store.selectAdjacentSession(offset: 1)
                }
            }

            Menu {
                Button {
                    store.newPane()
                } label: {
                    Label("New Pane", systemImage: "plus.rectangle.on.rectangle")
                }
                .disabled(store.selectedSession == nil)

                Button {
                    store.retry()
                } label: {
                    Label("Refresh Connection", systemImage: "arrow.clockwise")
                }

                if store.canConfigureHost {
                    Button {
                        store.showHostSetup()
                    } label: {
                        Label("Change Host", systemImage: "server.rack")
                    }
                }
            } label: {
                Image(systemName: "ellipsis")
                    .font(.system(size: 18, weight: .semibold))
                    .frame(width: 48, height: 48)
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .glassEffect(.regular.interactive(), in: Circle())
            .accessibilityLabel("Session Actions")
            .accessibilityIdentifier("session-menu")
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
        .onAppear {
            visibleSessionID = store.selectedSessionID
        }
        .onChange(of: store.selectedSessionID) { _, sessionID in
            guard visibleSessionID != sessionID else {
                return
            }
            withAnimation(.snappy(duration: 0.32)) {
                visibleSessionID = sessionID
            }
        }
        .onChange(of: visibleSessionID) { _, sessionID in
            guard let sessionID,
                  sessionID != store.selectedSessionID,
                  let session = store.sessions.first(where: { $0.id == sessionID }) else {
                return
            }
            store.selectSession(session)
        }
        .sensoryFeedback(.selection, trigger: visibleSessionID)
    }

    private func sessionPill(_ session: ZZSession) -> some View {
        Text(session.name)
            .font(.headline)
            .lineLimit(1)
            .padding(.horizontal, 18)
            .frame(maxWidth: .infinity, minHeight: 48)
            .foregroundStyle(.white)
            .contentShape(Capsule())
            .glassEffect(
                session.id == store.selectedSessionID
                    ? .regular.tint(Color.accentColor.opacity(0.65)).interactive()
                    : .regular.interactive(),
                in: Capsule()
            )
            .accessibilityElement(children: .combine)
            .accessibilityLabel("Session \(session.name)")
            .accessibilityIdentifier("session-\(session.id)")
    }
}

private struct FullscreenPane: View {
    @EnvironmentObject private var store: ZZStore
    let pane: ZZPane
    let namespace: Namespace.ID
    @State private var showsShortcuts = false
    @State private var visiblePaneID: UInt64?
    @State private var showsComposer = false
    @State private var composedText = ""

    var body: some View {
        ZStack(alignment: .bottom) {
            Group {
                if pane.kind == .terminal {
                    TerminalSurface(
                        store: store,
                        pane: pane.id,
                        frame: store.frame(for: pane.id),
                        interactive: store.isConnected,
                        preview: false
                    )
                } else if pane.kind == .agent {
                    AgentPaneView(pane: pane)
                        .background(Color.zzCard)
                } else {
                    PanePlaceholder(pane: pane)
                        .background(Color.zzCard)
                }
            }
            .matchedGeometryEffect(id: pane.id, in: namespace)

            paneBar
                .padding(.horizontal, 14)
                .padding(.bottom, 8)
        }
        .background(paneBackground.ignoresSafeArea())
        .onAppear {
            visiblePaneID = pane.id
        }
        .onChange(of: pane.id) { _, paneID in
            showsShortcuts = false
            guard visiblePaneID != paneID else {
                return
            }
            withAnimation(.snappy(duration: 0.32)) {
                visiblePaneID = paneID
            }
        }
        .onChange(of: visiblePaneID) { _, paneID in
            guard let paneID,
                  paneID != pane.id,
                  let target = panes.first(where: { $0.id == paneID }) else {
                return
            }
            store.openPane(target)
        }
        .sensoryFeedback(.selection, trigger: visiblePaneID)
        .sheet(isPresented: $showsComposer) {
            TerminalComposer(text: $composedText) {
                let text = composedText
                composedText = ""
                showsComposer = false
                store.sendText(text, to: pane.id)
            }
        }
    }

    private var paneBar: some View {
        HStack(spacing: 10) {
            Button {
                withAnimation(.snappy(duration: 0.32)) {
                    store.showOverview()
                }
            } label: {
                Image(systemName: "rectangle.grid.2x2")
                    .frame(width: 48, height: 48)
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .glassEffect(.regular.interactive(), in: Circle())
            .accessibilityLabel("Show Pane Overview")
            .accessibilityIdentifier("show-overview")

            ZStack {
                if showsShortcuts, pane.kind == .terminal {
                    shortcutBar
                        .transition(.opacity.combined(with: .scale(scale: 0.98)))
                } else {
                    panePager
                        .transition(.opacity.combined(with: .scale(scale: 0.98)))
                }
            }
            .frame(maxWidth: .infinity)

            Button {
                withAnimation(.snappy(duration: 0.2)) {
                    showsShortcuts.toggle()
                }
            } label: {
                Image(systemName: "keyboard")
                    .frame(width: 48, height: 48)
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .foregroundStyle(showsShortcuts ? Color.accentColor : Color.primary)
            .glassEffect(
                showsShortcuts
                    ? .regular.tint(Color.accentColor.opacity(0.28)).interactive()
                    : .regular.interactive(),
                in: Circle()
            )
            .disabled(pane.kind != .terminal)
            .opacity(pane.kind == .terminal ? 1 : 0.35)
            .accessibilityLabel(showsShortcuts ? "Hide Keyboard Shortcuts" : "Show Keyboard Shortcuts")
            .accessibilityIdentifier("show-shortcuts")
        }
        .accessibilityIdentifier("pane-bar")
    }

    private var panePager: some View {
        ScrollView(.horizontal) {
            LazyHStack(spacing: 10) {
                ForEach(panes) { candidate in
                    panePill(candidate)
                        .containerRelativeFrame(.horizontal)
                        .id(candidate.id)
                        .scrollTransition(.interactive, axis: .horizontal) { content, phase in
                            content
                                .opacity(phase.isIdentity ? 1 : 0.72)
                                .scaleEffect(phase.isIdentity ? 1 : 0.92)
                        }
                }
            }
            .scrollTargetLayout()
        }
        .scrollPosition(id: $visiblePaneID)
        .scrollTargetBehavior(.viewAligned(limitBehavior: .alwaysByOne))
        .scrollIndicators(.hidden)
        .scrollDisabled(panes.count < 2)
        .frame(height: 48)
        .accessibilityHint("Swipe left or right to switch panes")
        .accessibilityIdentifier("pane-pager")
        .accessibilityAction(named: "Previous Pane") {
            store.selectAdjacentPane(from: pane.id, offset: -1)
        }
        .accessibilityAction(named: "Next Pane") {
            store.selectAdjacentPane(from: pane.id, offset: 1)
        }
    }

    private func panePill(_ candidate: ZZPane) -> some View {
        HStack(spacing: 7) {
            Image(systemName: candidate.kind.symbol)
                .foregroundStyle(candidate.hasBell ? Color.orange : Color.secondary)
            Text(candidate.title.isEmpty ? candidate.kind.label : candidate.title)
                .font(.subheadline.weight(.semibold))
                .lineLimit(1)
        }
        .padding(.horizontal, 14)
        .frame(maxWidth: .infinity, minHeight: 48)
        .contentShape(Capsule())
        .glassEffect(
            candidate.id == pane.id
                ? .regular.tint(Color.accentColor.opacity(0.26)).interactive()
                : .regular.interactive(),
            in: Capsule()
        )
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Pane \(candidate.title.isEmpty ? candidate.kind.label : candidate.title)")
        .accessibilityIdentifier("pane-\(candidate.id)")
    }

    private var shortcutBar: some View {
        ScrollView(.horizontal) {
            HStack(spacing: 8) {
                TerminalShortcutButton("Esc") {
                    store.sendShortcutKey(UInt32(ZZ_KEY_ESCAPE.rawValue), to: pane.id)
                }
                TerminalShortcutButton("Tab") {
                    store.sendShortcutKey(UInt32(ZZ_KEY_TAB.rawValue), to: pane.id)
                }
                TerminalShortcutButton(
                    "Ctrl",
                    selected: store.controlModifierEnabled,
                    locked: store.controlModifierLocked
                ) {
                    store.toggleControlModifier()
                }
                TerminalShortcutButton(
                    "Alt",
                    selected: store.altModifierEnabled,
                    locked: store.altModifierLocked
                ) {
                    store.toggleAltModifier()
                }
                TerminalShortcutButton(
                    "Shift",
                    selected: store.shiftModifierEnabled,
                    locked: store.shiftModifierLocked
                ) {
                    store.toggleShiftModifier()
                }
                TerminalShortcutButton("←") {
                    store.sendShortcutKey(UInt32(ZZ_KEY_ARROW_LEFT.rawValue), to: pane.id)
                }
                TerminalShortcutButton("↓") {
                    store.sendShortcutKey(UInt32(ZZ_KEY_ARROW_DOWN.rawValue), to: pane.id)
                }
                TerminalShortcutButton("↑") {
                    store.sendShortcutKey(UInt32(ZZ_KEY_ARROW_UP.rawValue), to: pane.id)
                }
                TerminalShortcutButton("→") {
                    store.sendShortcutKey(UInt32(ZZ_KEY_ARROW_RIGHT.rawValue), to: pane.id)
                }
                TerminalShortcutButton("Prefix") {
                    store.sendPrefix(to: pane.id)
                }
                TerminalShortcutButton("Copy") {
                    store.copySelection(pane: pane.id)
                }
                TerminalShortcutButton("Compose") {
                    showsComposer = true
                }
            }
            .padding(.horizontal, 8)
        }
        .scrollIndicators(.hidden)
        .frame(height: 48)
        .glassEffect(.regular, in: Capsule())
        .disabled(!store.isConnected)
        .accessibilityIdentifier("keyboard-shortcuts")
    }

    private var panes: [ZZPane] {
        store.selectedSession?.panes ?? [pane]
    }

    private var paneBackground: Color {
        guard pane.kind == .terminal, let frame = store.frame(for: pane.id) else {
            return .zzCard
        }
        return Color(terminalColor: frame.background)
    }
}

private struct TerminalShortcutButton: View {
    let title: String
    let selected: Bool
    let locked: Bool
    let action: () -> Void

    init(
        _ title: String,
        selected: Bool = false,
        locked: Bool = false,
        action: @escaping () -> Void
    ) {
        self.title = title
        self.selected = selected
        self.locked = locked
        self.action = action
    }

    var body: some View {
        Button(action: action) {
            HStack(spacing: 5) {
                Text(title)
                if locked {
                    Image(systemName: "lock.fill")
                        .font(.caption2)
                }
            }
            .font(.subheadline.weight(.medium))
            .padding(.horizontal, 14)
            .frame(height: 36)
            .foregroundStyle(selected ? Color.white : Color.primary)
            .background {
                Capsule()
                    .fill(selected ? Color.accentColor.opacity(0.82) : Color.white.opacity(0.09))
            }
        }
        .buttonStyle(.plain)
        .accessibilityValue(locked ? "Locked" : selected ? "Once" : "Off")
        .accessibilityIdentifier("shortcut-\(title.lowercased())")
    }
}

private struct TerminalComposer: View {
    @Binding var text: String
    let send: () -> Void
    @Environment(\.dismiss) private var dismiss
    @FocusState private var focused: Bool

    var body: some View {
        NavigationStack {
            TextEditor(text: $text)
                .font(.body.monospaced())
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .focused($focused)
                .padding(12)
                .navigationTitle("Compose Terminal Input")
                .navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button("Cancel") {
                            dismiss()
                        }
                    }
                    ToolbarItem(placement: .confirmationAction) {
                        Button("Send", action: send)
                            .disabled(text.isEmpty)
                    }
                }
        }
        .presentationDetents([.medium, .large])
        .onAppear {
            focused = true
        }
    }
}

private extension Color {
    static let zzCanvas = Color(red: 0.035, green: 0.04, blue: 0.065)
    static let zzCard = Color(red: 0.07, green: 0.075, blue: 0.105)

    init(terminalColor packed: UInt32) {
        self.init(
            .sRGB,
            red: Double((packed >> 16) & 0xff) / 255,
            green: Double((packed >> 8) & 0xff) / 255,
            blue: Double(packed & 0xff) / 255,
            opacity: 1
        )
    }
}
