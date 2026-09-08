import SwiftUI
import ZZUI

struct SettingsCompositionGallery: View {
    @Environment(\.zzTheme) private var theme
    @State private var selection = "appearance"
    @State private var mode: ZZThemePreviewMode = .system
    @State private var font: String? = "system"
    @State private var zoom = "100"
    @State private var radius = "6"
    @State private var gaps = "8"
    @State private var animations = true
    @State private var blur = true
    @State private var enabled = true
    @State private var monospace = "SF Mono"
    @State private var color: ZZColor?
    @State private var result = "Changes apply to this fixture."

    private let items: [ZZSettingsNavigationItem] = [
        .init("appearance", title: "Interface", icon: "paintpalette", group: "Appearance"),
        .init("status", title: "Status bar", icon: "rectangle.bottomthird.inset.filled", group: "Appearance"),
        .init("editor", title: "Editor", icon: "doc.text", group: "Appearance"),
        .init("panes", title: "Panes", icon: "rectangle.split.2x2", group: "Appearance"),
        .init("mux", title: "Multiplexer", icon: "square.stack", group: "Tools"),
        .init("browser", title: "Browser", icon: "globe", group: "Tools"),
        .init("terminal", title: "Terminal", icon: "terminal", group: "Tools"),
        .init("hosts", title: "Hosts", icon: "server.rack", group: "Advanced"),
        .init("system", title: "System", icon: "cpu", group: "Advanced"),
        .init("about", title: "About", icon: "info.circle", group: "Advanced"),
    ]

    var body: some View {
        VStack(spacing: 20) {
            GallerySection(
                "Settings composition",
                detail:
                    "Navigate groups and edit the sample configuration. Resets and provenance stay next to each setting."
            ) {
                HSplitView {
                    ZZSettingsNavigation(selection: $selection, items: items)
                    ZZSettingsPage(
                        items.first(where: { $0.id == selection })?.title ?? "Settings",
                        description: description
                    ) {
                        pageContent
                    }.frame(minWidth: 390)
                }.frame(height: 580).zzSurface(elevation: 0)
            }
            GallerySection("Disabled and inherited values") {
                ZZSettingsStack("Overrides") {
                    ZZSettingEntry(
                        "Inherited setting", description: "This value comes from your theme.", provenance: "From theme",
                        enabled: false
                    ) {
                        ZZTextField("Inherited value", text: .constant("SF Mono")).frame(width: 150)
                    }
                    ZZSettingsDivider()
                    ZZSettingEntry(
                        "Override", description: "Reset restores the inherited value.", icon: "slider.horizontal.3",
                        provenance: monospace == "SF Mono" ? "Default" : "Overridden", reset: { monospace = "SF Mono" }
                    ) {
                        ZZTextField("Font override", text: $monospace).frame(width: 150)
                    }
                }
            }
        }
    }

    private var description: String {
        switch selection {
        case "appearance": "Customize the app theme, chrome colors, and visual details."
        case "panes": "Tune pane spacing, borders, corners, and shadows across the workspace."
        case "terminal": "Configure terminal typography and behavior."
        case "editor": "Set typography and editing behavior for editor panes."
        case "browser": "Configure browser controls and shortcuts."
        case "status": "Choose what appears when the sidebar is retracted."
        case "hosts": "Manage SSH machines in your fleet."
        case "mux": "Your tmux configuration stays in the Rust backend."
        case "system": "Daemon lifecycle, diagnostics, and experimental features."
        default: "Native macOS component gallery for zz."
        }
    }

