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
    }
}

private struct IPadSessionSidebar: View {
    @EnvironmentObject private var store: ZZStore
    @State private var expandedSessions: Set<UInt64> = []
    @State private var expandedWindows: Set<IPadSidebarWindowKey> = []

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
                centersTitle: row.centersTitle,
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
        .accessibilityLabel(row.accessibilityLabel)
        .accessibilityValue(row.accessibilityValue)
        .accessibilityAddTraits(row.selected ? .isSelected : [])
        .accessibilityIdentifier(row.accessibilityIdentifier)
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
            withoutTreeAnimation {
                expandedSessions.insert(session.id)
            }
            store.selectSession(session)
            return
        }

        let expanding = !expandedSessions.contains(session.id)
        withoutTreeAnimation {
            if expanding {
                expandedSessions.insert(session.id)
            } else {
                expandedSessions.remove(session.id)
            }
        }
    }

    private func toggleWindow(_ key: IPadSidebarWindowKey) {
        withoutTreeAnimation {
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
                centersTitle: false,
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
                centersTitle: true,
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
    let centersTitle: Bool
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
    var centersTitle = false
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
                .frame(maxWidth: centersTitle ? .infinity : nil, alignment: .center)
            if centersTitle {
                Color.clear
                    .frame(width: symbol == nil ? 0 : 20)
                    .accessibilityHidden(true)
            } else {
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

private struct IPadPaneWorkspace: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(ZZClientSettings.self) private var settings
    @EnvironmentObject private var store: ZZStore
    @Namespace private var panoramaNamespace
    let showSettings: () -> Void
    @State private var showsPanorama = true
    @State private var panoramaPhase = IPadPanoramaMotionPhase.entering
    @State private var panoramaTransitionWindow: IPadSidebarWindowKey?
    @State private var panoramaWindowAtOverview = true
    @State private var panoramaMotionRevision = 0

    var body: some View {
        Group {
            if showsPanorama {
                ZStack {
                    IPadPanorama(
                        phase: panoramaPhase,
                        transitionNamespace: panoramaNamespace,
                        transitionWindow: panoramaTransitionWindow,
                        windowAtOverview: panoramaWindowAtOverview,
                        onClose: {
                            dismissPanorama(toward: selectedWindowKey)
                        }
                    )
                    .ignoresSafeArea(
                        .container,
                        edges: panoramaPhase == .exiting ? .top : []
                    )

                    if let key = panoramaTransitionWindow,
                       let session = session(for: key),
                       let window = session.windows.first(where: { $0.id == key.window }) {
                        IPadPanoramaWorkspaceSnapshot(session: session, window: window)
                            .matchedGeometryEffect(
                                id: key,
                                in: panoramaNamespace,
                                isSource: !panoramaWindowAtOverview
                            )
                            .opacity(panoramaWindowAtOverview ? 0 : 1)
                            .zIndex(3)
                    }
                }
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

                Button(action: showSettings) {
                    Label("Settings", systemImage: "gearshape")
                }
                .accessibilityIdentifier("settings")

                Menu {
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
                    Label("Connection", systemImage: "ellipsis")
                }
            }
        }
        .navigationBarTitleDisplayMode(.inline)
        .toolbarVisibility(
            showsPanorama && panoramaPhase != .exiting ? .hidden : .automatic,
            for: .navigationBar
        )
        .onAppear {
            if showsPanorama {
                presentPanorama()
            }
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
    }

    private func presentPanorama() {
        let origin = selectedWindowKey
        store.showOverview()
        guard !panoramaWindowKeys.isEmpty else {
            panoramaPhase = .entering
            panoramaTransitionWindow = nil
            panoramaWindowAtOverview = false
            showsPanorama = true
            return
        }
        startPanoramaEntrance(from: origin)
    }

    private func startPanoramaEntrance(from window: IPadSidebarWindowKey?) {
        panoramaMotionRevision += 1
        let revision = panoramaMotionRevision
        panoramaTransitionWindow = window
        panoramaWindowAtOverview = false

        if reduceMotion {
            panoramaPhase = .visible
            panoramaWindowAtOverview = true
            withAnimation(.easeOut(duration: 0.15)) {
                showsPanorama = true
            }
            panoramaTransitionWindow = nil
            return
        }

        panoramaPhase = .entering
        showsPanorama = true
        Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(45))
            guard showsPanorama, panoramaMotionRevision == revision else {
                return
            }
            withAnimation(.snappy(duration: 0.62, extraBounce: 0.08)) {
                panoramaPhase = .visible
                panoramaWindowAtOverview = true
            }
            try? await Task.sleep(for: .milliseconds(900))
            guard showsPanorama, panoramaMotionRevision == revision else {
                return
            }
            panoramaTransitionWindow = nil
        }
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
        var transaction = Transaction(animation: nil)
        transaction.disablesAnimations = true
        withTransaction(transaction) {
            panoramaPhase = .visible
            panoramaTransitionWindow = target
            panoramaWindowAtOverview = true
        }

        guard !reduceMotion else {
            withAnimation(.easeOut(duration: 0.15)) {
                showsPanorama = false
            }
            panoramaPhase = .entering
            panoramaTransitionWindow = nil
            panoramaWindowAtOverview = true
            return
        }

        Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(30))
            guard showsPanorama, panoramaMotionRevision == revision else {
                return
            }
            withAnimation(.smooth(duration: 0.46)) {
                panoramaPhase = .exiting
                panoramaWindowAtOverview = false
            }
            try? await Task.sleep(for: .milliseconds(460))
            guard showsPanorama, panoramaMotionRevision == revision else {
                return
            }
            withTransaction(transaction) {
                showsPanorama = false
                panoramaPhase = .entering
                panoramaTransitionWindow = nil
                panoramaWindowAtOverview = true
            }
        }
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

    private func session(for key: IPadSidebarWindowKey) -> ZZSession? {
        store.sessions.first(where: { $0.id == key.session })
    }

    private static let fullLayout = ZZPaneLayout(x: 0, y: 0, width: 1, height: 1)
}

