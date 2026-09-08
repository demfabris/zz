import AppKit
import SwiftUI
import ZZUI

enum CompositionPage: String, CaseIterable, Identifiable {
    case workspace, panes, browser, commands, settings, agent, feedback
    var id: Self { self }
}

struct CompositionGallery: View {
    var page: CompositionPage = .workspace

    var body: some View {
        switch page {
        case .workspace: WorkspaceCompositionGallery()
        case .panes: PaneCompositionGallery()
        case .browser: BrowserCompositionGallery()
        case .commands: CommandCompositionGallery()
        case .settings: SettingsCompositionGallery()
        case .agent: AgentCompositionGallery()
        case .feedback: FeedbackCompositionGallery()
        }
    }
}

private struct WorkspaceCompositionGallery: View {
    @Environment(\.zzTheme) private var theme
    @State private var sidebar = true
    @State private var hostExpanded = true
    @State private var sessionExpanded = true
    @State private var selected = "terminal"
    @State private var window = "0"
    @State private var host = ""
    @State private var addedHost: String?
    @State private var addHost = false
    @State private var settings = false
    @State private var status = "Connected to local daemon"
    @State private var workspaceFocused = true
    @State private var persistentActions = false

    var body: some View {
        VStack(spacing: 20) {
            NavigationPresentationGallery()
            GallerySection(
                "Workspace", detail: "Expand the tree, switch windows, resize the sidebar, or add a local fixture host."
            ) {
                ZZAppShell(sidebarVisible: $sidebar) {
                    ZZWorkspaceSidebar {
                        HStack(spacing: 4) {
                            ZZIconButton("Settings", systemName: "gearshape") { settings = true }
                            ZZIconButton("Toggle sidebar", systemName: "sidebar.left") { sidebar.toggle() }
                            Spacer()
                            Text("Workspace").font(.system(size: 11)).foregroundStyle(theme.foreground.muted().color)
                        }
                    } content: {
                        VStack(spacing: 0) {
                            ZZWorkspaceTreeRow("This Mac", icon: "laptopcomputer", expanded: $hostExpanded) {
                                status = "This Mac · connected"
                            }
                            if hostExpanded {
                                ZZWorkspaceTreeRow("zz", icon: "square.stack", depth: 1, expanded: $sessionExpanded) {
                                    status = "Session zz"
                                }
                                if sessionExpanded {
                                    ZZWorkspaceTreeRow(
                                        "Terminal", icon: "terminal", depth: 2, active: selected == "terminal"
                                    ) {
                                        selected = "terminal"
                                    } actions: {
                                        ZZButton(
                                            "Split terminal", icon: "rectangle.split.2x1", variant: .text,
                                            size: .xSmall, flat: true, iconOnly: true
                                        ) { status = "Split terminal requested" }
                                    }
                                    ZZWorkspaceTreeRow(
                                        "Native client", icon: "sparkles", depth: 2, active: selected == "agent"
                                    ) { selected = "agent" }
                                    ZZWorkspaceTreeRow(
                                        "Documentation", icon: "globe", depth: 2, active: selected == "browser"
                                    ) { selected = "browser" }
                                }
                                ZZWorkspaceTreeRow(
                                    "scratch", icon: "rectangle", depth: 1, selected: selected == "scratch"
                                ) { selected = "scratch" }
                            }
                            ZZWorkspaceTreeRow("archbox", icon: "server.rack", connected: false) {
                                status = "archbox · disconnected"
                            }
                            if let addedHost {
                                ZZWorkspaceTreeRow(addedHost, icon: "server.rack", selected: selected == "added") {
                                    selected = "added"
                                    status = "\(addedHost) · fixture host"
                                }
                            }
                            ZZWorkspaceActionRow("Add host") { addHost = true }
                        }
                        .background(alignment: .topLeading) {
                            if hostExpanded && sessionExpanded {
                                ZZWorkspaceIndentGuides([
                                    .init("host", level: 0, startRow: 1, rowCount: 5),
                                    .init("session", level: 1, startRow: 2, rowCount: 3, active: true),
                                ])
                            }
                        }
                    }
                } content: {
                    ZZPane(active: true) {
                        VStack(alignment: .leading, spacing: 12) {
                            HStack {
                                if !sidebar {
                                    ZZIconButton("Show sidebar", systemName: "sidebar.left") { sidebar = true }
                                }
                                Text(selected == "terminal" ? "~/dev/zz" : selected.capitalized).font(.system(size: 12))
                                    .foregroundStyle(theme.foreground.muted().color)
                                Spacer()
                                ZZTag("%0")
                            }
                            Text("❯ swift build\nBuilding for debugging…\nBuild complete!\n\n❯ ")
                                .font(.system(size: 13, design: .monospaced)).lineSpacing(6).textSelection(.enabled)
                            Spacer()
                            Text(status).font(.system(size: 11)).foregroundStyle(theme.foreground.muted().color)
                        }.padding(16)
                    }
                } status: {
                    ZZWorkspaceStatusBar {
                        ZZWorkspaceStatusItem("zz", icon: "square.stack")
                    } windows: {
                        ZZWorkspaceStatusWindow("dev", index: "0", active: window == "0") { window = "0" }
                        ZZWorkspaceStatusWindow("agent", index: "1", active: window == "1", activity: true, agent: true)
                        { window = "1" }
                        ZZWorkspaceStatusWindow("logs", index: "2", active: window == "2", bell: true) { window = "2" }
                    } trailing: {
                        ZZWorkspaceStatusItem("main", icon: "arrow.triangle.branch")
                    }
                }.frame(height: 420).clipShape(ZZRoundedRectangle(radius: theme.radius))
            }
            GallerySection("Connection states") {
                HStack {
                    ZZConnectionState("Connecting to archbox…", connecting: true).frame(height: 110)
                    ZZConnectionState("Connection closed") {
                        ZZButton("Reconnect", icon: "arrow.clockwise") { status = "Reconnect requested" }
                    }.frame(height: 110)
                }
            }
            GallerySection("Workspace row states") {
                HStack(spacing: 20) {
                    ZZSwitch("Workspace focused", isOn: $workspaceFocused, size: .small)
                    ZZSwitch("Always show row actions", isOn: $persistentActions, size: .small)
                }
                VStack(spacing: 0) {
                    ZZWorkspaceTreeRow(
                        "Active pane", icon: "terminal", active: true, focused: workspaceFocused,
                        hoverActions: !persistentActions, action: { status = "Active pane selected" }
                    ) {
                        ZZButton(
                            "Close active fixture pane", icon: "xmark", variant: .text, size: .xSmall,
                            flat: true, iconOnly: true
                        ) { status = "Close requested" }
                    }
                    ZZWorkspaceTreeRow(
                        "Keyboard selection", icon: "sparkles", selected: true, focused: workspaceFocused
                    ) { status = "Keyboard selection activated" }
                    ZZWorkspaceTreeRow("Disconnected host", icon: "server.rack", connected: false) {
                        status = "Disconnected host selected"
                    }
                }.frame(maxWidth: 360)
            }
        }
        .sheet(isPresented: $addHost) {
            ZZAddHostPrompt(
                host: $host,
                submit: {
                    addedHost = host.trimmingCharacters(in: .whitespacesAndNewlines)
                    status = "Added \(addedHost ?? "host") to the fixture"
                    addHost = false
                }, cancel: { addHost = false })
        }
        .sheet(isPresented: $settings) {
            ZZDialog("Workspace settings", description: "These settings affect this gallery fixture.") {
                ZZSwitch("Show sidebar", isOn: $sidebar)
            } actions: {
                ZZButton("Done") { settings = false }
            }
        }
    }
}

