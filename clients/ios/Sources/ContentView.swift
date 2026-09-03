import SwiftUI
import UIKit

struct ContentView: View {
    @EnvironmentObject private var store: ZZStore
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    @Namespace private var paneTransition
    @State private var showsSettings = false

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
            case let .reconnecting(attempt, delay, error):
                if store.sessions.isEmpty {
                    ReconnectingView(attempt: attempt, delay: delay, error: error)
                } else {
                    workspace
                        .overlay(alignment: .top) {
                            ReconnectBanner(attempt: attempt, delay: delay, error: error)
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
        .alert(
            "Tmux config",
            isPresented: Binding(
                get: { store.tmuxImportPhase.needsAlert },
                set: { if !$0 { store.acknowledgeTmuxImport() } }
            )
        ) {
            if store.tmuxImportPhase.promptEndpoint != nil {
                Button("Import") {
                    store.runTmuxImportManually()
                }
                Button("Not now", role: .cancel) {
                    store.declineTmuxImport()
                }
            } else {
                Button("OK", role: .cancel) {
                    store.dismissTmuxImport()
                }
            }
        } message: {
            if let endpoint = store.tmuxImportPhase.promptEndpoint {
                Text(ZZTMuxImport.promptMessage(endpoint: endpoint))
            } else {
                Text(store.tmuxImportPhase.resultMessage ?? "Import finished.")
            }
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
        Group {
            if horizontalSizeClass == .regular {
                IPadWorkspace(showSettings: showSettings)
            } else if let pane = store.selectedPane {
                FullscreenPane(pane: pane, namespace: paneTransition)
            } else {
                PaneOverview(namespace: paneTransition, showSettings: showSettings)
            }
        }
        .sheet(isPresented: $showsSettings) {
            ClientSettingsView()
        }
    }

    private func showSettings() {
        showsSettings = true
    }
}

private struct ReconnectingView: View {
    @EnvironmentObject private var store: ZZStore
    let attempt: Int
    let delay: Int
    let error: String?

    var body: some View {
        VStack(spacing: 18) {
            ProgressView()
                .controlSize(.large)
            Text("Reconnecting to zz")
                .font(.title2.weight(.semibold))
            Text(delay > 0 ? "Attempt \(attempt) retries in \(delay) seconds." : "Attempt \(attempt) is starting now.")
                .foregroundStyle(.secondary)
            if let error, !error.isEmpty {
                Text(error)
                    .font(.caption.monospaced())
                    .foregroundStyle(.red)
                    .multilineTextAlignment(.center)
                    .lineLimit(3)
                    .textSelection(.enabled)
            }
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
    let error: String?

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
                if let error, !error.isEmpty {
                    Text(error)
                        .font(.caption2.monospaced())
                        .foregroundStyle(.red)
                        .lineLimit(2)
                        .truncationMode(.middle)
                }
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
        .accessibilityLabel(accessibilityLabel)
    }

    private var accessibilityLabel: String {
        var label = delay > 0
            ? "Reconnecting to zz, attempt \(attempt), retry in \(delay) seconds"
            : "Reconnecting to zz, attempt \(attempt), connecting"
        if let error, !error.isEmpty {
            label += ", \(error)"
        }
        return label
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
                    .background(Color.primary.opacity(0.08), in: RoundedRectangle(cornerRadius: 16))
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
                    Text("Your sessions stay on the computer. The app connects to them over SSH.")
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
                        .background(Color.primary.opacity(0.08), in: RoundedRectangle(cornerRadius: 16))

                    SecureField("Password (optional)", text: $password)
                        .textContentType(.password)
                        .submitLabel(.go)
                        .focused($focusedField, equals: .password)
                        .onSubmit(connect)
                        .padding(16)
                        .background(Color.primary.opacity(0.08), in: RoundedRectangle(cornerRadius: 16))

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
                .background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 20))

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

private struct IPadWorkspace: View {
    let showSettings: () -> Void
    @State private var columnVisibility: NavigationSplitViewVisibility = .all

    var body: some View {
        NavigationSplitView(columnVisibility: $columnVisibility) {
            IPadSessionSidebar()
                .navigationSplitViewColumnWidth(min: 230, ideal: 290, max: 380)
        } detail: {
            IPadPaneWorkspace(showSettings: showSettings)
        }
        .navigationSplitViewStyle(.balanced)
        .coordinateSpace(name: IPadPanoramaCoordinateSpace.name)
    }
}

private struct IPadSessionSidebar: View {
    @EnvironmentObject private var store: ZZStore
    @State private var expandedSessions: Set<UInt64> = []
    @State private var expandedWindows: Set<IPadSidebarWindowKey> = []
    @State private var paneToClose: ZZPane?
    @State private var windowToClose: ZZWindow?

    var body: some View {
        List {
            if store.sessions.isEmpty {
                ContentUnavailableView(
                    "No Sessions",
                    systemImage: "rectangle.stack.badge.plus",
                    description: Text("Create a session to start working.")
                )
            } else {
                ForEach(store.sessions) { session in
                    Section {
                        sessionButton(session)

                        ForEach(outlineItems(for: session)) { item in
                            itemButton(item)
                        }
                    }
                }
            }
        }
        .listStyle(.sidebar)
        .environment(\.defaultMinListRowHeight, 1)
        .navigationTitle("Sessions")
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    store.newSession()
                } label: {
                    Label("New Session", systemImage: "plus")
                }
                .disabled(store.isCreatingSession)
            }
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
        .alert(
            "Close window?",
            isPresented: Binding(
                get: { windowToClose != nil },
                set: { if !$0 { windowToClose = nil } }
            ),
            presenting: windowToClose
        ) { window in
            Button("Close Window", role: .destructive) {
                store.closeWindow(window.id)
                windowToClose = nil
            }
            Button("Cancel", role: .cancel) {
                windowToClose = nil
            }
        } message: { _ in
            Text("zz will stop every process running in this window.")
        }
        .onChange(of: store.selectedSessionID, initial: true) {
            expandCurrentSelection()
        }
        .onChange(of: store.selectedSession?.activeWindowID, initial: true) {
            expandCurrentSelection()
        }
    }

    private func sessionButton(_ session: ZZSession) -> some View {
        let expanded = expandedSessions.contains(session.id)
        return Button {
            toggleSession(session)
        } label: {
            IPadSidebarTreeRow(
                title: session.name,
                symbol: "square.stack.3d.up",
                level: 0,
                emphasized: true,
                isExpanded: expanded
            )
        }
        .buttonStyle(.plain)
        .listRowInsets(EdgeInsets(top: 10, leading: 0, bottom: 0, trailing: 0))
        .listRowSeparator(.hidden)
        .listRowBackground(Color.clear)
        .accessibilityLabel("Session \(session.name)")
        .accessibilityValue(
            session.isAttached
                ? "Attached, \(expanded ? "Expanded" : "Collapsed")"
                : expanded ? "Expanded" : "Collapsed"
        )
        .accessibilityHint(
            session.isAttached
                ? "Shows or hides windows"
                : "Switches to this session and shows its windows"
        )
    }

    private func itemButton(_ item: IPadSidebarItem) -> some View {
        let row = rowModel(for: item)
        return Button {
            activate(item)
        } label: {
            IPadSidebarTreeRow(
                title: row.title,
                symbol: row.symbol,
                level: row.level,
                selected: row.selected,
                emphasized: row.emphasized,
                iconColor: row.iconColor,
                isExpanded: row.isExpanded,
                badgeSymbol: row.badgeSymbol,
                badgeColor: row.badgeColor
            )
        }
        .buttonStyle(.plain)
        .listRowInsets(EdgeInsets(top: 0, leading: 0, bottom: 0, trailing: 0))
        .listRowSeparator(.hidden)
        .listRowBackground(Color.clear)
        .contextMenu {
            deleteMenu(for: item)
        }
        .accessibilityLabel(row.accessibilityLabel)
        .accessibilityValue(row.accessibilityValue)
        .accessibilityAddTraits(row.selected ? .isSelected : [])
        .accessibilityIdentifier(row.accessibilityIdentifier)
    }

    @ViewBuilder
    private func deleteMenu(for item: IPadSidebarItem) -> some View {
        switch item {
        case let .window(_, window):
            Button("Close Window", role: .destructive) {
                windowToClose = window
            }
        case let .pane(_, _, pane):
            Button("Close Pane", role: .destructive) {
                paneToClose = pane
            }
        }
    }

    private func outlineItems(for session: ZZSession) -> [IPadSidebarItem] {
        guard expandedSessions.contains(session.id) else {
            return []
        }
        var items: [IPadSidebarItem] = []
        for window in session.windows {
            items.append(.window(session: session, window: window))
            let key = IPadSidebarWindowKey(session: session.id, window: window.id)
            if expandedWindows.contains(key) {
                for pane in window.panes {
                    items.append(.pane(session: session, window: window, pane: pane))
                }
            }
        }
        return items
    }

    private func activate(_ item: IPadSidebarItem) {
        switch item {
        case let .window(session, window):
            toggleWindow(IPadSidebarWindowKey(session: session.id, window: window.id))
        case let .pane(session, _, pane):
            store.selectPane(pane, in: session)
        }
    }

    private func toggleSession(_ session: ZZSession) {
        if !session.isAttached {
            withAnimation(.snappy(duration: 0.25)) {
                _ = expandedSessions.insert(session.id)
            }
            store.selectSession(session)
            return
        }

        let expanding = !expandedSessions.contains(session.id)
        withAnimation(.snappy(duration: 0.25)) {
            if expanding {
                expandedSessions.insert(session.id)
            } else {
                expandedSessions.remove(session.id)
            }
        }
    }

    private func toggleWindow(_ key: IPadSidebarWindowKey) {
        withAnimation(.snappy(duration: 0.25)) {
            if expandedWindows.contains(key) {
                expandedWindows.remove(key)
            } else {
                expandedWindows.insert(key)
            }
        }
    }

    private func expandCurrentSelection() {
        guard let session = store.selectedSession else {
            return
        }
        withoutTreeAnimation {
            expandedSessions.insert(session.id)
            if let window = session.activeWindow {
                expandedWindows.insert(
                    IPadSidebarWindowKey(session: session.id, window: window.id)
                )
            }
        }
    }

    private func withoutTreeAnimation(_ update: () -> Void) {
        var transaction = Transaction(animation: nil)
        transaction.disablesAnimations = true
        withTransaction(transaction) {
            update()
        }
    }

    private func rowModel(for item: IPadSidebarItem) -> IPadSidebarRowModel {
        switch item {
        case let .window(session, window):
            let key = IPadSidebarWindowKey(session: session.id, window: window.id)
            let title = window.name.isEmpty
                ? "Window \(window.index)"
                : "\(window.index): \(window.name)"
            let accessibilityLabel = window.name.isEmpty
                ? "Window \(window.index)"
                : "Window \(window.index), \(window.name)"
            return IPadSidebarRowModel(
                title: title,
                symbol: "macwindow",
                level: 1,
                selected: false,
                emphasized: window.isCurrent,
                iconColor: window.isCurrent ? .accentColor : .secondary,
                isExpanded: expandedWindows.contains(key),
                badgeSymbol: window.zoomedPane == nil
                    ? nil
                    : "arrow.up.left.and.arrow.down.right",
                badgeColor: .secondary,
                accessibilityLabel: accessibilityLabel,
                accessibilityValue: expandedWindows.contains(key)
                    ? "Expanded"
                    : "Collapsed",
                accessibilityIdentifier: "ipad-window-\(window.id)"
            )
        case let .pane(session, window, pane):
            let selected = pane.id == store.selectedPaneID
                || (store.selectedPaneID == nil
                    && session.isAttached
                    && window.isCurrent
                    && pane.isActive)
            let title = pane.title.isEmpty ? pane.kind.label : pane.title
            return IPadSidebarRowModel(
                title: title,
                symbol: pane.kind.symbol,
                level: 2,
                selected: selected,
                emphasized: pane.isActive || selected,
                iconColor: pane.hasBell ? .orange : .secondary,
                isExpanded: nil,
                badgeSymbol: nil,
                badgeColor: .secondary,
                accessibilityLabel: "Pane \(title)",
                accessibilityValue: pane.isActive ? "Active" : "",
                accessibilityIdentifier: "ipad-pane-\(pane.id)"
            )
        }
    }
}

private struct IPadSidebarWindowKey: Hashable {
    let session: UInt64
    let window: UInt64
}

private enum IPadSidebarItem: Identifiable {
    enum ID: Hashable {
        case window(IPadSidebarWindowKey)
        case pane(session: UInt64, pane: UInt64)
    }

