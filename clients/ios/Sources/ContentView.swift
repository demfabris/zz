import SwiftUI

struct ContentView: View {
    @EnvironmentObject private var store: ZZStore
    @Namespace private var paneTransition

    var body: some View {
        Group {
            switch store.connectionState {
            case .idle, .connecting:
                ProgressView("Connecting to zz")
                    .controlSize(.large)
            case let .failed(message):
                ConnectionFailure(message: message)
            case .disconnected:
                ConnectionFailure(message: "The daemon disconnected.")
            case .connected:
                if let pane = store.selectedPane {
                    FullscreenPane(pane: pane, namespace: paneTransition)
                } else {
                    PaneOverview(namespace: paneTransition)
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

    var body: some View {
        ZStack(alignment: .bottom) {
            Group {
                if pane.kind == .terminal {
                    TerminalSurface(
                        store: store,
                        pane: pane.id,
                        frame: store.frame(for: pane.id),
                        interactive: true,
                        preview: false
                    )
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
                TerminalShortcutButton("Ctrl", selected: store.controlModifierEnabled) {
                    store.toggleControlModifier()
                }
                TerminalShortcutButton("Alt", selected: store.altModifierEnabled) {
                    store.toggleAltModifier()
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
            }
            .padding(.horizontal, 8)
        }
        .scrollIndicators(.hidden)
        .frame(height: 48)
        .glassEffect(.regular, in: Capsule())
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
    let action: () -> Void

    init(_ title: String, selected: Bool = false, action: @escaping () -> Void) {
        self.title = title
        self.selected = selected
        self.action = action
    }

    var body: some View {
        Button(action: action) {
            Text(title)
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
        .accessibilityIdentifier("shortcut-\(title.lowercased())")
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