private struct PaneCompositionGallery: View {
    @Environment(\.zzTheme) private var theme
    @State private var axis: ZZPaneSplitAxis = .horizontal
    @State private var gaps = true
    @State private var dimInactive = true
    @State private var active = 0
    @State private var identifiers = false
    @State private var query = ""
    @State private var searching = true
    @State private var match = 1
    @State private var dragState: ZZPaneDragState?
    @State private var borderWidth = "0.5"
    @State private var outerCorners = false
    @State private var shadows = true
    @State private var paneKind = "Terminal"

    var body: some View {
        VStack(spacing: 20) {
            GallerySection("New pane") {
                ZZPanePicker {
                    ZZPanePickerRow("Terminal", icon: "terminal", shortcut: "t", selected: paneKind == "Terminal") {
                        paneKind = "Terminal"
                    }
                    ZZPanePickerRow("Browser", icon: "globe", shortcut: "b", selected: paneKind == "Browser") {
                        paneKind = "Browser"
                    }
                    ZZPanePickerRow("Agent", icon: "cpu", shortcut: "a", selected: paneKind == "Agent") {
                        paneKind = "Agent"
                    }
                }.frame(height: 168)
            }
            GallerySection(
                "Pane composition",
                detail: "Drag the native divider. Pane focus, dimming, gaps, and overlays update together."
            ) {
                VStack(alignment: .leading, spacing: 14) {
                    HStack(spacing: 12) {
                        Picker("Split", selection: $axis) {
                            Text("Horizontal").tag(ZZPaneSplitAxis.horizontal)
                            Text("Vertical").tag(ZZPaneSplitAxis.vertical)
                        }.pickerStyle(.segmented).frame(width: 180)
                        ZZSwitch("Gaps", isOn: $gaps, size: .small)
                        ZZSwitch("Dim inactive", isOn: $dimInactive, size: .small)
                        Spacer()
                        ZZButton("Identify", selected: identifiers) { identifiers.toggle() }
                    }
                    HStack(spacing: 16) {
                        Text("Border").font(.system(size: 12))
                        ZZNumberInput("Pane border width", text: $borderWidth, step: 0.5, minimum: 0, maximum: 4).frame(
                            width: 90)
                        ZZSwitch("Outer corners only", isOn: $outerCorners, size: .small)
                        ZZSwitch("Pane shadows", isOn: $shadows, size: .small)
                    }
                    ZZPaneSplit(axis) {
                        samplePane(0)
                    } second: {
                        samplePane(1)
                    }.frame(height: 320)
                }
            }
            GallerySection("Drag and floating surfaces") {
                HStack(alignment: .top, spacing: 20) {
                    VStack(alignment: .leading, spacing: 12) {
                        ZZPaneDragChip("%2", title: "Server logs")
                        Picker("Drag state", selection: $dragState) {
                            Text("Off").tag(ZZPaneDragState?.none)
                            ForEach(ZZPaneDragState.allCases) { state in
                                Text(state.rawValue.capitalized).tag(Optional(state))
                            }
                        }.frame(width: 230)
                    }
                    ZZFloatingSurface("%3 · floating terminal") {
                        Text("❯ tail -f server.log\nListening on localhost:8080")
                            .font(.system(size: 12, design: .monospaced)).lineSpacing(6)
                    }
                }
            }
        }
    }