    case window(session: ZZSession, window: ZZWindow)
    case pane(session: ZZSession, window: ZZWindow, pane: ZZPane)

    var id: ID {
        switch self {
        case let .window(session, window):
            .window(IPadSidebarWindowKey(session: session.id, window: window.id))
        case let .pane(session, _, pane):
            .pane(session: session.id, pane: pane.id)
        }
    }
}

private struct IPadSidebarRowModel {
    let title: String
    let symbol: String?
    let level: Int
    let selected: Bool
    let emphasized: Bool
    let iconColor: Color
    let isExpanded: Bool?
    let badgeSymbol: String?
    let badgeColor: Color
    let accessibilityLabel: String
    let accessibilityValue: String
    let accessibilityIdentifier: String
}

private struct IPadSidebarTreeRow: View {
    private static let selectionBackground = Color(
        .sRGB,
        red: 77.0 / 255.0,
        green: 164.0 / 255.0,
        blue: 1,
        opacity: 1
    )

    let title: String
    let symbol: String?
    let level: Int
    var selected = false
    var emphasized = false
    var iconColor: Color = .secondary
    let isExpanded: Bool?
    var badgeSymbol: String?
    var badgeColor: Color = .secondary

    var body: some View {
        HStack(spacing: 10) {
            if let symbol {
                Image(systemName: symbol)
                    .frame(width: 20)
                    .foregroundStyle(selected ? Color.white : iconColor)
                    .accessibilityHidden(true)
            }
            Text(title)
                .lineLimit(1)
            Spacer(minLength: 6)
            if let badgeSymbol {
                Image(systemName: badgeSymbol)
                    .font(.caption2)
                    .foregroundStyle(selected ? Color.white : badgeColor)
                    .accessibilityHidden(true)
            }
            if let isExpanded {
                Image(systemName: isExpanded ? "chevron.down" : "chevron.right")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(selected ? Color.white : Color.secondary)
                    .frame(width: 18)
                    .accessibilityHidden(true)
            }
        }
        .font(symbol == nil ? .headline : .body.weight(emphasized ? .semibold : .regular))
        .padding(.leading, 8 + CGFloat(level) * 22)
        .padding(.trailing, 8)
        .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
        .foregroundStyle(selected ? Color.white : Color.primary)
        .background(selected ? Self.selectionBackground : Color.clear, in: Capsule())
        .contentShape(Rectangle())
    }
}

private enum IPadPanoramaMotionPhase: Equatable {
    case entering
    case visible
    case exiting
}

private enum IPadPanoramaCoordinateSpace {
    static let name = "ipad-panorama-workspace"
}

private struct IPadPaneWorkspace: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(ZZClientSettings.self) private var settings
    @EnvironmentObject private var store: ZZStore
    let showSettings: () -> Void
    @State private var showsPanorama = true
    @State private var panoramaPhase = IPadPanoramaMotionPhase.entering
    @State private var panoramaOpacity = 1.0
    @State private var panoramaTransitionWindow: IPadSidebarWindowKey?
    @State private var panoramaWindowAtOverview = true
    @State private var panoramaNavigationBarVisible = false
    @State private var panoramaMotionRevision = 0
    @State private var panoramaWorkspaceFrame = CGRect.zero
    @State private var panoramaTransitionWorkspaceFrame = CGRect.zero
    @State private var panoramaTransitionTargetFrame: CGRect?
    @State private var panoramaTransitionTargetLocked = false
    @State private var panoramaTransitionSession: ZZSession?
    @State private var panoramaTransitionWindowSnapshot: ZZWindow?
    @State private var panoramaTransitionFrames: [UInt64: TerminalFrame] = [:]
    @State private var panoramaTransitionAgentStates: [UInt64: ZZAgentState] = [:]
    @State private var panoramaEntranceArmed = false

    fileprivate static let entranceDuration = 0.30
    fileprivate static let exitDuration = 0.28

    fileprivate static var entranceAnimation: Animation {
        .easeOut(duration: entranceDuration)
    }

    fileprivate static var exitAnimation: Animation {
        .easeInOut(duration: exitDuration)
    }

    var body: some View {
        Group {
            if showsPanorama {
                ZStack {
                    IPadPanorama(
                        phase: panoramaPhase,
                        transitionWindow: panoramaTransitionWindow,
                        onTransitionFrameChange: updateTransitionTargetFrame,
                        onClose: {
                            dismissPanorama(toward: selectedWindowKey)
                        }
                    )
                    .ignoresSafeArea(
                        .container,
                        edges: panoramaNavigationBarVisible ? .top : []
                    )

                    if let session = panoramaTransitionSession,
                       let window = panoramaTransitionWindowSnapshot,
                       let targetFrame = panoramaTransitionTargetFrame,
                       panoramaTransitionWorkspaceFrame.width > 1,
                       panoramaTransitionWorkspaceFrame.height > 1 {
                        IPadPanoramaWorkspaceSnapshot(
                            session: session,
                            window: window,
                            frames: panoramaTransitionFrames,
                            agentStates: panoramaTransitionAgentStates
                        )
                            .frame(
                                width: panoramaTransitionWorkspaceFrame.width,
                                height: panoramaTransitionWorkspaceFrame.height
                            )
                            .scaleEffect(
                                x: panoramaWindowAtOverview
                                    ? targetFrame.width / panoramaTransitionWorkspaceFrame.width
                                    : 1,
                                y: panoramaWindowAtOverview
                                    ? targetFrame.height / panoramaTransitionWorkspaceFrame.height
                                    : 1
                            )
                            .position(
                                x: panoramaWindowAtOverview
                                    ? targetFrame.midX
                                        - panoramaTransitionWorkspaceFrame.minX
                                    : panoramaTransitionWorkspaceFrame.width / 2,
                                y: panoramaWindowAtOverview
                                    ? targetFrame.midY
                                        - panoramaTransitionWorkspaceFrame.minY
                                    : panoramaTransitionWorkspaceFrame.height / 2
                            )
                            .zIndex(3)
                    }
                }
                .opacity(panoramaOpacity)
            } else if let session = store.selectedSession,
               let window = session.activeWindow,
               !window.visiblePanes.isEmpty {
                IPadPaneSplitLayout(spacing: 5) {
                    ForEach(window.visiblePanes) { pane in
                        IPadPaneTile(pane: pane, session: session)
                            .layoutValue(
                                key: IPadPaneLayoutValueKey.self,
                                value: pane.layout ?? Self.fullLayout
                            )
                    }
                }
                .padding(5)
            } else if store.sessions.isEmpty {
                ContentUnavailableView {
                    Label("No Sessions", systemImage: "rectangle.stack")
                } description: {
                    Text("Create a session to start working.")
                } actions: {
                    Button("New Session", action: store.newSession)
                        .buttonStyle(.glassProminent)
                }
            } else {
                ContentUnavailableView(
                    "No Visible Panes",
                    systemImage: "rectangle.split.2x1",
                    description: Text("Choose a pane from the sidebar.")
                )
            }
        }
        .background(.black.opacity(0.92))
        .ignoresSafeArea(
            .container,
            edges: settings.extendPanesUnderHomeIndicator ? .bottom : []
        )
        .toolbar {
            ToolbarItem(placement: .principal) {
                IPadStatusBar()
            }

            ToolbarItemGroup(placement: .topBarTrailing) {
                Button(action: presentPanorama) {
                    Label("Show Panorama", systemImage: "rectangle.grid.2x2")
                }
                .accessibilityIdentifier("ipad-panorama-toggle")

                Menu {
                    Button {
                        store.newPane(kind: .terminal)
                    } label: {
                        Label(
                            ZZPaneKind.terminal.label,
                            systemImage: ZZPaneKind.terminal.symbol
                        )
                    }

                    Button {
                        store.newPane(kind: .agent)
                    } label: {
                        Label(
                            ZZPaneKind.agent.label,
                            systemImage: ZZPaneKind.agent.symbol
                        )
                    }
                } label: {
                    Label("New Pane", systemImage: ZZPaneKind.picker.symbol)
                }
                .disabled(store.selectedSession == nil)
                .accessibilityIdentifier("ipad-new-pane-menu")

                Menu {
                    Button(action: showSettings) {
                        Label("Settings", systemImage: "gearshape")
                    }
                    .accessibilityIdentifier("settings")

                    Section {
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
                    }
                } label: {
                    Label("More", systemImage: "ellipsis")
                }
                .accessibilityIdentifier("ipad-overflow-menu")
            }
        }
        .navigationBarTitleDisplayMode(.inline)
        .toolbarVisibility(
            showsPanorama && !panoramaNavigationBarVisible ? .hidden : .automatic,
            for: .navigationBar
        )
        .onGeometryChange(
            for: CGRect.self,
            of: { geometry in
                geometry.frame(in: .named(IPadPanoramaCoordinateSpace.name))
            },
            action: { frame in
                guard frame.width > 1,
                      frame.height > 1,
                      frame != panoramaWorkspaceFrame else {
                    return
                }
                panoramaWorkspaceFrame = frame
            }
        )
        .onAppear {
            if showsPanorama {
                presentPanorama()
            }
        }
        .onDisappear {
            store.setTerminalPreview(false)
        }
        .onChange(of: panoramaWindowKeys) { previous, current in
            guard showsPanorama, previous.isEmpty, !current.isEmpty else {
                return
            }
            startPanoramaEntrance(from: selectedWindowKey)
        }
        .onChange(of: store.selectedPaneID) { _, paneID in
            guard let paneID, showsPanorama else {
                return
            }
            dismissPanorama(toward: windowKey(containing: paneID))
        }
        .onChange(of: panoramaTransitionTargetFrame) { _, _ in
            guard panoramaEntranceArmed else {
                return
            }
            _ = tryBeginPanoramaEntrance(revision: panoramaMotionRevision)
        }
        .onChange(of: panoramaWorkspaceFrame) { _, _ in
            guard panoramaEntranceArmed else {
                return
            }
            _ = tryBeginPanoramaEntrance(revision: panoramaMotionRevision)
        }
    }