private struct IPadStatusBar: View {
    @EnvironmentObject private var store: ZZStore

    var body: some View {
        if let session = store.selectedSession, !session.windows.isEmpty {
            HStack(spacing: 4) {
                Text("[\(session.name)]")
                    .font(.caption.monospaced().weight(.medium))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .frame(maxWidth: 96)

                ForEach(visibleWindows(in: session)) { window in
                    windowButton(window, session: session)
                }

                let overflow = overflowWindows(in: session)
                if !overflow.isEmpty {
                    Menu {
                        ForEach(overflow) { window in
                            Button {
                                open(window, in: session)
                            } label: {
                                Label(
                                    windowTitle(window),
                                    systemImage: window.isCurrent ? "checkmark" : "macwindow"
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
            .accessibilityElement(children: .contain)
            .accessibilityLabel("tmux status, session \(session.name)")
        }
    }

    private func windowButton(_ window: ZZWindow, session: ZZSession) -> some View {
        Button {
            open(window, in: session)
        } label: {
            HStack(spacing: 5) {
                if window.panes.contains(where: \.hasBell) {
                    Image(systemName: "bell.fill")
                        .foregroundStyle(.orange)
                }
                Text(windowTitle(window))
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .frame(maxWidth: 96)
                if window.zoomedPane != nil {
                    Image(systemName: "arrow.up.left.and.arrow.down.right")
                        .font(.caption2)
                }
            }
            .font(.caption.monospaced().weight(window.isCurrent ? .semibold : .regular))
            .foregroundStyle(window.isCurrent ? Color.primary : Color.secondary)
            .padding(.horizontal, 9)
            .frame(height: 28)
            .background(
                window.isCurrent ? Color.primary.opacity(0.12) : Color.clear,
                in: RoundedRectangle(cornerRadius: 7, style: .continuous)
            )
            .contentShape(RoundedRectangle(cornerRadius: 7, style: .continuous))
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Window \(window.index), \(window.name)")
        .accessibilityValue(window.isCurrent ? "Current" : "")
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
        "\(window.index):\(window.name.isEmpty ? "window" : window.name)"
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
            TerminalSurface(
                store: store,
                pane: pane.id,
                frame: store.frame(for: pane.id),
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
    let transitionNamespace: Namespace.ID
    let transitionWindow: IPadSidebarWindowKey?
    let windowAtOverview: Bool
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
                        ForEach(store.sessions) { session in
                            IPadPanoramaSessionColumn(
                                session: session,
                                sessionOrder: sessionOrder(for: session),
                                phase: phase,
                                transitionNamespace: transitionNamespace,
                                transitionWindow: transitionWindow,
                                windowAtOverview: windowAtOverview
                            )
                            .containerRelativeFrame(.horizontal) { length, _ in
                                min(max(length * 0.84, 340), 480)
                            }
                            .containerRelativeFrame(.vertical)
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
        .allowsHitTesting(phase != .exiting)
    }

    private func sessionOrder(for session: ZZSession) -> Int {
        store.sessions.firstIndex(where: { $0.id == session.id }) ?? 0
    }
}

private struct IPadPanoramaSessionColumn: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let session: ZZSession
    let sessionOrder: Int
    let phase: IPadPanoramaMotionPhase
    let transitionNamespace: Namespace.ID
    let transitionWindow: IPadSidebarWindowKey?
    let windowAtOverview: Bool

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
            .opacity(phase == .visible ? 1 : 0)
            .offset(y: phase == .visible ? 0 : -10)
            .animation(sessionLabelAnimation, value: phase)

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
                                sessionOrder: sessionOrder,
                                windowOrder: windowOrder(for: window),
                                phase: phase,
                                transitionNamespace: transitionNamespace,
                                transitionWindow: transitionWindow,
                                windowAtOverview: windowAtOverview
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

    private var sessionLabelAnimation: Animation? {
        guard !reduceMotion else {
            return nil
        }
        return phase == .visible
            ? .smooth(duration: 0.26).delay(Double(sessionOrder) * 0.04 + 0.08)
            : .smooth(duration: 0.18)
    }

    private func windowOrder(for window: ZZWindow) -> Int {
        session.windows.firstIndex(where: { $0.id == window.id }) ?? 0
    }
}

private struct IPadPanoramaWindowCard: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let session: ZZSession
    let window: ZZWindow
    let sessionOrder: Int
    let windowOrder: Int
    let phase: IPadPanoramaMotionPhase
    let transitionNamespace: Namespace.ID
    let transitionWindow: IPadSidebarWindowKey?
    let windowAtOverview: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(windowTitle)
                .font(.subheadline.weight(.semibold))
                .monospacedDigit()
                .lineLimit(1)
                .padding(.horizontal, 4)
                .accessibilityElement(children: .combine)
                .accessibilityLabel(windowAccessibilityLabel)

            IPadPanoramaWindowPreview(
                session: session,
                window: window
            )
            .background(Color(uiColor: .secondarySystemBackground))
            .compositingGroup()
            .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: 18, style: .continuous)
                    .stroke(
                        session.isAttached && window.isCurrent
                            ? Color.accentColor.opacity(0.48)
                            : Color.primary.opacity(0.11),
                        lineWidth: 1
                    )
            }
            .matchedGeometryEffect(
                id: windowKey,
                in: transitionNamespace,
                isSource: !isTransitionTarget || windowAtOverview
            )
        }
        .scaleEffect(motionScale)
        .offset(y: motionOffset)
        .blur(radius: motionBlur)
        .opacity(motionOpacity)
        .zIndex(isTransitionTarget ? 1 : 0)
        .animation(motionAnimation, value: phase)
        .animation(motionAnimation, value: windowAtOverview)
    }

    private var windowTitle: String {
        "\(window.index):\(window.name.isEmpty ? "window" : window.name)"
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

    private var isExitTarget: Bool {
        phase == .exiting && isTransitionTarget
    }

    private var isTransitionTarget: Bool {
        transitionWindow == windowKey
    }

    private var windowKey: IPadSidebarWindowKey {
        IPadSidebarWindowKey(session: session.id, window: window.id)
    }

    private var motionScale: CGFloat {
        switch phase {
        case .entering:
            isTransitionTarget ? 1 : 1.55
        case .visible:
            1
        case .exiting:
            isExitTarget ? 1 : 0.86
        }
    }

    private var motionOffset: CGFloat {
        switch phase {
        case .entering:
            isTransitionTarget ? 0 : 22 + CGFloat(windowOrder) * 6
        case .visible:
            0
        case .exiting:
            isExitTarget ? 0 : 18
        }
    }

    private var motionBlur: CGFloat {
        switch phase {
        case .entering:
            isTransitionTarget ? 0 : 5
        case .visible:
            0
        case .exiting:
            isExitTarget ? 0 : 5
        }
    }

    private var motionOpacity: Double {
        switch phase {
        case .entering:
            isTransitionTarget ? (windowAtOverview ? 1 : 0) : 0.28
        case .visible:
            1
        case .exiting:
            isExitTarget ? (windowAtOverview ? 1 : 0) : 0
        }
    }

    private var motionAnimation: Animation? {
        guard !reduceMotion else {
            return nil
        }
        if phase == .visible {
            let delay = Double(sessionOrder) * 0.04 + Double(windowOrder) * 0.055
            return .snappy(duration: 0.62, extraBounce: 0.08).delay(delay)
        }
        return .smooth(duration: 0.46)
    }
}

private struct IPadPanoramaWorkspaceSnapshot: View {
    let session: ZZSession
    let window: ZZWindow

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
                        IPadPanoramaPanePreview(
                            session: session,
                            window: window,
                            pane: pane
                        )
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
            .compositingGroup()
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
        if pane.kind == .terminal, let frame = store.frame(for: pane.id) {
            TerminalSurface(
                store: store,
                pane: pane.id,
                frame: frame,
                interactive: false,
                preview: true
            )
        } else if pane.kind == .agent, store.agentState(for: pane.id) != nil {
            AgentPaneSummary(pane: pane)
        } else {
            VStack(spacing: 7) {
                if pane.kind == .terminal, session.isAttached, window.isCurrent {
                    ProgressView()
                        .controlSize(.small)
                } else {
                    Image(systemName: pane.kind.symbol)
                        .font(.title3)
                }
                Text(placeholderLabel)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
                    .multilineTextAlignment(.center)
            }
            .padding(8)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private var placeholderLabel: String {
        if pane.kind == .terminal, session.isAttached, window.isCurrent {
            return "Waiting for frame"
        }
        if pane.kind == .browser || pane.kind == .editor || pane.kind == .picker {
            return "Open on desktop"
        }
        return "Tap to attach"
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
    var bottomAccessoryInset: CGFloat = 0
    @State private var draft = ""

    var body: some View {
        let state = store.agentState(for: pane.id)

        ScrollView {
            LazyVStack(alignment: .leading, spacing: 14) {
                if let state {
                    AgentStatusHeader(pane: pane, state: state)
                    AgentActivityRow(state: state)
                    if let permission = state.permission {
                        AgentPermissionCard(pane: pane.id, permission: permission)
                    }
                    if let error = state.error, !error.isEmpty {
                        AgentErrorCard(message: error)
                    }
                    if let git = state.git {
                        AgentGitCard(git: git)
                    }
                } else {
                    ContentUnavailableView {
                        Label("Connecting to Agent", systemImage: "sparkles")
                    } description: {
                        Text("Waiting for the daemon’s retained Agent state.")
                    }
                    .frame(maxWidth: .infinity, minHeight: 260)
                }
            }
            .padding(20)
            .frame(maxWidth: 720)
            .frame(maxWidth: .infinity)
        }
        .scrollIndicators(.hidden)
        .safeAreaInset(edge: .bottom, spacing: 0) {
            AgentComposer(
                pane: pane.id,
                phase: state?.phase ?? .starting,
                queuedPrompts: state?.queuedPrompts ?? 0,
                draft: $draft
            )
            .padding(.bottom, bottomAccessoryInset)
        }
        .background(Color.zzCard)
        .onAppear {
            draft = store.agentDraft(for: pane.id)
        }
        .onChange(of: pane.id) { _, paneID in
            draft = store.agentDraft(for: paneID)
        }
        .onChange(of: draft) { _, text in
            store.saveAgentDraft(text, for: pane.id)
        }
    }
}

private struct AgentStatusHeader: View {
    let pane: ZZPane
    let state: ZZAgentState

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: symbol)
                .font(.title3)
                .foregroundStyle(color)
                .symbolEffect(.pulse, isActive: state.status == .working)
                .frame(width: 28)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(state.title ?? (pane.title.isEmpty ? "Agent" : pane.title))
                    .font(.headline)
                    .lineLimit(1)
                Text(state.phase.label)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 8)
            if state.queuedPrompts > 0 {
                Text("\(state.queuedPrompts) queued")
                    .font(.caption.weight(.medium))
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 9)
                    .frame(height: 26)
                    .background(Color.primary.opacity(0.07), in: Capsule())
            }
        }
        .padding(14)
        .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
        .accessibilityElement(children: .combine)
    }

    private var symbol: String {
        switch state.status {
        case .idle: "checkmark.circle.fill"
        case .working: "sparkles"
        case .needsInput: "hand.raised.fill"
        case .failed: "exclamationmark.triangle.fill"
        }
    }

    private var color: Color {
        switch state.status {
        case .idle: .green
        case .working: .accentColor
        case .needsInput: .orange
        case .failed: .red
        }
    }
}

private struct AgentActivityRow: View {
    let state: ZZAgentState

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            activitySymbol
                .frame(width: 24, height: 24)
            VStack(alignment: .leading, spacing: 4) {
                Text(title)
                    .font(.body.weight(.semibold))
                Text(detail)
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.horizontal, 4)
        .padding(.vertical, 8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .combine)
    }

