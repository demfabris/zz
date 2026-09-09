import AppKit
import SwiftUI
import ZZNativeCore
import ZZUI

@main
struct ZZNativeApp: App {
    @NSApplicationDelegateAdaptor(NativeAppDelegate.self) private var delegate

    var body: some Scene {
        Window("zz Native", id: "workspace") {
            NativeWorkspace { delegate.configure($0) }.frame(minWidth: 720, minHeight: 440)
        }
        .defaultSize(width: 1280, height: 840)
        .windowStyle(.hiddenTitleBar)
        .windowResizability(.contentMinSize)
        .commands {
            CommandGroup(replacing: .appSettings) {
                Button("Settings…") { delegate.client?.settings.visible = true }
            }
        }
    }
}

@MainActor
final class NativeAppDelegate: NSObject, NSApplicationDelegate {
    weak var client: NativeClient?
    private var shutdownTimer: Timer?
    private var statusItem: NSStatusItem?

    func configure(_ client: NativeClient) {
        self.client = client
        if client.settings.bool("tray"), statusItem == nil {
            let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
            item.button?.image = NSImage(systemSymbolName: "terminal", accessibilityDescription: "zz")
            let menu = NSMenu()
            let show = NSMenuItem(title: "Show zz", action: #selector(showWorkspace), keyEquivalent: "")
            show.target = self
            menu.addItem(show)
            let settings = NSMenuItem(title: "Settings…", action: #selector(showSettings), keyEquivalent: "")
            settings.target = self
            menu.addItem(settings)
            menu.addItem(.separator())
            menu.addItem(
                NSMenuItem(title: "Quit zz", action: #selector(NSApplication.terminate(_:)), keyEquivalent: ""))
            item.menu = menu
            statusItem = item
        } else if !client.settings.bool("tray"), let item = statusItem {
            NSStatusBar.system.removeStatusItem(item)
            statusItem = nil
        }
    }
    @objc private func showWorkspace() {
        NSApp.activate(ignoringOtherApps: true)
        NSApp.windows.first { $0.title == "zz Native" }?.makeKeyAndOrderFront(nil)
    }
    @objc private func showSettings() { showWorkspace(); client?.settings.visible = true }

    private var pointerMonitor: Any?
    private weak var pointerCapture: BrowserView?
    private var keyMonitor: Any?
    private var claimedKeys: [UInt16: UInt64] = [:]
    private let chrome = NativeChrome()
    func applicationWillFinishLaunching(_ notification: Notification) {
        NSWindow.allowsAutomaticWindowTabbing = false
    }
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
        pointerMonitor = NSEvent.addLocalMonitorForEvents(matching: [
            .leftMouseDown, .leftMouseUp, .leftMouseDragged,
            .rightMouseDown, .rightMouseUp, .rightMouseDragged, .otherMouseDown, .otherMouseUp, .otherMouseDragged,
            .scrollWheel,
        ]) { [weak self] event in
            let handled = MainActor.assumeIsolated { self?.routePointer(event) == true }
            return handled ? nil : event
        }
        keyMonitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .keyUp]) { [weak self] event in
            let handled = MainActor.assumeIsolated {
                guard let self, let client = self.client else { return false }
                if event.type == .keyUp {
                    guard let pane = self.claimedKeys.removeValue(forKey: event.keyCode) else { return false }
                    client.key(
                        pane, code: TerminalKey.code(event),
                        scalar: event.charactersIgnoringModifiers?.unicodeScalars.first?.value ?? 0,
                        function: TerminalKey.function(event), action: 2,
                        modifiers: TerminalKey.modifiers(event.modifierFlags))
                    return true
                }
                guard event.window?.title == "zz Native", event.window?.attachedSheet == nil else { return false }
                self.chrome.configure(client.settings)
                if self.chrome.resolve(event, table: "ui") == "open-settings" {
                    client.settings.visible = true; return true
                }
                let uiAction = self.chrome.resolve(event, table: "ui")
                @MainActor func zoom() -> Bool {
                    switch uiAction {
                    case "ui-zoom-in": client.settings.setUIZoom(client.settings.uiZoom + 10)
                    case "ui-zoom-out": client.settings.setUIZoom(client.settings.uiZoom - 10)
                    case "ui-zoom-reset": client.settings.setUIZoom(100)
                    default: return false
                    }
                    return true
                }
                guard let active = client.activePane else { return zoom() }
                if client.claimsPrefix(
                    code: TerminalKey.code(event),
                    scalar: event.charactersIgnoringModifiers?.unicodeScalars.first?.value ?? 0,
                    function: TerminalKey.function(event), modifiers: TerminalKey.modifiers(event.modifierFlags))
                {
                    if !event.isARepeat {
                        self.claimedKeys[event.keyCode] = active.id
                        client.key(
                            active.id, code: TerminalKey.code(event),
                            scalar: event.charactersIgnoringModifiers?.unicodeScalars.first?.value ?? 0,
                            function: TerminalKey.function(event),
                            modifiers: TerminalKey.modifiers(event.modifierFlags), text: event.characters ?? "")
                    }
                    return true
                }
                if let agent = client.agents[active.id] {
                    let editingText = (event.window?.firstResponder as? NSTextView)?.isEditable == true
                    if editingText, event.charactersIgnoringModifiers?.lowercased() == "v",
                        event.modifierFlags.intersection([.command, .control, .option, .shift]) == .command,
                        agent.pasteImages()
                    {
                        return true
                    }
                    return agent.handleKey(event, editingText: editingText) || zoom()
                }
                guard let pane = client.browsers.panes[active.id],
                    let action = self.chrome.resolve(event, table: "browser")
                else { return zoom() }
                return client.browsers.performChrome(
                    action, pane: pane, editingText: event.window?.firstResponder is NSTextView)
            }
            return handled ? nil : event
        }
    }
    private func routePointer(_ event: NSEvent) -> Bool {
        guard let window = event.window, window.title == "zz Native", window.attachedSheet == nil,
            let content = window.contentView
        else { return false }
        let down = [.leftMouseDown, .rightMouseDown, .otherMouseDown].contains(event.type)
        let up = [.leftMouseUp, .rightMouseUp, .otherMouseUp].contains(event.type)
        let hit = content.hitTest(content.convert(event.locationInWindow, from: nil)) as? BrowserView
        guard let browser = (down || event.type == .scrollWheel) ? hit : (pointerCapture ?? hit),
            browser.window === window
        else { return false }
        if down { pointerCapture = browser }
        defer { if up { pointerCapture = nil } }
        switch event.type {
        case .leftMouseDown: browser.mouseDown(with: event)
        case .leftMouseUp: browser.mouseUp(with: event)
        case .leftMouseDragged: browser.mouseDragged(with: event)
        case .rightMouseDown: browser.rightMouseDown(with: event)
        case .rightMouseUp: browser.rightMouseUp(with: event)
        case .rightMouseDragged: browser.rightMouseDragged(with: event)
        case .otherMouseDown: browser.otherMouseDown(with: event)
        case .otherMouseUp: browser.otherMouseUp(with: event)
        case .otherMouseDragged: browser.otherMouseDragged(with: event)
        case .scrollWheel: browser.scrollWheel(with: event)
        default: return false
        }
        return true
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        client?.settings.bool("tray") != true
    }
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard let client else { return .terminateNow }
        guard shutdownTimer == nil else { return .terminateLater }
        if client.settings.bool("quit-daemon-on-exit") { client.execute("kill-server") }
        client.disconnect()
        let timer = Timer(timeInterval: 0.01, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                if self.client?.browsers.shutdown() != false {
                    self.shutdownTimer?.invalidate()
                    self.shutdownTimer = nil
                    NSApp.reply(toApplicationShouldTerminate: true)
                }
            }
        }
        shutdownTimer = timer
        RunLoop.main.add(timer, forMode: .common)
        return .terminateLater
    }
}