    private func presentPanorama() {
        let origin = selectedWindowKey
        store.setTerminalPreview(true)
        prepareTransition(toward: origin)
        panoramaTransitionWorkspaceFrame = panoramaWorkspaceFrame
        panoramaOpacity = 1
        store.showOverview()
        guard !panoramaWindowKeys.isEmpty else {
            panoramaWindowAtOverview = false
            var transaction = Transaction(animation: nil)
            transaction.disablesAnimations = true
            UIView.setAnimationsEnabled(false)
            withTransaction(transaction) {
                panoramaPhase = .entering
                panoramaNavigationBarVisible = false
                showsPanorama = true
            }
            Task { @MainActor in
                await Task.yield()
                UIView.setAnimationsEnabled(true)
            }
            return
        }
        startPanoramaEntrance(from: origin)
    }

    private func startPanoramaEntrance(from window: IPadSidebarWindowKey?) {
        panoramaMotionRevision += 1
        let revision = panoramaMotionRevision
        if panoramaTransitionWindow != window {
            prepareTransition(toward: window)
        }
        panoramaWindowAtOverview = false

        if reduceMotion {
            panoramaEntranceArmed = false
            panoramaPhase = .visible
            panoramaWindowAtOverview = true
            panoramaOpacity = 0
            var transaction = Transaction(animation: nil)
            transaction.disablesAnimations = true
            UIView.setAnimationsEnabled(false)
            withTransaction(transaction) {
                panoramaNavigationBarVisible = false
                showsPanorama = true
            }
            clearTransition()
            Task { @MainActor in
                await Task.yield()
                UIView.setAnimationsEnabled(true)
                guard showsPanorama, panoramaMotionRevision == revision else {
                    return
                }
                withAnimation(.easeOut(duration: 0.15)) {
                    panoramaOpacity = 1
                }
            }
            return
        }

        panoramaEntranceArmed = true
        var transaction = Transaction(animation: nil)
        transaction.disablesAnimations = true
        UIView.setAnimationsEnabled(false)
        withTransaction(transaction) {
            panoramaPhase = .entering
            panoramaNavigationBarVisible = false
            showsPanorama = true
        }
        Task { @MainActor in
            await Task.yield()
            UIView.setAnimationsEnabled(true)
            guard showsPanorama, panoramaMotionRevision == revision else {
                return
            }
            if tryBeginPanoramaEntrance(revision: revision) {
                return
            }
            try? await Task.sleep(for: .milliseconds(500))
            guard showsPanorama, panoramaMotionRevision == revision else {
                return
            }
            _ = tryBeginPanoramaEntrance(revision: revision, fallback: true)
        }
    }

    private func tryBeginPanoramaEntrance(
        revision: Int,
        fallback: Bool = false
    ) -> Bool {
        guard panoramaEntranceArmed,
              showsPanorama,
              panoramaMotionRevision == revision
        else {
            return false
        }
        let targetReady =
            panoramaTransitionWindow == nil || panoramaTransitionTargetFrame != nil
        let workspaceReady =
            panoramaWorkspaceFrame.width > 1 && panoramaWorkspaceFrame.height > 1
        guard fallback || (targetReady && workspaceReady) else {
            return false
        }
        panoramaEntranceArmed = false
        if workspaceReady {
            panoramaTransitionWorkspaceFrame = panoramaWorkspaceFrame
        }
        panoramaTransitionTargetLocked = true
        withAnimation(Self.entranceAnimation, completionCriteria: .logicallyComplete) {
            panoramaPhase = .visible
            panoramaWindowAtOverview = true
        } completion: {
            guard showsPanorama, panoramaMotionRevision == revision else {
                return
            }
            clearTransition()
        }
        return true
    }

    private func dismissPanorama(toward window: IPadSidebarWindowKey?) {
        guard showsPanorama else {
            return
        }
        let target = window ?? selectedWindowKey
        if panoramaPhase == .exiting, panoramaTransitionWindow == target {
            return
        }

        panoramaMotionRevision += 1
        let revision = panoramaMotionRevision
        panoramaEntranceArmed = false
        var transaction = Transaction(animation: nil)
        transaction.disablesAnimations = true
        withTransaction(transaction) {
            panoramaPhase = .visible
            panoramaWindowAtOverview = true
            panoramaNavigationBarVisible = true
            prepareTransition(toward: target)
        }

        guard !reduceMotion else {
            panoramaPhase = .exiting
            Task { @MainActor in
                withAnimation(.easeOut(duration: 0.15)) {
                    panoramaOpacity = 0
                }
                try? await Task.sleep(for: .milliseconds(150))
                guard showsPanorama, panoramaMotionRevision == revision else {
                    return
                }
                withTransaction(transaction) {
                    showsPanorama = false
                    panoramaOpacity = 1
                    panoramaPhase = .entering
                    panoramaWindowAtOverview = true
                    clearTransition()
                }
                store.setTerminalPreview(false)
            }
            return
        }

        Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(16))
            guard showsPanorama, panoramaMotionRevision == revision else {
                return
            }
            for _ in 0..<4 {
                if panoramaTransitionWindow == nil
                    || (panoramaTransitionTargetFrame != nil
                        && panoramaWorkspaceFrame.width > 1
                        && panoramaWorkspaceFrame.height > 1) {
                    break
                }
                try? await Task.sleep(for: .milliseconds(16))
                guard showsPanorama, panoramaMotionRevision == revision else {
                    return
                }
            }
            panoramaTransitionWorkspaceFrame = panoramaWorkspaceFrame
            panoramaTransitionTargetLocked = true
            try? await Task.sleep(for: .milliseconds(16))
            guard showsPanorama, panoramaMotionRevision == revision else {
                return
            }
            withAnimation(Self.exitAnimation) {
                panoramaPhase = .exiting
                panoramaWindowAtOverview = false
            }
            try? await Task.sleep(for: .seconds(Self.exitDuration))
            guard showsPanorama, panoramaMotionRevision == revision else {
                return
            }
            withTransaction(transaction) {
                showsPanorama = false
                panoramaPhase = .entering
                panoramaWindowAtOverview = true
                clearTransition()
            }
            store.setTerminalPreview(false)
        }
    }

    private func prepareTransition(toward key: IPadSidebarWindowKey?) {
        panoramaTransitionWindow = key
        panoramaTransitionTargetFrame = nil
        panoramaTransitionTargetLocked = false
        panoramaTransitionFrames = [:]
        panoramaTransitionAgentStates = [:]
        panoramaTransitionSession = nil
        panoramaTransitionWindowSnapshot = nil

        guard let key,
              let session = store.sessions.first(where: { $0.id == key.session }),
              let window = session.windows.first(where: { $0.id == key.window }) else {
            return
        }

        panoramaTransitionSession = session
        panoramaTransitionWindowSnapshot = window
        panoramaTransitionFrames = Dictionary(
            uniqueKeysWithValues: window.visiblePanes.compactMap { pane in
                store.frame(for: pane.id).map { (pane.id, $0) }
            }
        )
        panoramaTransitionAgentStates = Dictionary(
            uniqueKeysWithValues: window.visiblePanes.compactMap { pane in
                store.agentState(for: pane.id).map { (pane.id, $0) }
            }
        )
    }

    private func clearTransition() {
        panoramaEntranceArmed = false
        panoramaTransitionWindow = nil
        panoramaTransitionTargetFrame = nil
        panoramaTransitionTargetLocked = false
        panoramaTransitionWorkspaceFrame = .zero
        panoramaTransitionSession = nil
        panoramaTransitionWindowSnapshot = nil
        panoramaTransitionFrames = [:]
        panoramaTransitionAgentStates = [:]
    }

    private func updateTransitionTargetFrame(
        _ key: IPadSidebarWindowKey,
        _ frame: CGRect
    ) {
        guard !panoramaTransitionTargetLocked,
              key == panoramaTransitionWindow,
              frame.width > 1,
              frame.height > 1 else {
            return
        }
        if let current = panoramaTransitionTargetFrame,
           abs(current.minX - frame.minX) < 0.5,
           abs(current.minY - frame.minY) < 0.5,
           abs(current.width - frame.width) < 0.5,
           abs(current.height - frame.height) < 0.5 {
            return
        }
        panoramaTransitionTargetFrame = frame
    }

    private func windowKey(containing paneID: UInt64) -> IPadSidebarWindowKey? {
        for session in store.sessions {
            if let window = session.windows.first(where: { window in
                window.panes.contains(where: { $0.id == paneID })
            }) {
                return IPadSidebarWindowKey(session: session.id, window: window.id)
            }
        }
        return nil
    }

    private var selectedWindowKey: IPadSidebarWindowKey? {
        guard let session = store.selectedSession, let window = session.activeWindow else {
            return nil
        }
        return IPadSidebarWindowKey(session: session.id, window: window.id)
    }

    private var panoramaWindowKeys: [IPadSidebarWindowKey] {
        store.sessions.flatMap { session in
            session.windows.map { window in
                IPadSidebarWindowKey(session: session.id, window: window.id)
            }
        }
    }

    private static let fullLayout = ZZPaneLayout(x: 0, y: 0, width: 1, height: 1)
}

private struct IPadStatusBar: View {
    @EnvironmentObject private var store: ZZStore

