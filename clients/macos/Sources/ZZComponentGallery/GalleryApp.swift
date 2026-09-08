import AppKit
import SwiftUI
import ZZUI

@main
struct ZZComponentGalleryApp: App {
    @NSApplicationDelegateAdaptor(GalleryAppDelegate.self) private var appDelegate
    @Environment(\.openWindow) private var openWindow

    var body: some Scene {
        WindowGroup("zz Native Preview") {
            DesktopPreview().frame(minWidth: 900, minHeight: 600)
        }
        .defaultSize(width: 1380, height: 950)
        .windowResizability(.contentMinSize)
        .windowStyle(.hiddenTitleBar)
        .commands {
            CommandGroup(after: .windowArrangement) {
                Button("Component Catalog") { openWindow(id: "catalog") }
                    .keyboardShortcut("k", modifiers: [.command, .option])
            }
            CommandGroup(replacing: .appInfo) {
                Button("About zz Native Gallery") {
                    NSApp.orderFrontStandardAboutPanel(options: [
                        .applicationName: "zz Native Gallery",
                        .applicationVersion: "Component library · SwiftUI + AppKit",
                        .credits: NSAttributedString(
                            string: "Native presentation for zz. The client core and daemon stay in Rust."),
                    ])
                }
            }
        }
        Window("Component Catalog", id: "catalog") {
            GalleryRoot().frame(minWidth: 900, minHeight: 600)
        }
        .defaultSize(width: 1220, height: 860)
        .windowResizability(.contentMinSize)
        .windowToolbarStyle(.unified)
    }
}

@MainActor
final class GalleryAppDelegate: NSObject, NSApplicationDelegate {
    func applicationWillFinishLaunching(_ notification: Notification) {
        NSWindow.allowsAutomaticWindowTabbing = false
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
    }
}

enum GalleryPage: String, CaseIterable, Identifiable {
    case foundations, buttons, badges, inputs, text, overlays
    case workspace, panes, browser, commands, settings, agent, feedback
    var id: Self { self }
    var title: String {
        switch self {
        case .foundations: "Foundations"
        case .buttons: "Buttons"
        case .badges: "Tags, keys & lists"
        case .inputs: "Inputs & selection"
        case .text: "Text & editor"
        case .overlays: "Menus & overlays"
        case .workspace: "Workspace & navigation"
        case .panes: "Panes & terminal chrome"
        case .browser: "Browser chrome"
        case .commands: "Commands & choosers"
        case .settings: "Settings"
        case .agent: "Agent"
        case .feedback: "Feedback & attachments"
        }
    }
    var symbol: String {
        switch self {
        case .foundations: "square.stack.3d.up"
        case .buttons: "cursorarrow.click"
        case .badges: "tag"
        case .inputs: "slider.horizontal.3"
        case .text: "text.alignleft"
        case .overlays: "macwindow.on.rectangle"
        case .workspace: "sidebar.left"
        case .panes: "rectangle.split.2x2"
        case .browser: "globe"
        case .commands: "command"
        case .settings: "gearshape"
        case .agent: "sparkles"
        case .feedback: "bubble.left"
        }
    }
    var detail: String {
        switch self {
        case .foundations: "The zz palette, typography, surfaces, and sizing in native Swift."
        case .buttons: "The complete button family, from compact toolbar actions to primary controls."
        case .badges: "Small components for status, shortcuts, selection, and scrolling."
        case .inputs: "Native editing and selection, dressed in zz's control surfaces."
        case .text: "Selectable Markdown and an AppKit editor with native text services."
        case .overlays: "Menus, popovers, dialogs, and notifications with native window behavior."
        case .workspace: "Sidebar rows, window surfaces, navigation, and status bars."
        case .panes: "Pane frames, dividers, overlays, and terminal controls."
        case .browser: "Tabs, address fields, suggestions, and browser state presentation."
        case .commands: "Searchable command and chooser surfaces with keyboard navigation."
        case .settings: "Compact forms, grouped settings, provenance, and appearance controls."
        case .agent: "Conversation, tools, permissions, attachments, and composition."
        case .feedback: "Connection prompts, confirmations, and attachment previews."
        }
    }
    var composition: CompositionPage? {
        switch self {
        case .workspace: .workspace
        case .panes: .panes
        case .browser: .browser
        case .commands: .commands
        case .settings: .settings
        case .agent: .agent
        case .feedback: .feedback
        default: nil
        }
    }
}

enum GalleryAppearance: String, CaseIterable, Identifiable {
    case system = "System"
    case light = "Light"
    case dark = "Dark"
    var id: Self { self }
    var colorScheme: ColorScheme? {
        switch self {
        case .system: nil
        case .light: .light
        case .dark: .dark
        }
    }
}

struct GalleryRoot: View {
    @Environment(\.colorScheme) private var colorScheme
    @AppStorage("gallery.appearance") private var appearance: GalleryAppearance = .system
    @AppStorage("gallery.radius") private var radius = 6.0
    @AppStorage("gallery.fontFamily") private var fontFamily = "System"
    @AppStorage("gallery.shadows") private var shadows = true
    @State private var page: GalleryPage? = .foundations
    @State private var search = ""
    @State private var showInspector = false
    @State private var disableControls = false
    @State private var toasts = ZZToastCenter()

    private var theme: ZZTheme {
        let scheme = appearance.colorScheme ?? colorScheme
        var theme: ZZTheme = scheme == .dark ? .macOSClassicDark : .macOSClassicLight
        theme.radius = radius
        theme.fontFamily = fontFamily == "System" ? nil : fontFamily
        theme.shadows = shadows
        return theme
    }