struct NativeWorkspace: View {
    private let onClient: (NativeClient) -> Void
    @State private var client: NativeClient
    @State private var sidebarVisible = true
    @State private var connectionVisible = false
    @State private var hostExpanded = true
    @State private var draftEndpoint = ""
    @State private var draftSession = ""
    @State private var renameTarget: UInt64?
    @State private var renameText = ""
    @Environment(\.colorScheme) private var systemColorScheme
    private var dark: Bool {
        switch client.settings.text("theme-mode") {
        case "light": false
        case "dark": true
        default: systemColorScheme == .dark
        }
    }
    private var theme: ZZTheme {
        let settings = client.settings
        var theme = dark ? ZZTheme.macOSClassicDark : ZZTheme.macOSClassicLight
        if let preset = settings.snapshot?.presets.first(where: { $0.id == settings.text("chrome-preset") }) {
            let colors = dark ? preset.dark : preset.light
            theme.background = ZZColor(hex: colors[0]) ?? theme.background
            theme.foreground = ZZColor(hex: colors[1]) ?? theme.foreground
            theme.success = ZZColor(hex: colors[2]) ?? theme.success
            theme.warning = ZZColor(hex: colors[3]) ?? theme.warning
            theme.danger = ZZColor(hex: colors[4]) ?? theme.danger
        }
        theme.background = ZZColor(hex: settings.text("chrome-background")) ?? theme.background
        theme.foreground = ZZColor(hex: settings.text("chrome-foreground")) ?? theme.foreground
        theme.success = ZZColor(hex: settings.text("chrome-success")) ?? theme.success
        theme.warning = ZZColor(hex: settings.text("chrome-warning")) ?? theme.warning
        theme.danger = ZZColor(hex: settings.text("chrome-danger")) ?? theme.danger
        theme.radius = settings.number("widget-corner-radius", fallback: 6)
        theme.fontFamily = settings.text("ui-font-family")
        theme.shadowStrength = settings.number("shadow-strength", fallback: 1)
        return theme
    }