    var body: some View {
        if let session = store.selectedSession, !session.windows.isEmpty {
            HStack(spacing: 8) {
                sessionMenu(session)

                Picker("Window", selection: windowSelection(in: session)) {
                    ForEach(visibleWindows(in: session)) { window in
                        windowPickerLabel(window)
                            .tag(window.id)
                    }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(maxWidth: .infinity)
                .layoutPriority(1)
                .accessibilityLabel("Window")
                .accessibilityValue(activeWindowAccessibilityValue(in: session))

                let overflow = overflowWindows(in: session)
                if !overflow.isEmpty {
                    Menu {
                        ForEach(overflow) { window in
                            Button {
                                open(window, in: session)
                            } label: {
                                Label(
                                    windowTitle(window),
                                    systemImage: windowMenuSymbol(window)
                                )
                            }
                        }
                    } label: {
                        Image(systemName: "ellipsis")
                            .frame(width: 28, height: 28)
                    }
                    .accessibilityLabel("More windows")
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .accessibilityElement(children: .contain)
            .accessibilityLabel("tmux status, session \(session.name)")
        }
    }

    private func sessionMenu(_ session: ZZSession) -> some View {
        Menu {
            ForEach(store.sessions) { candidate in
                Button {
                    store.selectSession(candidate)
                } label: {
                    Label(
                        candidate.name,
                        systemImage: candidate.id == session.id
                            ? "checkmark"
                            : "square.stack.3d.up"
                    )
                }
            }
        } label: {
            Label {
                Text(session.name)
                    .lineLimit(1)
                    .truncationMode(.middle)
            } icon: {
                Image(systemName: "square.stack.3d.up")
            }
            .labelStyle(.titleAndIcon)
        }
        .frame(maxWidth: 120)
        .accessibilityLabel("Session")
        .accessibilityValue(session.name)
    }

    private func windowPickerLabel(_ window: ZZWindow) -> some View {
        Label(windowTitle(window), systemImage: windowMenuSymbol(window))
            .labelStyle(.titleAndIcon)
            .lineLimit(1)
            .accessibilityLabel(windowAccessibilityValue(window))
    }

    private func windowSelection(in session: ZZSession) -> Binding<UInt64> {
        Binding {
            session.activeWindowID
        } set: { windowID in
            guard let window = session.windows.first(where: { $0.id == windowID }) else {
                return
            }
            open(window, in: session)
        }
    }

    private func activeWindowAccessibilityValue(in session: ZZSession) -> String {
        guard let window = session.activeWindow else {
            return ""
        }
        return windowAccessibilityValue(window)
    }

    private func windowAccessibilityValue(_ window: ZZWindow) -> String {
        var parts = ["Window \(window.index)"]
        if !window.name.isEmpty {
            parts.append(window.name)
        }
        if window.panes.contains(where: { $0.kind == .agent }) {
            parts.append("Agent")
        }
        if window.panes.contains(where: \.hasBell) {
            parts.append("bell")
        }
        if window.zoomedPane != nil {
            parts.append("zoomed")
        }
        return parts.joined(separator: ", ")
    }

    private func windowMenuSymbol(_ window: ZZWindow) -> String {
        if window.panes.contains(where: \.hasBell) {
            return "bell.fill"
        }
        if window.panes.contains(where: { $0.kind == .agent }) {
            return "sparkles"
        }
        if window.zoomedPane != nil {
            return "arrow.up.left.and.arrow.down.right"
        }
        return "macwindow"
    }

    private func visibleWindows(in session: ZZSession) -> [ZZWindow] {
        let limit = 5
        let windows = session.windows
        guard windows.count > limit else {
            return windows
        }
        let current = windows.firstIndex(where: \.isCurrent) ?? 0
        let start = min(max(current - limit / 2, 0), windows.count - limit)
        return Array(windows[start..<(start + limit)])
    }

    private func overflowWindows(in session: ZZSession) -> [ZZWindow] {
        let visible = Set(visibleWindows(in: session).map(\.id))
        return session.windows.filter { !visible.contains($0.id) }
    }

    private func windowTitle(_ window: ZZWindow) -> String {
        "\(window.index) \(window.name.isEmpty ? "Window" : window.name)"
    }

    private func open(_ window: ZZWindow, in session: ZZSession) {
        store.open(ZZNavigationTarget(session: session.id, pane: window.activePane))
    }
}

private struct IPadPaneTile: View {
    @EnvironmentObject private var store: ZZStore
    let pane: ZZPane
    let session: ZZSession

    var body: some View {
        paneContent
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Color.zzCard)
            .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .stroke(
                        pane.id == store.selectedPaneID
                            ? Color.accentColor.opacity(0.9)
                            : Color.white.opacity(0.12),
                        lineWidth: pane.id == store.selectedPaneID ? 2 : 1
                    )
            }
            .accessibilityElement(children: .contain)
            .accessibilityLabel("Pane \(paneLabel)")
            .accessibilityValue(pane.id == store.selectedPaneID ? "Selected" : "")
            .accessibilityAction(named: "Select Pane") {
                store.selectPane(pane, in: session)
            }
    }

    private var paneLabel: String {
        pane.title.isEmpty ? pane.kind.label : pane.title
    }

    @ViewBuilder
    private var paneContent: some View {
        if pane.kind == .terminal {
            LiveTerminalSurface(
                store: store,
                pane: pane.id,
                interactive: store.isConnected,
                preview: false
            )
        } else if pane.kind == .agent {
            AgentPaneView(pane: pane)
        } else {
            PanePlaceholder(pane: pane)
                .onTapGesture {
                    store.selectPane(pane, in: session)
                }
        }
    }
}

private struct IPadPaneLayoutValueKey: LayoutValueKey {
    static let defaultValue = ZZPaneLayout(x: 0, y: 0, width: 1, height: 1)
}

private struct IPadPaneSplitLayout: Layout {
    let spacing: CGFloat

    func sizeThatFits(
        proposal: ProposedViewSize,
        subviews: Subviews,
        cache: inout ()
    ) -> CGSize {
        proposal.replacingUnspecifiedDimensions(by: CGSize(width: 1024, height: 768))
    }

    func placeSubviews(
        in bounds: CGRect,
        proposal: ProposedViewSize,
        subviews: Subviews,
        cache: inout ()
    ) {
        for subview in subviews {
            let layout = subview[IPadPaneLayoutValueKey.self]
            let frame = CGRect(
                x: bounds.minX + bounds.width * CGFloat(layout.x),
                y: bounds.minY + bounds.height * CGFloat(layout.y),
                width: bounds.width * CGFloat(layout.width),
                height: bounds.height * CGFloat(layout.height)
            ).insetBy(dx: spacing, dy: spacing)
            guard frame.width > 0, frame.height > 0 else {
                continue
            }
            subview.place(
                at: frame.origin,
                anchor: .topLeading,
                proposal: ProposedViewSize(width: frame.width, height: frame.height)
            )
        }
    }
}

private struct IPadPanorama: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @EnvironmentObject private var store: ZZStore
    let phase: IPadPanoramaMotionPhase
    let transitionWindow: IPadSidebarWindowKey?
    let onTransitionFrameChange: (IPadSidebarWindowKey, CGRect) -> Void
    let onClose: () -> Void

    var body: some View {
        let prefersReducedMotion = reduceMotion

        Group {
            if store.sessions.isEmpty {
                ContentUnavailableView {
                    Label("No Sessions", systemImage: "rectangle.stack")
                } description: {
                    Text("Create a session to start working.")
                } actions: {
                    Button("New Session", action: store.newSession)
                        .buttonStyle(.glassProminent)
                }
            } else {
                ScrollView(.horizontal) {
                    LazyHStack(alignment: .top, spacing: 20) {
                        ForEach(Array(store.sessions.enumerated()), id: \.element.id) { index, session in
                            IPadPanoramaSessionColumn(
                                order: index,
                                session: session,
                                phase: phase,
                                transitionWindow: transitionWindow,
                                onTransitionFrameChange: onTransitionFrameChange
                            )
                            .containerRelativeFrame(.horizontal) { length, _ in
                                min(max(length * 0.84, 340), 480)
                            }
                            .containerRelativeFrame(.vertical)
                            .opacity(columnOpacity(for: phase, reduceMotion: prefersReducedMotion))
                            .animation(columnAnimation(for: index, phase: phase, reduceMotion: prefersReducedMotion), value: phase)
                            .scrollTransition(.interactive, axis: .horizontal) { content, scrollPhase in
                                content
                                    .opacity(
                                        prefersReducedMotion
                                            || phase != .visible
                                            || scrollPhase.isIdentity ? 1 : 0.82
                                    )
                                    .scaleEffect(
                                        prefersReducedMotion
                                            || phase != .visible
                                            || scrollPhase.isIdentity ? 1 : 0.96,
                                        anchor: .topLeading
                                    )
                            }
                            .id(session.id)
                            .zIndex(
                                transitionWindow?.session == session.id ? 1 : 0
                            )
                        }
                    }
                    .scrollTargetLayout()
                }
                .contentMargins(.all, 20, for: .scrollContent)
                .scrollTargetBehavior(.viewAligned(limitBehavior: .alwaysByOne))
                .scrollIndicators(.hidden)
                .scrollClipDisabled()
                .opacity(contentOpacity)
                .animation(contentAnimation, value: phase)
                .accessibilityElement(children: .contain)
                .accessibilityLabel("Session panorama")
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .safeAreaInset(edge: .top, alignment: .trailing, spacing: -44) {
            Button(action: onClose) {
                Image(systemName: "xmark")
                    .font(.system(size: 14, weight: .bold))
                    .frame(width: 44, height: 44)
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .glassEffect(.regular.interactive(), in: Circle())
            .padding(.top, 8)
            .padding(.trailing, 20)
            .opacity(phase == .visible ? 1 : 0)
            .scaleEffect(phase == .visible ? 1 : 0.76)
            .allowsHitTesting(phase == .visible)
            .animation(
                reduceMotion ? nil : .snappy(duration: 0.3, extraBounce: 0.08),
                value: phase
            )
            .accessibilityLabel("Close Panorama")
            .accessibilityIdentifier("ipad-panorama-close")
        }
        .allowsHitTesting(phase == .visible && transitionWindow == nil)
    }

    private var contentOpacity: Double {
        guard !reduceMotion else {
            return 1
        }
        return switch phase {
        case .entering: 0.18
        case .visible: 1
        case .exiting: 0
        }
    }

    private var contentAnimation: Animation? {
        guard !reduceMotion else {
            return nil
        }
        return phase == .visible
            ? .easeOut(duration: 0.25)
            : .easeIn(duration: 0.15)
    }

    private func columnOpacity(for phase: IPadPanoramaMotionPhase, reduceMotion: Bool) -> Double {
        guard !reduceMotion, phase != .visible else {
            return 1
        }
        return 0
    }

    private func columnAnimation(for order: Int, phase: IPadPanoramaMotionPhase, reduceMotion: Bool) -> Animation? {
        guard !reduceMotion else {
            return nil
        }
        if phase == .visible {
            return .easeOut(duration: 0.25).delay(Double(order) * 0.03)
        }
        return .easeIn(duration: 0.15)
    }
}

private struct IPadPanoramaSessionColumn: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let order: Int
    let session: ZZSession
    let phase: IPadPanoramaMotionPhase
    let transitionWindow: IPadSidebarWindowKey?
    let onTransitionFrameChange: (IPadSidebarWindowKey, CGRect) -> Void

    var body: some View {
        let prefersReducedMotion = reduceMotion

        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 9) {
                if session.isAttached {
                    Circle()
                        .fill(Color.accentColor)
                        .frame(width: 7, height: 7)
                        .accessibilityHidden(true)
                }
                Text(session.name)
                    .font(.title2.weight(.semibold))
                    .lineLimit(1)
            }
            .accessibilityElement(children: .combine)
            .accessibilityLabel(
                "Session \(session.name)" + (session.isAttached ? ", attached" : "")
            )

            if session.windows.isEmpty {
                VStack(spacing: 10) {
                    Image(systemName: "macwindow.badge.plus")
                        .font(.title2)
                    Text("No Windows")
                        .font(.headline)
                }
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollView(.vertical) {
                    LazyVStack(spacing: 16) {
                        ForEach(session.windows) { window in
                            IPadPanoramaWindowCard(
                                session: session,
                                window: window,
                                transitionWindow: transitionWindow,
                                onTransitionFrameChange: onTransitionFrameChange
                            )
                            .scrollTransition(.interactive, axis: .vertical) { content, scrollPhase in
                                content
                                    .opacity(
                                        prefersReducedMotion
                                            || phase != .visible
                                            || scrollPhase.isIdentity ? 1 : 0.82
                                    )
                                    .scaleEffect(
                                        prefersReducedMotion
                                            || phase != .visible
                                            || scrollPhase.isIdentity ? 1 : 0.96,
                                        anchor: .top
                                    )
                            }
                            .transition(
                                .asymmetric(
                                    insertion: .opacity.combined(
                                        with: .scale(scale: 0.96, anchor: .top)
                                    ),
                                    removal: .opacity
                                )
                            )
                        }
                    }
                    .scrollTargetLayout()
                    .padding(.bottom, 4)
                    .animation(
                        reduceMotion ? nil : .snappy(duration: 0.36),
                        value: session.windows.map(\.id)
                    )
                }
                .scrollTargetBehavior(.viewAligned(limitBehavior: .alwaysByOne))
                .scrollIndicators(.hidden)
                .scrollClipDisabled()
            }
        }
        .frame(maxHeight: .infinity, alignment: .top)
    }
}