    @ViewBuilder private var pageContent: some View {
        switch selection {
        case "appearance": appearanceContent
        case "panes": paneContent
        case "terminal", "editor": typographyContent
        case "about":
            ZZSettingsStack {
                ZZSettingEntry("zz native", description: "SwiftUI and AppKit presentation · Rust client core") {
                    ZZTag("Component gallery")
                }
                ZZSettingsDivider()
                ZZSettingEntry("Version", description: "macOS 14 and later") { Text("0.1").monospaced() }
            }
        default:
            ZZSettingsStack("\(items.first(where: { $0.id == selection })?.title ?? "Workspace") options") {
                ZZSettingEntry(
                    "Enabled", description: "Toggle the sample \(selection) feature.", reset: { enabled = true }
                ) {
                    ZZSwitch("Enabled", isOn: $enabled, size: .small, showsLabel: false)
                }
                ZZSettingsDivider()
                ZZSettingEntry("Configuration", description: result) {
                    ZZButton("Open", icon: "doc.text") { result = "Opened the \(selection) fixture configuration." }
                }
            }
        }
    }

    private var appearanceContent: some View {
        VStack(spacing: 20) {
            ZZSettingsStack("Theme") {
                ZZSettingEntry("Theme mode", description: "Follow the system or choose a fixed appearance.") {
                    EmptyView()
                } detail: {
                    HStack(spacing: 20) {
                        ForEach(ZZThemePreviewMode.allCases) { option in
                            ZZThemeTile(option, selected: mode == option) { mode = option }
                        }
                    }
                }
                ZZSettingsDivider()
                ZZSettingEntry(
                    "UI font", description: "Use the macOS system font or choose a family.",
                    provenance: font == "system" ? "Default" : "Overridden",
                    reset: { font = "system" }
                ) {
                    ZZSelect(
                        "UI font", selection: $font,
                        options: [
                            .init("system", title: "System default"), .init("mono", title: "SF Mono"),
                            .init("menlo", title: "Menlo"),
                        ]
                    )
                    .frame(width: 150)
                }
                ZZSettingsDivider()
                ZZSettingEntry("UI zoom", description: "Scale workspace controls.", reset: { zoom = "100" }) {
                    ZZNumberInput("UI zoom", text: $zoom, step: 10, minimum: 50, maximum: 300).frame(width: 100)
                }
            }
            ZZSettingsStack("Tweaks") {
                ZZSettingEntry("Animations", description: "Animate interface transitions.") {
                    ZZSwitch("Animations", isOn: $animations, size: .small, showsLabel: false)
                }
                ZZSettingsDivider()
                ZZSettingEntry("Background blur", description: "Use native macOS material in the chrome.") {
                    ZZSwitch("Background blur", isOn: $blur, size: .small, showsLabel: false)
                }
                ZZSettingsDivider()
                ZZSettingEntry(
                    "Foreground", description: "Controls the chrome text and selection color.",
                    provenance: color == nil ? "Default" : "Overridden", reset: { color = nil }
                ) {
                    ZZColorPicker("Foreground", selection: $color, inherited: theme.foreground)
                }
            }
        }
    }

    private var paneContent: some View {
        ZZSettingsStack("Pane layout") {
            ZZSettingEntry("Gap", description: "Space between adjacent panes.", reset: { gaps = "8" }) {
                ZZNumberInput("Pane gap", text: $gaps, minimum: 0, maximum: 32).frame(width: 100)
            }
            ZZSettingsDivider()
            ZZSettingEntry(
                "Corner radius", description: "Adaptive corners preserve each control’s shape.", reset: { radius = "6" }
            ) {
                ZZNumberInput("Pane radius", text: $radius, minimum: 0, maximum: 32).frame(width: 100)
            }
            ZZSettingsDivider()
            ZZSettingEntry("Inactive dimming", description: "Keep the active pane in focus.") {
                ZZSwitch("Inactive dimming", isOn: $enabled, size: .small, showsLabel: false)
            }
        }
    }

    private var typographyContent: some View {
        ZZSettingsStack("Typography") {
            ZZSettingEntry(
                "Font family", description: "Use a monospaced font for content.", reset: { monospace = "SF Mono" }
            ) {
                ZZTextField("Font family", text: $monospace).frame(width: 150)
            }
            ZZSettingsDivider()
            ZZSettingEntry("Smooth scrolling", description: "Use native scroll momentum.") {
                ZZSwitch("Smooth scrolling", isOn: $animations, size: .small, showsLabel: false)
            }
        }
    }
}
