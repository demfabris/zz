import AppKit
import SwiftUI
import ZZNativeCore
import ZZUI

@main
struct ZZNativeApp: App {
    @NSApplicationDelegateAdaptor(NativeAppDelegate.self) private var delegate

    var body: some Scene {
        Window("zz Native", id: "workspace") {
            NativeWorkspace().frame(minWidth: 720, minHeight: 440)
        }
        .defaultSize(width: 1280, height: 840)
        .windowStyle(.hiddenTitleBar)
        .windowResizability(.contentMinSize)
    }
}

@MainActor
final class NativeAppDelegate: NSObject, NSApplicationDelegate {
    func applicationWillFinishLaunching(_ notification: Notification) {
        NSWindow.allowsAutomaticWindowTabbing = false
    }
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}

struct NativeWorkspace: View {
    @State private var client: NativeClient
    @State private var sidebarVisible = true
    @State private var connectionVisible = false
    @State private var hostExpanded = true
    @State private var draftEndpoint = ""
    @State private var draftSession = ""
    @State private var renameTarget: UInt64?
    @State private var renameText = ""
    private var theme: ZZTheme {
        var theme = ZZTheme.macOSClassicDark
        theme.radius = 24
        theme.foreground = ZZColor(red: 1, green: 1, blue: 1)
        return theme
    }

    init() {
        let arguments = ProcessInfo.processInfo.arguments
        func argument(_ name: String) -> String? {
            guard let index = arguments.firstIndex(of: name), index + 1 < arguments.count else { return nil }
            return arguments[index + 1]
        }
        _client = State(
            initialValue: NativeClient(endpoint: argument("--socket"), session: argument("--session") ?? ""))
    }