private struct IPadPanoramaWindowCard: View {
    let session: ZZSession
    let window: ZZWindow
    let transitionWindow: IPadSidebarWindowKey?
    let onTransitionFrameChange: (IPadSidebarWindowKey, CGRect) -> Void

    var body: some View {
        let reportsTransitionFrame = isTransitionTarget

        IPadPanoramaWindowPreview(
            session: session,
            window: window
        )
        .opacity(isTransitionTarget ? 0 : 1)
        .onGeometryChange(
            for: CGRect?.self,
            of: { geometry in
                reportsTransitionFrame
                    ? geometry.frame(in: .named(IPadPanoramaCoordinateSpace.name))
                    : nil
            },
            action: { frame in
                if let frame {
                    onTransitionFrameChange(windowKey, frame)
                }
            }
        )
        .zIndex(isTransitionTarget ? 1 : 0)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(windowAccessibilityLabel)
    }

    private var windowAccessibilityLabel: String {
        var value = "Window \(window.index)"
        if !window.name.isEmpty {
            value += ", \(window.name)"
        }
        if session.isAttached, window.isCurrent {
            value += ", current"
        }
        if window.zoomedPane != nil {
            value += ", zoomed"
        }
        return value
    }

    private var isTransitionTarget: Bool {
        transitionWindow == windowKey
    }

    private var windowKey: IPadSidebarWindowKey {
        IPadSidebarWindowKey(session: session.id, window: window.id)
    }
}

private struct IPadPanoramaWorkspaceSnapshot: View {
    let session: ZZSession
    let window: ZZWindow
    let frames: [UInt64: TerminalFrame]
    let agentStates: [UInt64: ZZAgentState]

    var body: some View {
        Group {
            if window.visiblePanes.isEmpty {
                ContentUnavailableView(
                    "No Visible Panes",
                    systemImage: "rectangle.split.2x1"
                )
            } else {
                IPadPaneSplitLayout(spacing: 5) {
                    ForEach(window.visiblePanes) { pane in
                        IPadPanoramaFrozenPaneContent(
                            session: session,
                            window: window,
                            pane: pane,
                            frame: frames[pane.id],
                            agentState: agentStates[pane.id]
                        )
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .background(Color.black.opacity(0.38))
                        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                        .overlay {
                            RoundedRectangle(cornerRadius: 10, style: .continuous)
                                .stroke(Color.white.opacity(0.1), lineWidth: 1)
                        }
                        .layoutValue(
                            key: IPadPaneLayoutValueKey.self,
                            value: pane.layout ?? Self.fullLayout
                        )
                    }
                }
                .padding(5)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(.black.opacity(0.92))
        .allowsHitTesting(false)
    }

    private static let fullLayout = ZZPaneLayout(x: 0, y: 0, width: 1, height: 1)
}

private struct IPadPanoramaWindowPreview: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let session: ZZSession
    let window: ZZWindow

    var body: some View {
        Group {
            if window.visiblePanes.isEmpty {
                VStack(spacing: 8) {
                    Image(systemName: "rectangle.dashed")
                        .font(.title2)
                    Text("No Panes")
                        .font(.headline)
                }
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                IPadPaneSplitLayout(spacing: 3) {
                    ForEach(window.visiblePanes) { pane in
                        IPadPanoramaPanePreview(
                            session: session,
                            window: window,
                            pane: pane
                        )
                        .layoutValue(
                            key: IPadPaneLayoutValueKey.self,
                            value: pane.layout ?? Self.fullLayout
                        )
                        .transition(
                            .asymmetric(
                                insertion: .opacity.combined(
                                    with: .scale(scale: 0.96, anchor: .center)
                                ),
                                removal: .opacity
                            )
                        )
                    }
                }
                .animation(
                    reduceMotion ? nil : .snappy(duration: 0.34),
                    value: paneTopology
                )
            }
        }
        .aspectRatio(16.0 / 10.0, contentMode: .fit)
        .frame(maxWidth: .infinity)
        .background(Color.black.opacity(0.5))
    }

    private static let fullLayout = ZZPaneLayout(x: 0, y: 0, width: 1, height: 1)

    private var paneTopology: [PaneTopology] {
        window.visiblePanes.map {
            PaneTopology(id: $0.id, layout: $0.layout ?? Self.fullLayout)
        }
    }

    private struct PaneTopology: Equatable {
        let id: UInt64
        let layout: ZZPaneLayout
    }
}

private struct IPadPanoramaPanePreview: View {
    @EnvironmentObject private var store: ZZStore
    let session: ZZSession
    let window: ZZWindow
    let pane: ZZPane

    var body: some View {
        Button(action: openPane) {
            IPadPanoramaPaneContent(
                session: session,
                window: window,
                pane: pane
            )
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .allowsHitTesting(false)
            .background(Color.black.opacity(0.38))
            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .stroke(
                        isFocusedPane
                            ? Color.accentColor.opacity(0.92)
                            : Color.white.opacity(0.1),
                        lineWidth: isFocusedPane ? 1.5 : 1
                    )
            }
            .contentShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
        }
        .buttonStyle(IPadPanoramaPaneButtonStyle())
        .hoverEffect(.highlight)
        .accessibilityLabel(
            "Open \(session.name), window \(window.index), pane "
                + (pane.title.isEmpty ? pane.kind.label : pane.title)
        )
        .accessibilityValue(paneAccessibilityValue)
        .accessibilityIdentifier("ipad-panorama-pane-\(pane.id)")
    }

    private var isFocusedPane: Bool {
        pane.id == store.selectedPaneID
            || (session.isAttached && window.isCurrent && pane.isActive)
    }

    private var paneAccessibilityValue: String {
        var states: [String] = []
        if session.isAttached {
            states.append("attached session")
        }
        if window.isCurrent {
            states.append("current window")
        }
        if pane.isActive {
            states.append("active pane")
        }
        return states.joined(separator: ", ")
    }

    private func openPane() {
        store.selectPane(pane, in: session)
    }
}

private struct IPadPanoramaPaneContent: View {
    @EnvironmentObject private var store: ZZStore
    let session: ZZSession
    let window: ZZWindow
    let pane: ZZPane

    var body: some View {
        if pane.kind == .terminal, session.isAttached {
            IPadPanoramaLiveTerminalContent(store: store, pane: pane.id)
        } else if pane.kind == .agent, store.agentState(for: pane.id) != nil {
            AgentPaneSummary(pane: pane)
        } else {
            IPadPanoramaPanePlaceholder(
                symbol: pane.kind.symbol,
                label: placeholderLabel,
                waitsForFrame: false
            )
        }
    }

    private var placeholderLabel: String {
        if pane.kind == .terminal, session.isAttached {
            return "Waiting for frame"
        }
        if pane.kind == .browser || pane.kind == .editor || pane.kind == .picker {
            return "Open on desktop"
        }
        return "Tap to attach"
    }
}

private struct IPadPanoramaLiveTerminalContent: View {
    @ObservedObject private var frameSlot: TerminalFrameSlot
    private let store: ZZStore
    private let pane: UInt64

    init(store: ZZStore, pane: UInt64) {
        self.store = store
        self.pane = pane
        _frameSlot = ObservedObject(wrappedValue: store.frameSlot(for: pane))
    }

    var body: some View {
        if let frame = frameSlot.frame {
            TerminalSurface(
                store: store,
                pane: pane,
                frame: frame,
                interactive: false,
                preview: true
            )
        } else {
            IPadPanoramaPanePlaceholder(
                symbol: ZZPaneKind.terminal.symbol,
                label: "Waiting for frame",
                waitsForFrame: true
            )
        }
    }
}

