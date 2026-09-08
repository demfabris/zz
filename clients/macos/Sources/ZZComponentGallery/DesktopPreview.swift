import AppKit
import SwiftUI
import ZZUI

struct DesktopPreview: View {
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.openWindow) private var openWindow
    @AppStorage("desktop.appearance") private var appearance: GalleryAppearance = .dark
    @State private var sidebarVisible = true
    @State private var hostExpanded = true
    @State private var sessionExpanded = true
    @State private var windowExpanded = true
    @State private var selectedPane = "browser"
    @State private var horizontalFraction: CGFloat = 0.5
    @State private var verticalFraction: CGFloat = 0.5
    @State private var appearanceVisible = false
    @State private var referenceStyle = true
    @State private var addHostVisible = false
    @State private var hostName = ""
    @State private var additionalHosts: [String] = []
    @State private var address = ""
    @State private var openedAddress = ""
    @State private var picking = false
    @State private var message = ""
    @State private var messages: [String] = []
    @State private var followsTail = true
    @State private var permission = "Manual"
    @State private var model = "Fable"
    @State private var effort = "XHigh"
    @State private var toasts = ZZToastCenter()

    private var theme: ZZTheme {
        let dark = (appearance.colorScheme ?? colorScheme) == .dark
        var theme: ZZTheme = dark ? .macOSClassicDark : .macOSClassicLight
        if referenceStyle && dark { theme.foreground = ZZColor(red: 1, green: 1, blue: 1) }
        theme.radius = referenceStyle ? 24 : 6
        theme.monospacedFontFamily = "Berkeley Mono Nerd Font"
        return theme
    }

    private var paneRadius: CGFloat { referenceStyle ? 24 : 13.5 }

    var body: some View {
        ZZThemeContainer(theme: theme) {
            HStack(spacing: 0) {
                if sidebarVisible { sidebar.frame(width: 256) }
                VStack(spacing: 0) {
                    if !sidebarVisible {
                        HStack {
                            ZZIconButton("Show sidebar", systemName: "sidebar.left") { sidebarVisible = true }
                                .padding(.leading, 76)
                            Spacer()
                        }.frame(height: 35)
                    }
                    ZZWorkspaceSplit(axis: .vertical, fraction: $verticalFraction) {
                        ZZWorkspaceSplit(axis: .horizontal, fraction: $horizontalFraction) {
                            pane("terminal") { terminal }
                        } second: {
                            pane("browser") { browser }
                        }
                    } second: {
                        pane("agent") { agent }
                    }
                }.padding(6)
            }
            .background { ZZWindowBackground(tint: theme.background.opacity(0.93)) }
            .zzToastOverlay(toasts)
            .sheet(isPresented: $addHostVisible) {
                ZZDialog("Add host", description: "Add a host to this local preview.") {
                    ZZTextField("Host", text: $hostName, placeholder: "user@hostname")
                } actions: {
                    ZZButton("Cancel") { addHostVisible = false }.keyboardShortcut(.cancelAction)
                    ZZButton("Add", variant: .primary) {
                        additionalHosts.append(hostName)
                        hostName = ""
                        addHostVisible = false
                    }.disabled(hostName.trimmingCharacters(in: .whitespaces).isEmpty)
                        .keyboardShortcut(.defaultAction)
                }
            }
        }
        .preferredColorScheme(appearance.colorScheme)
        .ignoresSafeArea()
        .onAppear {
            let args = ProcessInfo.processInfo.arguments
            if args.contains("--page") { openWindow(id: "catalog") }
            if let index = args.firstIndex(of: "--appearance"), args.indices.contains(index + 1),
                let selected = GalleryAppearance.allCases.first(where: { $0.rawValue.lowercased() == args[index + 1] })
            {
                appearance = selected
            }
        }
    }

    private var sidebar: some View {
        ZZWorkspaceSidebar(transparent: true) {
            HStack(spacing: 4) {
                Color.clear.frame(width: 72)
                ZZIconButton("Appearance", systemName: "gearshape") { appearanceVisible.toggle() }
                    .popover(isPresented: $appearanceVisible) { appearancePanel }
                ZZIconButton("Hide sidebar", systemName: "sidebar.left") { sidebarVisible = false }
                Spacer(minLength: 0)
            }
        } content: {
            VStack(spacing: 0) {
                ZZWorkspaceTreeRow("macbook", icon: "desktopcomputer", expanded: $hostExpanded, action: {})
                if hostExpanded {
                    ZZWorkspaceTreeRow(
                        "0", icon: "square.3.layers.3d", depth: 1, expanded: $sessionExpanded, action: {})
                    if sessionExpanded {
                        ZZWorkspaceTreeRow(
                            "bash", icon: "rectangle.split.2x1", depth: 2, selected: true,
                            expanded: $windowExpanded, action: {}
                        ) {
                            ZZIconButton("Reset split", systemName: "rectangle.split.2x1") {
                                horizontalFraction = 0.5
                                verticalFraction = 0.5
                            }
                        }
                        if windowExpanded {
                            ZZWorkspaceTreeRow(
                                "bash", icon: "terminal", depth: 3,
                                emphasized: selectedPane == "terminal"
                            ) { selectedPane = "terminal" }
                            ZZWorkspaceTreeRow(
                                openedAddress.isEmpty ? "about:blank" : openedAddress,
                                icon: "globe", depth: 3, emphasized: selectedPane == "browser"
                            ) {
                                selectedPane = "browser"
                            }
                            ZZWorkspaceTreeRow(
                                "agent", icon: "cpu", depth: 3,
                                emphasized: selectedPane == "agent"
                            ) { selectedPane = "agent" }
                        }
                    }
                }
            }
            .background {
                if hostExpanded {
                    ZZWorkspaceIndentGuides([
                        .init("host", level: 0, startRow: 1, rowCount: sessionExpanded ? (windowExpanded ? 5 : 2) : 1),
                        .init(
                            "session", level: 1, startRow: 2, rowCount: sessionExpanded ? (windowExpanded ? 4 : 1) : 0),
                        .init(
                            "window", level: 2, startRow: 3, rowCount: sessionExpanded && windowExpanded ? 3 : 0,
                            active: true),
                    ])
                }
            }
            ForEach(additionalHosts, id: \.self) { host in
                ZZWorkspaceTreeRow(host, icon: "network", connected: false, action: {})
            }
            ZZWorkspaceActionRow("Add host") { addHostVisible = true }
        }
    }

    private var appearancePanel: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Appearance").font(theme.font(size: 13, weight: .semibold))
            Picker("Appearance", selection: $appearance) {
                ForEach(GalleryAppearance.allCases) { Text($0.rawValue).tag($0) }
            }.pickerStyle(.segmented).labelsHidden()
            Toggle("Screenshot settings", isOn: $referenceStyle)
            Text(
                "macOS Classic · \(referenceStyle ? "24pt corners, white foreground" : "default corners and foreground")"
            )
            .font(theme.font(size: 11)).foregroundStyle(theme.foreground.muted().color)
            ZZSeparator()
            ZZButton("Component catalog", icon: "square.grid.2x2", flat: true) {
                appearanceVisible = false
                openWindow(id: "catalog")
            }
            Text("Local preview · session and transport integration comes next.")
                .font(theme.font(size: 11)).foregroundStyle(theme.foreground.muted().color)
        }.padding(16).frame(width: 300)
    }

    private func pane<Content: View>(_ id: String, @ViewBuilder content: () -> Content) -> some View {
        ZZPane(active: selectedPane == id, gap: 0, corners: ZZPaneCorners(paneRadius), content: content)
            .simultaneousGesture(TapGesture().onEnded { selectedPane = id })
    }

    private var terminal: some View {
        HStack(spacing: 0) {
            Text("demfabris").foregroundStyle(theme.success.color)
            Text("@").foregroundStyle(theme.foreground.muted().color)
            Text("macbook").foregroundStyle(theme.danger.color)
            Text(" ~ ").foregroundStyle(theme.warning.color)
            Text("$ ").foregroundStyle(theme.foreground.color)
            Rectangle().strokeBorder(theme.foreground.muted().color, lineWidth: 0.5).frame(width: 8, height: 16)
        }
        .font(Font(theme.nsFont(size: 13, weight: .semibold, monospaced: true)))
        .padding(4).frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Terminal preview, demfabris at macbook, shell prompt")
    }

    private var browser: some View {
        VStack(spacing: 0) {
            ZZBrowserToolbar(
                canGoBack: !openedAddress.isEmpty,
                back: {
                    openedAddress = ""; address = ""
                },
                forward: {}, reload: { toasts.show("Browser preview refreshed", title: "Reload") }
            ) {
                ZZBrowserAddress($address) { navigate(address) }
            } actions: {
                ZZIconButton("New tab", systemName: "plus") {
                    openedAddress = ""; address = ""
                }
                ZZIconButton("Pick element", systemName: "cursorarrow.rays") { picking.toggle() }
                ZZBrowserActionMenu {
                    Button("Copy address") {
                        NSPasteboard.general.clearContents(); NSPasteboard.general.setString(address, forType: .string)
                    }
                    Button("Component catalog") { openWindow(id: "catalog") }
                }
            }
            ZStack(alignment: .top) {
                ZZBrowserStart(showsHint: false) {
                    ForEach(Self.recentAddresses, id: \.self) { url in
                        ZZBrowserRecentRow(url) { navigate(url) }
                    }
                }
                if picking {
                    ZZBrowserPickStatus("Choose an element to send to your agent") { picking = false }.padding(8)
                }
            }.frame(maxHeight: .infinity)
        }
    }

    private var agent: some View {
        VStack(spacing: 0) {
            ZZAgentHeader {
                ZZMenu("Claude Code", icon: "asterisk", flat: true) {
                    Button("Claude Code") {}
                    Button("Component catalog") { openWindow(id: "catalog") }
                }
            } trailing: {
                HStack(spacing: 6) {
                    ZZIconButton("New conversation", systemName: "bubble.left.and.bubble.right") {
                        messages = []; message = ""
                    }
                    ZZIconButton("Conversation history", systemName: "clock.arrow.circlepath") {
                        toasts.show("No previous conversations in this preview", title: "Conversation history")
                    }
                }
            }
            if messages.isEmpty {
                ZZAgentEmptyState("Ask the agent to work in this workspace")
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
            } else {
                ZZAgentTimeline(revision: messages.count, followsTail: $followsTail) {
                    ForEach(Array(messages.enumerated()), id: \.offset) { _, text in ZZAgentUserMessage(text) }
                }
            }
            ZZAgentComposer(
                text: $message, canSend: !message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
                send: {
                    messages.append(message); message = ""
                }, stop: {}
            ) {
                ZZMenu(permission, icon: "checkmark", flat: true) {
                    ForEach(["Manual", "Auto"], id: \.self) { value in Button(value) { permission = value } }
                }
                ZZMenu(model, icon: "asterisk", flat: true) {
                    ForEach(["Fable", "Opus", "Sonnet"], id: \.self) { value in Button(value) { model = value } }
                }
                ZZMenu(effort, icon: "cpu", flat: true) {
                    ForEach(["Low", "Medium", "High", "XHigh"], id: \.self) { value in Button(value) { effort = value }
                    }
                }
            } attachments: {
                EmptyView()
            } footer: {
                HStack {
                    Spacer()
                    Label("demfabris", systemImage: "folder").font(theme.font(size: 12, weight: .medium))
                }
            }
        }
    }

    private func navigate(_ url: String) {
        address = url
        openedAddress = url
        selectedPane = "browser"
        toasts.show(url, title: "Address selected")
    }

    private static let recentAddresses = [
        "http://localhost:5173/a/", "http://localhost:5173/", "https://dribbble.com/search/landing-page",
        "https://dribbble.com/session/new", "https://accounts.google.com/gsi/transform",
        "https://accounts.google.com/signin/oauth/v3/consent", "https://accounts.google.com/v3/signin/accountchooser",
        "https://accounts.google.com/gsi/select",
    ]
}