    init(onClient: @escaping (NativeClient) -> Void) {
        self.onClient = onClient
        let arguments = ProcessInfo.processInfo.arguments
        func argument(_ name: String) -> String? {
            guard let index = arguments.firstIndex(of: name), index + 1 < arguments.count else { return nil }
            return arguments[index + 1]
        }
        _client = State(
            initialValue: NativeClient(
                endpoint: argument("--socket"), session: argument("--session") ?? "", config: argument("--config"),
                muxConfig: argument("--mux-config")))
    }

    var body: some View {
        ZZThemeContainer(theme: theme) {
            ZZAppShell(
                sidebarVisible: $sidebarVisible,
                backgroundOpacity: client.settings.bool("window-background-blur") ? 0.88 : 1
            ) {
                sidebar
            } content: {
                workspace
            } status: {
                status
            }
            .modifier(NativeInterfaceZoom(scale: client.settings.uiZoom / 100))
            .background {
                ZZWindowBackground(
                    tint: theme.background.opacity(client.settings.bool("window-background-blur") ? 0.1 : 1),
                    enabled: client.settings.bool("window-background-blur"))
            }
            .background { NativeWindowOptions(settings: client.settings, dark: dark) }
            .transaction { if !client.settings.bool("animations") { $0.disablesAnimations = true } }
            .sheet(isPresented: Binding(get: { client.settings.visible }, set: { client.settings.visible = $0 })) {
                NativeSettingsView(client: client, model: client.settings).onAppear { client.cancelPrefix() }
            }
            .sheet(isPresented: $connectionVisible) { connectionPanel.onAppear { client.cancelPrefix() } }
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
        .preferredColorScheme(client.settings.text("theme-mode") == "system" ? nil : (dark ? .dark : .light))
        .ignoresSafeArea()
        .onAppear {
            onClient(client); client.setDarkAppearance(dark); client.connect()
        }
        .onDisappear { client.disconnect() }
        .onChange(of: client.settings.snapshot?.revision) { _, _ in onClient(client) }
        .onChange(of: dark) { _, value in client.setDarkAppearance(value) }
        .onChange(of: client.connectionGeneration) { _, _ in client.setDarkAppearance(dark) }
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
                Color.clear.frame(width: client.settings.bool("use-system-titlebar") ? 0 : 72)
                ZZIconButton("Hide sidebar", systemName: "sidebar.left") { sidebarVisible = false }
                ZZIconButton("Settings", systemName: "gearshape") { client.settings.visible = true }
                Spacer(minLength: 0)
            }
        } content: {
            VStack(spacing: 0) {
                ZZWorkspaceTreeRow(
                    "Local host", icon: "desktopcomputer", connected: client.connected,
                    expanded: $hostExpanded, connecting: client.connecting,
                    connectionDetail: client.connected ? nil : client.message,
                    showConnectionDetail: { showConnection() }, action: { showConnection() }
                ) {
                    ZZIconButton("New session", systemName: "plus") { client.execute("new-session") }
                        .disabled(!client.connected)
                }
                if hostExpanded {
                    ForEach(client.sessions) { session in
                        ZZWorkspaceTreeRow(session.name, icon: "square.3.layers.3d", depth: 1, active: session.attached)
                        {
                            client.attach(session)
                        } actions: {
                            ZZIconButton("New window in \(session.name)", systemName: "plus") {
                                client.execute("new-window", ["-t", "$\(session.id)"])
                            }
                            ZZIconButton("Close session \(session.name)", systemName: "xmark") {
                                client.execute("kill-session", ["-t", "$\(session.id)"])
                            }
                        }
                        .disabled(!client.connected)
                        if session.attached {
                            ForEach(session.windows) { window in
                                ZZWorkspaceTreeRow(
                                    window.name, icon: "rectangle.split.2x1", depth: 2, active: window.current
                                ) {
                                    client.selectWindow(window.id)
                                } actions: {
                                    ZZWindowLayoutMenu { axis in
                                        if let pane = window.panes.first(where: \.active) ?? window.panes.first {
                                            client.execute(
                                                "split-picker",
                                                [axis == .horizontal ? "-h" : "-v", "-t", "%\(pane.id)"])
                                        }
                                    }
                                    ZZIconButton("Close window \(window.name)", systemName: "xmark") {
                                        client.execute("kill-window", ["-t", "@\(window.id)"])
                                    }
                                }
                                .disabled(!client.connected)
                                if window.current {
                                    ForEach(window.panes) { pane in
                                        ZZWorkspaceTreeRow(
                                            pane.title.isEmpty ? "Pane \(pane.id)" : pane.title,
                                            icon: pane.kind == 1 ? "terminal" : "square", depth: 3,
                                            selected: pane.active
                                        ) {
                                            client.selectPane(pane.id)
                                        } actions: {
                                            ZZIconButton("Close pane \(pane.id)", systemName: "xmark") {
                                                client.execute("kill-pane", ["-t", "%\(pane.id)"])
                                            }
                                        }
                                        .disabled(!client.connected)
                                    }
                                }
                            }
                        }
                    }
                }
                ZZWorkspaceActionRow("Add host") {
                    client.settings.section = "hosts"
                    client.settings.visible = true
                }
            }
        }
    }

    private var paneActions: some View {
        HStack(spacing: 8) {
            ZZIconButton("Split right", systemName: "rectangle.split.2x1") { split(horizontal: true) }
                .disabled(client.activePane == nil || !client.connected)
            ZZIconButton("Split down", systemName: "rectangle.split.1x2") { split(horizontal: false) }
                .disabled(client.activePane == nil || !client.connected)
            ZZIconButton("Zoom pane", systemName: "arrow.up.left.and.arrow.down.right") {
                if let pane = client.activePane { client.execute("resize-pane", ["-Z", "-t", "%\(pane.id)"]) }
            }.disabled(client.activePane == nil || !client.connected)
        }
    }

    @ViewBuilder private var workspace: some View {
        if let window = client.currentWindow, !window.panes.isEmpty {
            GeometryReader { geometry in
                ZStack(alignment: .topLeading) {
                    ForEach(window.panes) { pane in
                        ZZPane(
                            active: pane.active,
                            gap: client.settings.bool("pane-gaps")
                                ? client.settings.number("pane-margin", fallback: 6) : 0,
                            inactiveOpacity: pane.active
                                ? 1 : client.settings.number("pane-inactive-opacity", fallback: 1),
                            borderWidth: client.settings.bool("pane-gaps")
                                ? client.settings.number("pane-border-width", fallback: 0.5) : 0,
                            radius: client.settings.number("pane-corner-radius", fallback: 24)
                        ) {
                            if pane.kind == 1 {
                                TerminalSurface(
                                    client: client, pane: pane, slot: client.slot(for: pane.id),
                                    generation: client.connectionGeneration
                                )
                            } else if pane.kind == 0 {
                                ZZConnectionState("Choose a pane type") {
                                    ZZButton("Terminal", icon: "terminal", variant: .primary) {
                                        client.execute("select-pane-kind", ["-t", "%\(pane.id)", "terminal"])
                                    }
                                    if client.settings.bool("experimental-agent-pane") {
                                        ZZButton("Agent", icon: "sparkles") {
                                            client.execute("select-pane-kind", ["-t", "%\(pane.id)", "agent"])
                                        }
                                    }
                                    ZZButton("Browser", icon: "globe") {
                                        client.execute("select-pane-kind", ["-t", "%\(pane.id)", "browser"])
                                    }
                                }
                            } else if pane.kind == 2, let browser = client.browsers.panes[pane.id] {
                                NativeBrowserPaneView(client: client, pane: pane, model: browser)
                            } else if pane.kind == 3, let agent = client.agents[pane.id] {
                                NativeAgentPaneView(client: client, pane: pane, model: agent)
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
            .padding(client.settings.bool("pane-gaps") ? 3 : 0)
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
        ZZWorkspaceStatusBar(windowAlignment: client.settings.text("status-align") == "left" ? .leading : .center) {
            HStack(spacing: 6) {
                if !sidebarVisible {
                    Color.clear.frame(width: client.settings.bool("use-system-titlebar") ? 0 : 72)
                    ZZIconButton("Show sidebar", systemName: "sidebar.left") { sidebarVisible = true }
                }
                if client.settings.bool("status-show-session") {
                    ZZWorkspaceStatusItem(client.attachedSession?.name ?? "Disconnected", icon: "square.3.layers.3d")
                }
            }
        } windows: {
            ForEach(client.attachedSession?.windows ?? []) { window in
                ZZWorkspaceStatusWindow(
                    window.name, index: String(window.index), active: window.current, connected: client.connected,
                    activity: client.settings.bool("status-badges")
                        && window.panes.contains { client.agents[$0.id]?.snapshot?.phase == "running" },
                    agent: client.settings.bool("status-agents") && window.panes.contains { $0.agent != nil },
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
            if client.settings.bool("show-fps") {
                Text("\(client.framesPerSecond) frames/s").font(theme.font(size: 10)).monospacedDigit()
            }
            if client.settings.bool("status-host") {
                Text(client.endpoint.hasPrefix("ssh://") ? client.endpoint : ProcessInfo.processInfo.hostName)
                    .font(theme.font(size: 11)).lineLimit(1).frame(maxWidth: 100)
            }
            if client.settings.text("status-clock") != "off" {
                TimelineView(.periodic(from: .now, by: 1)) { context in
                    Text(clockText(context.date)).font(theme.font(size: 11)).monospacedDigit()
                }
            }
            if client.settings.bool("status-update"), client.settings.updates.result?.state == "available" {
                ZZIconButton("Update available", systemName: "arrow.down.circle") {
                    client.settings.section = "about"
                    client.settings.visible = true
                }
            }
            paneActions
            ZZIconButton("New window", systemName: "plus") { client.execute("new-window") }.disabled(!client.connected)
        }
    }

    private func clockText(_ date: Date) -> String {
        let formatter = DateFormatter()
        switch client.settings.text("status-clock") {
        case "12-hour": formatter.dateFormat = "h:mm a"
        case "time-date": formatter.dateFormat = "MMM d HH:mm"
        default: formatter.dateFormat = "HH:mm"
        }
        return formatter.string(from: date)
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

private struct NativeInterfaceZoom: ViewModifier {
    let scale: Double
    func body(content: Content) -> some View {
        if scale == 1 {
            content
        } else {
            GeometryReader { geometry in
                content.frame(width: geometry.size.width / scale, height: geometry.size.height / scale)
                    .scaleEffect(scale, anchor: .topLeading)
            }
        }
    }
}