private struct IPadPanoramaFrozenPaneContent: View {
    @EnvironmentObject private var store: ZZStore
    let session: ZZSession
    let window: ZZWindow
    let pane: ZZPane
    let frame: TerminalFrame?
    let agentState: ZZAgentState?

    var body: some View {
        if pane.kind == .terminal, let frame {
            TerminalSurface(
                store: store,
                pane: pane.id,
                frame: frame,
                interactive: false,
                preview: false
            )
        } else if pane.kind == .agent, let agentState {
            AgentPaneSummaryContent(pane: pane, state: agentState)
        } else {
            IPadPanoramaPanePlaceholder(
                symbol: pane.kind.symbol,
                label: placeholderLabel,
                waitsForFrame: pane.kind == .terminal && session.isAttached
            )
        }
    }

    private var placeholderLabel: String {
        if pane.kind == .terminal, session.isAttached {
            return "Waiting for frame"
        }
        if pane.kind == .browser || pane.kind == .editor || pane.kind == .picker {
            return "Open on desktop"
        }
        return "Tap to attach"
    }
}

private struct IPadPanoramaPanePlaceholder: View {
    let symbol: String
    let label: String
    let waitsForFrame: Bool

    var body: some View {
        VStack(spacing: 7) {
            if waitsForFrame {
                ProgressView()
                    .controlSize(.small)
            } else {
                Image(systemName: symbol)
                    .font(.title3)
            }
            Text(label)
                .font(.caption2)
                .foregroundStyle(.secondary)
                .lineLimit(2)
                .multilineTextAlignment(.center)
        }
        .padding(8)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

private struct IPadPanoramaPaneButtonStyle: ButtonStyle {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .opacity(configuration.isPressed ? 0.82 : 1)
            .scaleEffect(configuration.isPressed && !reduceMotion ? 0.96 : 1)
            .animation(
                reduceMotion ? nil : .smooth(duration: 0.15),
                value: configuration.isPressed
            )
    }
}

private struct PaneOverview: View {
    @EnvironmentObject private var store: ZZStore
    let namespace: Namespace.ID
    let showSettings: () -> Void
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
            SessionRail(showSettings: showSettings)
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
                        .background(Color.primary.opacity(0.07), in: Capsule())
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
    let namespace: Namespace.ID
    let onOpen: () -> Void
    let onClose: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 9) {
            ZStack(alignment: .topTrailing) {
                Button(action: onOpen) {
                    Group {
                        if pane.kind == .terminal {
                            LiveTerminalSurface(
                                store: store,
                                pane: pane.id,
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
                            .stroke(
                                pane.isActive
                                    ? Color.accentColor.opacity(0.8)
                                    : Color.primary.opacity(0.1),
                                lineWidth: pane.isActive ? 2 : 1
                            )
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
        AgentPaneSummaryContent(pane: pane, state: store.agentState(for: pane.id))
    }
}

private struct AgentPaneSummaryContent: View {
    let pane: ZZPane
    let state: ZZAgentState?

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
}

private struct AgentPaneView: View {
    @EnvironmentObject private var store: ZZStore
    let pane: ZZPane
    var bottomAccessoryInset: CGFloat = 0
    @State private var draft = ""

    private static let threadBottomID = "agent-thread-bottom"

    var body: some View {
        let state = store.agentState(for: pane.id)
        let thread = store.agentThread(for: pane.id)

        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 14) {
                    if thread.blocks.isEmpty, state == nil {
                        AgentEmptyState()
                            .frame(maxWidth: .infinity, minHeight: 260)
                    } else {
                        ForEach(thread.blocks) { block in
                            AgentThreadBlockView(block: block)
                        }
                        if let state {
                            if state.phase == .starting || state.phase == .running {
                                AgentLiveStatusRow(state: state)
                            }
                            if let permission = state.permission {
                                AgentPermissionCard(pane: pane.id, permission: permission)
                            }
                            if let error = state.error, !error.isEmpty {
                                AgentErrorCard(message: error)
                            }
                            if let git = state.git {
                                AgentGitCard(git: git)
                            }
                        }
                    }
                }
                .padding(20)
                .frame(maxWidth: 720)
                .frame(maxWidth: .infinity)
                Color.clear
                    .frame(height: 1)
                    .id(Self.threadBottomID)
            }
            .scrollIndicators(.hidden)
            .safeAreaInset(edge: .bottom, spacing: 0) {
                VStack(spacing: 0) {
                    AgentSettingsBar(pane: pane.id, state: state)
                    AgentComposer(
                        pane: pane.id,
                        phase: state?.phase ?? .starting,
                        queuedPrompts: state?.queuedPrompts ?? 0,
                        draft: $draft
                    )
                }
                .padding(.bottom, bottomAccessoryInset)
            }
            .onChange(of: thread) { _, _ in
                withAnimation(.easeOut(duration: 0.2)) {
                    proxy.scrollTo(Self.threadBottomID, anchor: .bottom)
                }
            }
            .onChange(of: state?.phase) { _, _ in
                withAnimation(.easeOut(duration: 0.2)) {
                    proxy.scrollTo(Self.threadBottomID, anchor: .bottom)
                }
            }
            .onAppear {
                store.primeAgentState(for: pane.id)
                store.ensureAgentStream(for: pane.id)
                store.ensureAgentSessions(for: pane.id)
                proxy.scrollTo(Self.threadBottomID, anchor: .bottom)
            }
        }
        .background(Color.zzCard)
        .onAppear {
            draft = store.agentDraft(for: pane.id)
        }
        .onChange(of: pane.id) { _, paneID in
            draft = store.agentDraft(for: paneID)
            store.primeAgentState(for: paneID)
            store.ensureAgentStream(for: paneID)
            store.ensureAgentSessions(for: paneID)
        }
        .onChange(of: draft) { _, text in
            store.saveAgentDraft(text, for: pane.id)
        }
    }
}

private struct AgentEmptyState: View {
    var body: some View {
        ContentUnavailableView {
            Label("Start the conversation", systemImage: "sparkles")
        } description: {
            Text("Send a prompt below. If this never connects, the daemon needs agent support enabled.")
        }
    }
}

private struct AgentThreadBlockView: View {
    let block: ZZAgentThreadBlock

    var body: some View {
        switch block.kind {
        case let .user(turn):
            AgentTurnBubble(turn: turn)
        case let .agentText(_, text):
            AgentTextBlock(text: text)
        case let .thought(_, text):
            AgentThoughtBlock(text: text)
        case let .tool(_, title, status):
            AgentToolRow(title: title, status: status)
        }
    }
}

private struct AgentTextBlock: View {
    let text: String

    var body: some View {
        HStack(alignment: .bottom, spacing: 8) {
            VStack(alignment: .leading, spacing: 4) {
                Text(text)
                    .font(.body)
                    .textSelection(.enabled)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 10)
                    .background(
                        Color.primary.opacity(0.07),
                        in: RoundedRectangle(cornerRadius: 18, style: .continuous)
                    )
            }
            Spacer(minLength: 56)
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Agent said \(text)")
    }
}

private struct AgentThoughtBlock: View {
    let text: String
    @State private var expanded = false

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 6) {
                Button {
                    withAnimation(.snappy(duration: 0.2)) {
                        expanded.toggle()
                    }
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: "brain")
                            .font(.caption)
                        Text("Thought")
                            .font(.caption.weight(.semibold))
                        Image(systemName: expanded ? "chevron.down" : "chevron.right")
                            .font(.caption2.weight(.semibold))
                    }
                    .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
                .accessibilityLabel(expanded ? "Hide thought" : "Show thought")
                if expanded {
                    Text(text)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
            }
            Spacer(minLength: 56)
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Agent thought \(text)")
    }
}

private struct AgentToolRow: View {
    let title: String
    let status: ZZAgentToolStatus

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: symbol)
                .font(.callout)
                .foregroundStyle(color)
                .frame(width: 22)
                .accessibilityHidden(true)
            Text(title)
                .font(.callout.weight(.medium))
                .lineLimit(2)
            Spacer(minLength: 4)
            if status == .running {
                ProgressView()
                    .controlSize(.small)
                    .accessibilityHidden(true)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 9)
        .background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 14))
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Tool \(title), \(statusLabel)")
    }

    private var symbol: String {
        switch status {
        case .pending: "circle"
        case .running: "circle.dotted"
        case .done: "checkmark.circle.fill"
        case .failed: "exclamationmark.circle.fill"
        }
    }

    private var color: Color {
        switch status {
        case .pending: .secondary
        case .running: .accentColor
        case .done: .green
        case .failed: .red
        }
    }

    private var statusLabel: String {
        switch status {
        case .pending: "pending"
        case .running: "running"
        case .done: "done"
        case .failed: "failed"
        }
    }
}

private struct AgentTurnBubble: View {
    let turn: ZZAgentTurn

    var body: some View {
        HStack(alignment: .bottom, spacing: 8) {
            Spacer(minLength: 56)
            VStack(alignment: .trailing, spacing: 4) {
                Text(turn.text)
                    .font(.body)
                    .foregroundStyle(.white)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 10)
                    .background(
                        Color.accentColor,
                        in: RoundedRectangle(cornerRadius: 18, style: .continuous)
                    )
                    .textSelection(.enabled)
                HStack(spacing: 4) {
                    Text(turn.sentAt, style: .time)
                    statusIcon
                    Text(statusLabel)
                }
                .font(.caption2)
                .foregroundStyle(statusColor)
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("You said \(turn.text), \(statusLabel)")
    }

    @ViewBuilder
    private var statusIcon: some View {
        switch turn.status {
        case .working:
            Image(systemName: "clock")
        case .done:
            Image(systemName: "checkmark")
        case .failed:
            Image(systemName: "exclamationmark.circle.fill")
        }
    }

    private var statusLabel: String {
        switch turn.status {
        case .working: "Working"
        case .done: "Done"
        case .failed: "Failed"
        }
    }

    private var statusColor: Color {
        switch turn.status {
        case .working, .done: .secondary
        case .failed: .red
        }
    }
}

private struct AgentLiveStatusRow: View {
    let state: ZZAgentState

    var body: some View {
        HStack(spacing: 8) {
            ProgressView()
                .controlSize(.small)
            Text(label)
                .font(.caption.weight(.medium))
        }
        .foregroundStyle(.secondary)
        .padding(.horizontal, 14)
        .padding(.vertical, 7)
        .background(Color.primary.opacity(0.06), in: Capsule())
        .frame(maxWidth: .infinity)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(label)
    }