    @ViewBuilder
    private var activitySymbol: some View {
        if state.phase == .starting || state.phase == .running {
            ProgressView()
                .controlSize(.small)
        } else {
            Image(systemName: symbol)
                .foregroundStyle(color)
        }
    }

    private var title: String {
        switch state.phase {
        case .starting: "Starting Agent"
        case .ready: "Ready for a prompt"
        case .running: "Agent is working"
        case .awaitingPermission: "Waiting for your approval"
        case .failed: "Agent needs attention"
        }
    }

    private var detail: String {
        switch state.phase {
        case .starting: "Connecting to the configured ACP adapter."
        case .ready: "Ask about the current workspace or queue the next task."
        case .running:
            state.queuedPrompts == 0
                ? "You can queue a follow-up while this turn runs."
                : "\(state.queuedPrompts) follow-up \(state.queuedPrompts == 1 ? "is" : "are") queued."
        case .awaitingPermission: "Choose an option below, queue another prompt, or stop the turn."
        case .failed: "Review the error below before trying again."
        }
    }

    private var symbol: String {
        switch state.phase {
        case .ready: "text.bubble.fill"
        case .awaitingPermission: "hand.raised.fill"
        case .failed: "exclamationmark.triangle.fill"
        case .starting, .running: "sparkles"
        }
    }

    private var color: Color {
        switch state.phase {
        case .ready: .accentColor
        case .awaitingPermission: .orange
        case .failed: .red
        case .starting, .running: .secondary
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
                    AgentPaneView(pane: pane, bottomAccessoryInset: 72)
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