    private var visiblePages: [GalleryPage] {
        guard !search.isEmpty else { return GalleryPage.allCases }
        return GalleryPage.allCases.filter {
            $0.title.localizedStandardContains(search) || $0.detail.localizedStandardContains(search)
        }
    }

    var body: some View {
        ZZThemeContainer(theme: theme) {
            NavigationSplitView {
                sidebar
                    .navigationSplitViewColumnWidth(min: 210, ideal: 226, max: 280)
            } detail: {
                detail
                    .inspector(isPresented: $showInspector) { inspector.inspectorColumnWidth(240) }
            }
            .navigationTitle((page ?? .foundations).title)
            .toolbar {
                ToolbarItem(placement: .automatic) {
                    Picker("Appearance", selection: $appearance) {
                        ForEach(GalleryAppearance.allCases) { Text($0.rawValue).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    .frame(width: 186)
                    .help("Preview system, light, or dark appearance")
                }
                ToolbarItem(placement: .automatic) {
                    Button("Inspect theme", systemImage: "slider.horizontal.3") { showInspector.toggle() }
                        .help("Inspect theme and control states")
                }
            }
            .zzToastOverlay(toasts)
        }
        .preferredColorScheme(appearance.colorScheme)
        .onAppear {
            let args = ProcessInfo.processInfo.arguments
            if let index = args.firstIndex(of: "--page"), args.indices.contains(index + 1),
                let selected = GalleryPage(rawValue: args[index + 1])
            {
                page = selected
            }
            if let index = args.firstIndex(of: "--appearance"), args.indices.contains(index + 1),
                let selected = GalleryAppearance.allCases.first(where: { $0.rawValue.lowercased() == args[index + 1] })
            {
                appearance = selected
            }
        }
    }

    private var sidebar: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Text("zz").font(.system(size: 27, weight: .bold, design: .rounded))
                    .frame(width: 42, height: 42)
                    .background(theme.foreground.color, in: ZZRoundedRectangle(radius: 12))
                    .foregroundStyle(theme.foreground.on().color)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Native gallery").font(.system(size: 13, weight: .semibold))
                    Text("SwiftUI + AppKit").font(.system(size: 11)).foregroundStyle(theme.foreground.muted().color)
                }
                Spacer(minLength: 0)
            }
            .padding(16)
            List(selection: $page) {
                Section("Components") {
                    ForEach(visiblePages.filter { $0.composition == nil }) { item in
                        Label(item.title, systemImage: item.symbol).foregroundStyle(theme.foreground.color)
                            .listItemTint(theme.foreground.color).tag(item)
                    }
                }
                Section("Workspace") {
                    ForEach(visiblePages.filter { $0.composition != nil }) { item in
                        Label(item.title, systemImage: item.symbol).foregroundStyle(theme.foreground.color)
                            .listItemTint(theme.foreground.color).tag(item)
                    }
                }
            }
            .listStyle(.sidebar)
            .searchable(text: $search, placement: .sidebar, prompt: "Find a component")
            HStack(spacing: 6) {
                Image(systemName: "square.on.square")
                Text("zz-ui · native macOS")
                Spacer()
            }
            .font(.system(size: 10))
            .foregroundStyle(theme.foreground.muted().color)
            .padding(14)
        }
        .navigationTitle("Components")
    }

    private var detail: some View {
        let selected = page ?? .foundations
        return ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                VStack(alignment: .leading, spacing: 7) {
                    Text(selected.title).font(.system(size: 27, weight: .semibold))
                    Text(selected.detail).font(.system(size: 13)).foregroundStyle(theme.foreground.muted().color)
                }
                .padding(.bottom, 2)
                pageContent(selected).disabled(disableControls)
            }
            .frame(maxWidth: 1040, alignment: .leading)
            .padding(28)
            .frame(maxWidth: .infinity, alignment: .top)
        }
        .background(theme.background.raised(1).color)
        .id(selected)
    }

    @ViewBuilder
    private func pageContent(_ page: GalleryPage) -> some View {
        switch page {
        case .foundations: FoundationGallery()
        case .buttons: ButtonGallery()
        case .badges: PrimitiveGallery()
        case .inputs: InputGallery()
        case .text: TextGallery()
        case .overlays: OverlayGallery(toasts: toasts)
        default:
            if let composition = page.composition { CompositionGallery(page: composition) }
        }
    }

    private var inspector: some View {
        VStack(alignment: .leading, spacing: 20) {
            Text("Preview settings").font(.system(size: 14, weight: .semibold))
            Picker("UI font", selection: $fontFamily) {
                Text("System").tag("System")
                Text("Helvetica Neue").tag("Helvetica Neue")
                Text("Menlo").tag("Menlo")
            }
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    Text("Corner radius")
                    Spacer()
                    Text(radius, format: .number.precision(.fractionLength(0))).monospacedDigit()
                }
                Slider(value: $radius, in: 0...32, step: 1).accessibilityLabel("Corner radius")
                Text("The same adaptive curve as GPUI. Switches and pills keep their shape.")
                    .font(.system(size: 11)).foregroundStyle(theme.foreground.muted().color)
            }
            Toggle("Disable controls", isOn: $disableControls)
            Toggle("Control shadows", isOn: $shadows)
            ZZButton("Reset preview", icon: "arrow.counterclockwise", flat: true) {
                radius = 6
                disableControls = false
                appearance = .system
                fontFamily = "System"
                shadows = true
            }
            ZZSeparator()
            Text("Presentation only").font(.system(size: 12, weight: .semibold))
            Text(
                "These controls use local fixtures. The full client will connect through zz-client-ffi and keep session state, transport, and commands in Rust."
            )
            .font(.system(size: 12)).foregroundStyle(theme.foreground.muted().color)
            Spacer()
        }
        .padding(18)
    }
}