    private var label: String {
        switch state.phase {
        case .starting:
            "Starting Agent"
        case .running:
            state.queuedPrompts == 0
                ? "Agent is working"
                : "Agent is working · \(state.queuedPrompts) queued"
        case .ready, .awaitingPermission, .failed:
            state.phase.label
        }
    }
}

private struct AgentPermissionCard: View {
    @EnvironmentObject private var store: ZZStore
    let pane: UInt64
    let permission: ZZAgentPermission

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Label("Approval needed", systemImage: "hand.raised.fill")
                .font(.headline)
                .foregroundStyle(.orange)
            Text(permission.title)
                .font(.body.weight(.medium))
                .textSelection(.enabled)
            ForEach(permission.options) { option in
                if option.kind.isApproval {
                    Button(option.name) {
                        respond(option)
                    }
                    .buttonStyle(.borderedProminent)
                    .frame(maxWidth: .infinity)
                } else {
                    Button(option.name, role: .destructive) {
                        respond(option)
                    }
                    .buttonStyle(.bordered)
                    .tint(.red)
                    .frame(maxWidth: .infinity)
                }
            }
        }
        .padding(18)
        .background(Color.orange.opacity(0.09), in: RoundedRectangle(cornerRadius: 18))
        .overlay {
            RoundedRectangle(cornerRadius: 18)
                .stroke(Color.orange.opacity(0.3))
        }
    }

    private func respond(_ option: ZZAgentPermissionOption) {
        store.respondToPermission(
            pane: pane,
            request: permission.requestID,
            option: option.id
        )
    }
}

private struct AgentErrorCard: View {
    let message: String

    var body: some View {
        Label(message, systemImage: "exclamationmark.triangle.fill")
            .font(.callout)
            .foregroundStyle(.red)
            .textSelection(.enabled)
            .padding(16)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.red.opacity(0.1), in: RoundedRectangle(cornerRadius: 16))
    }
}

private struct AgentGitCard: View {
    let git: ZZAgentGitSummary

    var body: some View {
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
        .background(Color.primary.opacity(0.06), in: RoundedRectangle(cornerRadius: 16))
        .accessibilityElement(children: .combine)
    }
}

private struct AgentSettingsBar: View {
    @EnvironmentObject private var store: ZZStore
    let pane: UInt64
    let state: ZZAgentState?
    @State private var showsSessions = false

    var body: some View {
        ScrollView(.horizontal) {
            HStack(spacing: 8) {
                Button(action: { showsSessions = true }) {
                    HStack(spacing: 6) {
                        Image(systemName: "folder")
                        Text(directoryName)
                            .lineLimit(1)
                        Image(systemName: "chevron.up.chevron.down")
                            .font(.caption2)
                    }
                    .font(.caption.weight(.medium))
                    .padding(.horizontal, 12)
                    .padding(.vertical, 7)
                    .background(Color.primary.opacity(0.07), in: Capsule())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Agent sessions, \(directoryName)")

                if let option = state?.configOption(category: .model) {
                    configMenu(option: option, icon: "asterisk")
                }
                if let option = state?.configOption(category: .thoughtLevel) {
                    configMenu(option: option, icon: "cpu")
                }
                if state?.configOption(category: .mode) == nil,
                   let modes = state?.modeState,
                   !modes.modes.isEmpty {
                    legacyModeMenu(modes: modes)
                }
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 8)
        }
        .scrollIndicators(.hidden)
        .background(.ultraThinMaterial)
        .sheet(isPresented: $showsSessions) {
            AgentSessionSheet(pane: pane)
        }
    }

    private var settingsLocked: Bool {
        state?.phase != .ready || state?.permission != nil
    }

    private var directoryName: String {
        if let sessionID = state?.sessionID,
           let match = store.agentSessionList(for: pane).sessions.first(where: {
               $0.sessionID == sessionID
           }) {
            let last = (match.cwd as NSString).lastPathComponent
            return last.isEmpty ? match.cwd : last
        }
        return "Sessions"
    }

    private func configMenu(option: ZZAgentConfigOption, icon: String) -> some View {
        Menu {
            ForEach(option.choices) { choice in
                Button {
                    store.setAgentConfigOption(pane: pane, option: option.id, value: choice.value)
                } label: {
                    VStack(alignment: .leading, spacing: 2) {
                        HStack(spacing: 6) {
                            if choice.value == option.currentValue {
                                Image(systemName: "checkmark")
                            }
                            Text(choice.name)
                        }
                        if let description = choice.description {
                            Text(description)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
            }
        } label: {
            HStack(spacing: 6) {
                Image(systemName: icon)
                Text(option.currentChoiceName)
                    .lineLimit(1)
                Image(systemName: "chevron.up.chevron.down")
                    .font(.caption2)
            }
            .font(.caption.weight(.medium))
            .padding(.horizontal, 12)
            .padding(.vertical, 7)
            .background(Color.primary.opacity(0.07), in: Capsule())
        }
        .buttonStyle(.plain)
        .disabled(settingsLocked || option.choices.isEmpty)
        .accessibilityLabel("\(option.name), \(option.currentChoiceName)")
    }

    private func legacyModeMenu(modes: ZZAgentModeState) -> some View {
        Menu {
            ForEach(modes.modes) { mode in
                Button {
                    store.setAgentMode(pane: pane, mode: mode.id)
                } label: {
                    HStack(spacing: 6) {
                        if mode.id == modes.currentID {
                            Image(systemName: "checkmark")
                        }
                        VStack(alignment: .leading, spacing: 2) {
                            Text(mode.name)
                            if let description = mode.description {
                                Text(description)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                        }
                    }
                }
            }
        } label: {
            HStack(spacing: 6) {
                Image(systemName: "checklist")
                Text(modes.currentName ?? "Mode")
                    .lineLimit(1)
                Image(systemName: "chevron.up.chevron.down")
                    .font(.caption2)
            }
            .font(.caption.weight(.medium))
            .padding(.horizontal, 12)
            .padding(.vertical, 7)
            .background(Color.primary.opacity(0.07), in: Capsule())
        }
        .buttonStyle(.plain)
        .disabled(settingsLocked)
        .accessibilityLabel("Session mode, \(modes.currentName ?? "unknown")")
    }
}

private struct AgentSessionSheet: View {
    @EnvironmentObject private var store: ZZStore
    @Environment(\.dismiss) private var dismiss
    let pane: UInt64
    @State private var newDirectory = ""
    @State private var sessionToDelete: ZZAgentSessionSummary?

    var body: some View {
        NavigationStack {
            List {
                statusRows
                sessionsSection
                newSessionSection
            }
            .navigationTitle("Agent Sessions")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close") {
                        dismiss()
                    }
                }
            }
            .alert(
                "Delete session?",
                isPresented: Binding(
                    get: { sessionToDelete != nil },
                    set: { if !$0 { sessionToDelete = nil } }
                ),
                presenting: sessionToDelete
            ) { session in
                Button("Delete Session", role: .destructive) {
                    store.deleteAgentSession(pane: pane, session: session)
                    sessionToDelete = nil
                }
                Button("Cancel", role: .cancel) {
                    sessionToDelete = nil
                }
            } message: { session in
                Text("zz will forget “\(session.displayTitle)”.")
            }
        }
        .presentationDetents([.medium, .large])
        .onAppear {
            store.listAgentSessions(pane: pane)
        }
    }

    @ViewBuilder
    private var statusRows: some View {
        let list = store.agentSessionList(for: pane)
        if list.loading, list.sessions.isEmpty {
            HStack {
                Spacer()
                ProgressView("Loading sessions")
                Spacer()
            }
            .listRowBackground(Color.clear)
        }
        if let error = list.error {
            Text(error)
                .font(.callout)
                .foregroundStyle(.red)
        }
    }

    private var sessionsSection: some View {
        let currentID = store.agentState(for: pane)?.sessionID
        let locked = self.locked
        return Section("Sessions") {
            ForEach(store.agentSessionList(for: pane).sessions) { session in
                sessionRow(
                    session,
                    isCurrent: session.sessionID == currentID,
                    locked: locked
                )
            }
        }
    }

    private var newSessionSection: some View {
        Section {
            TextField("/absolute/path", text: $newDirectory)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .submitLabel(.go)
                .onSubmit(startSession)
            Button("Start Session Here") {
                startSession()
            }
            .disabled(locked || !isAbsolutePath)
        } header: {
            Text("New session")
        } footer: {
            Text("Starts a new agent session with that working directory.")
        }
    }

    private func sessionRow(
        _ session: ZZAgentSessionSummary,
        isCurrent: Bool,
        locked: Bool
    ) -> some View {
        Button {
            store.switchAgentSession(pane: pane, session: session)
            dismiss()
        } label: {
            AgentSessionRowLabel(session: session, isCurrent: isCurrent)
        }
        .buttonStyle(.plain)
        .disabled(locked || isCurrent)
        .contextMenu {
            Button("Delete Session", role: .destructive) {
                sessionToDelete = session
            }
            .disabled(locked)
        }
        .accessibilityLabel("Switch to \(session.displayTitle)")
    }

    private var locked: Bool {
        guard let state = store.agentState(for: pane) else {
            return true
        }
        return state.phase != .ready || state.permission != nil
    }

    private var isAbsolutePath: Bool {
        newDirectory.trimmingCharacters(in: .whitespacesAndNewlines).hasPrefix("/")
    }

    private func startSession() {
        store.startAgentSession(pane: pane, cwd: newDirectory)
        newDirectory = ""
        dismiss()
    }
}

private struct AgentSessionRowLabel: View {
    let session: ZZAgentSessionSummary
    let isCurrent: Bool

    var body: some View {
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text(session.displayTitle)
                    .font(.body.weight(.medium))
                    .lineLimit(1)
                Text(session.cwd)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            Spacer(minLength: 8)
            if isCurrent {
                Image(systemName: "checkmark")
                    .foregroundStyle(Color.accentColor)
                    .accessibilityHidden(true)
            }
        }
        .padding(.vertical, 4)
    }
}

private struct AgentComposer: View {
    @EnvironmentObject private var store: ZZStore
    let pane: UInt64
    let phase: ZZAgentPhase
    let queuedPrompts: UInt32
    @Binding var draft: String
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if queuedPrompts > 0 {
                Label(
                    "\(queuedPrompts) queued \(queuedPrompts == 1 ? "prompt" : "prompts")",
                    systemImage: "clock.arrow.trianglehead.counterclockwise.rotate.90"
                )
                .font(.caption.weight(.medium))
                .foregroundStyle(.secondary)
            }