    private func samplePane(_ index: Int) -> some View {
        ZZPane(
            active: active == index, gap: gaps ? 8 : 0, inactiveOpacity: active == index || !dimInactive ? 1 : 0.5,
            borderWidth: Double(borderWidth) ?? 0.5, corners: corners(for: index), shadow: shadows
        ) {
            VStack(alignment: .leading, spacing: 10) {
                HStack {
                    ZZButton("Pane \(index)", icon: "terminal", variant: .ghost, flat: true) { active = index }
                    Spacer()
                    if !searching && index == 0 {
                        ZZIconButton("Find", systemName: "magnifyingglass") { searching = true }
                    }
                }
                Text(
                    index == 0
                        ? "❯ cargo test -p zz-client\n\nrunning 164 tests\ntest result: ok. 164 passed"
                        : "❯ git status\n\nOn branch codex/native-macos\n\n❯ "
                )
                .font(.system(size: 12, design: .monospaced)).lineSpacing(5).textSelection(.enabled)
                Spacer()
            }.padding(12)
        } overlay: {
            if index == 1, let dragState { ZZPaneDragOverlay(dragState) }
            ZZPaneOverlayStack {
                ZZFrameRateBadge("UI", fps: index == 0 ? 120 : nil)
                if index == 1 { ZZTag("SYNC", tone: .danger) }
            }
            if identifiers {
                ZZPaneIndicator("\(index)", key: "\(index)", active: active == index) {
                    active = index
                    identifiers = false
                }
            }
            if index == 0 {
                ZZPaneOverlayStack(alignment: .bottomTrailing) {
                    if searching {
                        ZZTerminalSearch(
                            query: $query, matches: "\(match)/8", previous: { match = max(1, match - 1) },
                            next: { match = min(8, match + 1) }, close: { searching = false })
                    } else {
                        ZZTerminalStatus("Copied 3 lines", label: "COPY")
                    }
                }
            }
        }
    }

    private func corners(for index: Int) -> ZZPaneCorners? {
        guard outerCorners else { return nil }
        if axis == .horizontal {
            return ZZPaneCorners(
                topLeft: index == 0 ? 20 : 0, topRight: index == 1 ? 20 : 0,
                bottomLeft: index == 0 ? 20 : 0, bottomRight: index == 1 ? 20 : 0)
        }
        return ZZPaneCorners(
            topLeft: index == 0 ? 20 : 0, topRight: index == 0 ? 20 : 0,
            bottomLeft: index == 1 ? 20 : 0, bottomRight: index == 1 ? 20 : 0)
    }
}