    var body: some View {
        ZZThemeContainer(theme: theme) {
            ZZAppShell(sidebarVisible: $sidebarVisible) {
                sidebar
            } content: {
                VStack(spacing: 0) {
                    toolbar
                    workspace
                }
            } status: {
                status
            }
            .background { ZZWindowBackground(tint: theme.background.opacity(0.93)) }
            .sheet(isPresented: $connectionVisible) { connectionPanel }
            .sheet(isPresented: Binding(get: { renameTarget != nil }, set: { if !$0 { renameTarget = nil } })) {
                ZZDialog("Rename window") {
                    ZZTextField("Name", text: $renameText)
                } actions: {
                    ZZButton("Cancel") { renameTarget = nil }.keyboardShortcut(.cancelAction)
                    ZZButton("Rename", variant: .primary) {
                        if let target = renameTarget {
                            client.execute("rename-window", ["-t", "@\(target)", renameText])
                        }
                        renameTarget = nil
                    }.keyboardShortcut(.defaultAction)
                }
            }
            .alert(
                "Command failed",
                isPresented: Binding(get: { client.commandError != nil }, set: { if !$0 { client.clearError() } })
            ) {
                Button("OK") { client.clearError() }
            } message: {
                Text(client.commandError ?? "")
            }
        }
        .preferredColorScheme(.dark)
        .ignoresSafeArea()
        .onAppear { client.connect() }
        .onDisappear { client.disconnect() }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.willTerminateNotification)) { _ in
            client.disconnect()
        }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            client.setFocused(true)
        }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didResignActiveNotification)) { _ in
            client.setFocused(false)
        }
    }

    private var sidebar: some View {
        ZZWorkspaceSidebar(transparent: true) {
            HStack(spacing: 4) {
                Color.clear.frame(width: 72)
                ZZIconButton("Connection", systemName: "network") { showConnection() }
                ZZIconButton("Hide sidebar", systemName: "sidebar.left") { sidebarVisible = false }
                Spacer(minLength: 0)
            }
        } content: {
            VStack(spacing: 0) {
                ZZWorkspaceTreeRow(
                    "Local host", icon: "desktopcomputer", connected: client.connected,
                    expanded: $hostExpanded, connecting: client.connecting,
                    connectionDetail: client.connected ? nil : client.message,
                    showConnectionDetail: { showConnection() }, action: { showConnection() })
                if hostExpanded {
                    ForEach(client.sessions) { session in
                        ZZWorkspaceTreeRow(session.name, icon: "square.3.layers.3d", depth: 1, active: session.attached)
                        {
                            client.attach(session)
                        }
                        if session.attached {
                            ForEach(session.windows) { window in
                                ZZWorkspaceTreeRow(
                                    window.name, icon: "rectangle.split.2x1", depth: 2, active: window.current
                                ) {
                                    client.selectWindow(window.id)
                                }
                                if window.current {
                                    ForEach(window.panes) { pane in
                                        ZZWorkspaceTreeRow(
                                            pane.title.isEmpty ? "Pane \(pane.id)" : pane.title,
                                            icon: pane.kind == 1 ? "terminal" : "square", depth: 3,
                                            selected: pane.active
                                        ) {
                                            client.selectPane(pane.id)
                                        }
                                    }
                                }
                            }
                            ZZWorkspaceActionRow("New window", depth: 2) {
                                client.execute("new-window", ["-t", "$\(session.id)"])
                            }
                        }
                    }
                    ZZWorkspaceActionRow("New session", depth: 1) { client.execute("new-session") }
                        .disabled(!client.connected)
                }
            }
        }
        .frame(width: 256)
    }

    private var toolbar: some View {
        HStack(spacing: 8) {
            if !sidebarVisible {
                Color.clear.frame(width: 72)
                ZZIconButton("Show sidebar", systemName: "sidebar.left") { sidebarVisible = true }
            }
            Text(client.currentWindow?.name ?? "zz").font(theme.font(size: 12)).lineLimit(1)
            Spacer()
            ZZIconButton("Split right", systemName: "rectangle.split.2x1") { split(horizontal: true) }
                .disabled(client.activePane == nil || !client.connected)
            ZZIconButton("Split down", systemName: "rectangle.split.1x2") { split(horizontal: false) }
                .disabled(client.activePane == nil || !client.connected)
            ZZIconButton("Zoom pane", systemName: "arrow.up.left.and.arrow.down.right") {
                if let pane = client.activePane { client.execute("resize-pane", ["-Z", "-t", "%\(pane.id)"]) }
            }.disabled(client.activePane == nil || !client.connected)
            ZZIconButton("Connection", systemName: "network") { showConnection() }
        }
        .padding(.horizontal, 10).frame(height: 35)
    }

    @ViewBuilder private var workspace: some View {
        if let window = client.currentWindow, !window.panes.isEmpty {
            GeometryReader { geometry in
                ZStack(alignment: .topLeading) {
                    ForEach(window.panes) { pane in
                        ZZPane(active: pane.active, gap: 6, radius: 24) {
                            if pane.kind == 1 {
                                TerminalSurface(
                                    client: client, pane: pane, slot: client.slot(for: pane.id),
                                    generation: client.connectionGeneration
                                )
                                .padding(10)
                            } else if pane.kind == 0 {
                                ZZConnectionState("Choose a pane type") {
                                    ZZButton("Terminal", icon: "terminal", variant: .primary) {
                                        client.execute("select-pane-kind", ["-t", "%\(pane.id)", "terminal"])
                                    }
                                }
                            } else {
                                ZZConnectionState("This pane is not available in the native client yet.")
                            }
                        }
                        .frame(
                            width: geometry.size.width * pane.rect.width,
                            height: geometry.size.height * pane.rect.height
                        )
                        .offset(x: geometry.size.width * pane.rect.minX, y: geometry.size.height * pane.rect.minY)
                    }
                }
            }
            .padding(3)
            .overlay(alignment: .bottom) {
                if !client.connected {
                    HStack {
                        Text(client.message).font(theme.font(size: 12))
                        ZZButton("Reconnect") { client.connect() }
                    }.padding(10).background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12)).padding()
                }
                if client.prefixArmed {
                    ScrollView {
                        VStack(alignment: .leading, spacing: 4) {
                            Text("Prefix").font(.headline)
                            ForEach(client.prefixHints, id: \.self) {
                                Text($0).font(.system(size: 11, design: .monospaced))
                            }
                        }.padding(12)
                    }.frame(maxWidth: 430, maxHeight: 240).background(
                        .regularMaterial, in: RoundedRectangle(cornerRadius: 12)
                    ).padding()
                }
            }
        } else {
            ZZConnectionState(client.message, connecting: client.connecting) {
                ZZButton("Connection settings", icon: "network") { showConnection() }
                if client.connected { ZZButton("New session", variant: .primary) { client.execute("new-session") } }
            }
        }
    }

    private func split(horizontal: Bool) {
        if let pane = client.activePane {
            client.execute("split-window", [horizontal ? "-h" : "-v", "-t", "%\(pane.id)"])
        }
    }

    private var status: some View {
        ZZWorkspaceStatusBar {
            ZZWorkspaceStatusItem(client.attachedSession?.name ?? "Disconnected", icon: "square.3.layers.3d")
        } windows: {
            ForEach(client.attachedSession?.windows ?? []) { window in
                ZZWorkspaceStatusWindow(
                    window.name, index: String(window.index), active: window.current, connected: client.connected,
                    close: { client.execute("kill-window", ["-t", "@\(window.id)"]) },
                    rename: {
                        renameText = window.name
                        renameTarget = window.id
                    }
                ) {
                    client.selectWindow(window.id)
                }
            }
        } trailing: {
            ZZIconButton("New window", systemName: "plus") { client.execute("new-window") }.disabled(!client.connected)
        }
    }

    private func showConnection() {
        draftEndpoint = client.endpoint
        draftSession = client.sessionTarget
        connectionVisible = true
    }

    private var connectionPanel: some View {
        ZZDialog("Connect to zz", description: "Connect to a running daemon using its socket path.") {
            ZZTextField("Socket", text: $draftEndpoint, placeholder: "/tmp/zz.sock")
            ZZTextField("Session", text: $draftSession, placeholder: "Default session")
            Text(client.message).font(.system(size: 12)).foregroundStyle(.secondary).textSelection(.enabled)
        } actions: {
            ZZButton("Disconnect") { client.disconnect() }
            ZZButton("Cancel") { connectionVisible = false }.keyboardShortcut(.cancelAction)
            ZZButton("Connect", variant: .primary) {
                client.endpoint = draftEndpoint
                client.sessionTarget = draftSession
                client.connect()
                connectionVisible = false
            }
            .keyboardShortcut(.defaultAction).disabled(draftEndpoint.isEmpty)
        }
    }
}