            HStack(alignment: .bottom, spacing: 10) {
                TextField(placeholder, text: $draft, axis: .vertical)
                    .lineLimit(1...6)
                    .textInputAutocapitalization(.sentences)
                    .focused($focused)
                    .submitLabel(.send)
                    .disabled(!acceptsText)
                    .onSubmit {
                        if hasPrompt {
                            performAction()
                        }
                    }
                    .padding(.horizontal, 13)
                    .padding(.vertical, 11)
                    .background(
                        Color.primary.opacity(0.07),
                        in: RoundedRectangle(cornerRadius: 15, style: .continuous)
                    )

                Button(action: performAction) {
                    Image(systemName: buttonSymbol)
                        .font(.system(size: 15, weight: .bold))
                        .frame(width: 42, height: 42)
                        .contentShape(Circle())
                }
                .buttonStyle(.plain)
                .foregroundStyle(action == .stop ? Color.red : Color.primary)
                .glassEffect(buttonGlass, in: Circle())
                .disabled(action == .unavailable)
                .accessibilityLabel(buttonLabel)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 11)
        .background(.ultraThinMaterial)
    }

    private var hasPrompt: Bool {
        !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private var action: ZZAgentComposerAction {
        ZZAgentComposerAction.resolve(
            phase: phase,
            hasPrompt: hasPrompt,
            queuedPrompts: queuedPrompts
        )
    }

    private var acceptsText: Bool {
        phase == .ready || phase == .running || phase == .awaitingPermission
    }

    private var placeholder: String {
        switch phase {
        case .ready: "Ask Agent"
        case .running, .awaitingPermission:
            queuedPrompts < ZZAgentComposerAction.maximumQueuedPrompts
                ? "Queue a follow-up"
                : "Prompt queue full"
        case .starting: "Starting Agent"
        case .failed: "Agent unavailable"
        }
    }

    private var buttonSymbol: String {
        switch action {
        case .send, .queue: "arrow.up"
        case .stop: "stop.fill"
        case .unavailable: phase == .failed ? "exclamationmark" : "ellipsis"
        }
    }

    private var buttonLabel: String {
        switch action {
        case .send: "Send prompt"
        case .queue: "Queue prompt"
        case .stop: "Stop Agent"
        case .unavailable:
            if hasPrompt,
               phase == .running || phase == .awaitingPermission,
               queuedPrompts >= ZZAgentComposerAction.maximumQueuedPrompts {
                "Prompt queue full"
            } else {
                "Agent unavailable"
            }
        }
    }

    private var buttonGlass: Glass {
        switch action {
        case .stop:
            .regular.tint(Color.red.opacity(0.2)).interactive()
        case .send, .queue:
            .regular.tint(Color.accentColor.opacity(0.24)).interactive()
        case .unavailable:
            .regular
        }
    }

    private func performAction() {
        switch action {
        case .send, .queue:
            guard store.submitAgentPrompt(draft, pane: pane) else {
                return
            }
            draft = ""
            focused = true
        case .stop:
            store.cancelAgent(pane: pane)
        case .unavailable:
            break
        }
    }
}

private struct SessionRail: View {
    @EnvironmentObject private var store: ZZStore
    let showSettings: () -> Void
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

                Divider()

                Button(action: showSettings) {
                    Label("Settings", systemImage: "gearshape")
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
            .foregroundStyle(
                session.id == store.selectedSessionID ? Color.white : Color.primary
            )
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
    @State private var showsCommandPrompt = false
    @State private var showsKeyList = false

    var body: some View {
        ZStack(alignment: .bottom) {
            Group {
                if pane.kind == .terminal {
                    LiveTerminalSurface(
                        store: store,
                        pane: pane.id,
                        interactive: store.isConnected,
                        preview: false
                    )
                } else if pane.kind == .agent {
                    AgentPaneView(pane: pane, bottomAccessoryInset: 72)
                        .background(Color.zzCard)
                } else {
                    PanePlaceholder(pane: pane)
                        .background(Color.zzCard)
                }
            }
            .matchedGeometryEffect(id: pane.id, in: namespace)

            VStack(spacing: 6) {
                prefixBindingsStrip
                paneBar
            }
            .padding(.horizontal, 14)
            .padding(.bottom, 8)
        }
        .background {
            if pane.kind == .terminal {
                LiveTerminalBackground(store: store, pane: pane.id)
                    .ignoresSafeArea()
            } else {
                Color.zzCard
                    .ignoresSafeArea()
            }
        }
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
        .sheet(isPresented: $showsCommandPrompt) {
            CommandPromptSheet { line in
                showsCommandPrompt = false
                _ = store.submitCommand(line)
            }
        }
        .sheet(isPresented: $showsKeyList) {
            KeyListSheet(bindings: store.prefixBindings) {
                showsKeyList = false
                _ = store.requestKeyList()
            } importTmuxConfig: {
                store.runTmuxImportManually()
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
                TerminalShortcutButton(
                    store.prefixArmed ? "Prefix ●" : "Prefix",
                    selected: store.prefixArmed
                ) {
                    store.sendPrefix(to: pane.id)
                }
                TerminalShortcutButton("Copy") {
                    store.copySelection(pane: pane.id)
                }
                TerminalShortcutButton("Compose") {
                    showsComposer = true
                }
                TerminalShortcutButton("Cmd") {
                    showsCommandPrompt = true
                }
                TerminalShortcutButton("Keys") {
                    showsKeyList = true
                }
                TerminalShortcutButton("Bufs") {
                    store.requestChooseBuffer()
                }
                TerminalShortcutButton("Panes") {
                    store.requestDisplayPanes()
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

    /// Daemon-published prefix bindings, shown while the prefix is armed so a
    /// soft-keyboard user sees what each key does without daemon changes.
    private var prefixBindingsStrip: some View {
        Group {
            if store.prefixArmed, !store.prefixBindings.isEmpty {
                ScrollView(.horizontal) {
                    HStack(spacing: 8) {
                        ForEach(store.prefixBindings.prefix(24)) { binding in
                            VStack(alignment: .leading, spacing: 1) {
                                Text(binding.displayKey)
                                    .font(.caption.weight(.bold).monospaced())
                                Text(
                                    binding.note.isEmpty ? binding.summary : binding.note
                                )
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                            }
                            .padding(.horizontal, 10)
                            .frame(height: 44)
                            .glassEffect(.regular, in: Capsule())
                            .accessibilityElement(children: .combine)
                            .accessibilityLabel(
                                "\(binding.displayKey), \(binding.note.isEmpty ? binding.summary : binding.note)"
                            )
                        }
                    }
                    .padding(.horizontal, 8)
                }
                .scrollIndicators(.hidden)
                .frame(height: 48)
                .accessibilityIdentifier("prefix-bindings")
            }
        }
    }

    private var panes: [ZZPane] {
        store.selectedSession?.panes ?? [pane]
    }

}

private struct LiveTerminalBackground: View {
    @ObservedObject private var frameSlot: TerminalFrameSlot

    init(store: ZZStore, pane: UInt64) {
        _frameSlot = ObservedObject(wrappedValue: store.frameSlot(for: pane))
    }

    var body: some View {
        if let frame = frameSlot.frame {
            Color(terminalColor: frame.background)
        } else {
            Color.zzCard
        }
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

/// Minimal command prompt: one line naming a daemon command plus arguments,
/// sent through the existing `zz_client_execute` path without daemon changes.
private struct CommandPromptSheet: View {
    let submit: (String) -> Void
    @Environment(\.dismiss) private var dismiss
    @FocusState private var focused: Bool
    @State private var line = ""

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 12) {
                Text("Run a daemon command, e.g. new-window or kill-pane -t %1.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                TextField("new-session -s work", text: $line)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .submitLabel(.go)
                    .focused($focused)
                    .onSubmit(run)
                    .padding(16)
                    .background(Color.primary.opacity(0.08), in: RoundedRectangle(cornerRadius: 16))
                    .accessibilityIdentifier("command-prompt-field")
                Spacer()
            }
            .padding(24)
            .frame(maxWidth: .infinity, alignment: .leading)
            .navigationTitle("Command Prompt")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        dismiss()
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Run", action: run)
                        .disabled(ZZCommandLine.split(line) == nil)
                }
            }
        }
        .presentationDetents([.medium])
        .onAppear {
            focused = true
        }
    }

    private func run() {
        guard ZZCommandLine.split(line) != nil else {
            return
        }
        submit(line)
    }
}

/// Prefix-table key list from the daemon-published bindings. The full
/// `list-keys` output still needs command-output FFI (see
/// `ZZStore.requestKeyList`); until then this covers the prefix table every
/// iOS user actually navigates.
private struct KeyListSheet: View {
    let bindings: [ZZPrefixBinding]
    let requestFullList: () -> Void
    let importTmuxConfig: () -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                if bindings.isEmpty {
                    Text("No prefix bindings published yet. Attach to a session first.")
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(bindings) { binding in
                        HStack {
                            Text(binding.displayKey)
                                .font(.body.monospaced())
                                .frame(minWidth: 110, alignment: .leading)
                            VStack(alignment: .leading) {
                                Text(binding.summary.isEmpty ? "(unbound)" : binding.summary)
                                    .font(.callout)
                                if !binding.note.isEmpty {
                                    Text(binding.note)
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }
                            }
                        }
                        .accessibilityElement(children: .combine)
                        .accessibilityLabel("\(binding.displayKey), \(binding.summary)")
                    }
                }
                Section {
                    Button("Request Full list-keys Output") {
                        requestFullList()
                    }
                    .accessibilityIdentifier("request-full-key-list")
                    Text("The daemon answers list-keys through command output, which iOS cannot display yet. The output lands in the attached session.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Section {
                    Button("Import Tmux Config From This Host") {
                        importTmuxConfig()
                    }
                    .accessibilityIdentifier("import-tmux-config")
                    Text("Copies the host’s tmux config into zz/mux.conf and reloads, so custom binds appear above.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            .navigationTitle("Prefix Keys")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") {
                        dismiss()
                    }
                }
            }
        }
        .presentationDetents([.medium, .large])
        .accessibilityIdentifier("key-list")
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
    static let zzCanvas = Color(
        uiColor: UIColor { traits in
            traits.userInterfaceStyle == .dark
                ? UIColor(red: 0.035, green: 0.04, blue: 0.065, alpha: 1)
                : UIColor(red: 0.955, green: 0.96, blue: 0.98, alpha: 1)
        }
    )
    static let zzCard = Color(
        uiColor: UIColor { traits in
            traits.userInterfaceStyle == .dark
                ? UIColor(red: 0.07, green: 0.075, blue: 0.105, alpha: 1)
                : UIColor(red: 1, green: 1, blue: 1, alpha: 1)
        }
    )

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
