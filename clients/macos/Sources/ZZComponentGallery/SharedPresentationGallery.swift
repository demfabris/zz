import SwiftUI
import ZZUI

struct AgentPresentationGallery: View {
    @Environment(\.zzTheme) private var theme
    @FocusState private var permissionFocused: Bool
    @FocusState private var suggestionsFocused: Bool
    @State private var busy = false
    @State private var selectedPermission = 0
    @State private var permissionResult = "Awaiting a choice"
    @State private var selectedSuggestion = 0
    @State private var suggestionResult = "Choose a provider command"

    private let options = ["Allow once", "Allow for this session", "Deny"]
    private let suggestions = [
        ("compact", "Compact the current conversation"), ("review", "Review the working changes"),
        ("help", "Show the provider's available commands"), ("model", "Choose a model"),
        ("context", "Show context usage"), ("cost", "Show session usage"),
        ("clear", "Start a fresh conversation"),
    ]

    var body: some View {
        GallerySection("Empty and error states") {
            ZZSwitch("Connecting", isOn: $busy)
            ZZAgentEmptyState(busy ? "Connecting to the agent…" : "Ask the agent to work in this workspace", busy: busy)
            ZZAgentError(
                "The agent process exited before the request completed.\nReconnect to continue the conversation.")
        }
        GallerySection("Permission choices", detail: "Focus the card, then use 1–3, ↑ ↓, Return, or Escape.") {
            ZZButton("Focus permission choices", flat: true) { permissionFocused = true }
            ZZAgentPermissionCard("Allow the agent to run this command?", counter: "1 of 2") {
                ForEach(options.indices, id: \.self) { index in
                    ZZAgentPermissionOption(index: index, highlighted: selectedPermission == index) {
                        ZZButton(options[index], variant: index == 2 ? .danger : .default, size: .small) {
                            selectedPermission = index
                            permissionResult = options[index]
                            permissionFocused = true
                        }
                    }
                }
            } cancel: {
                ZZButton("Cancel", variant: .ghost, size: .xSmall) { permissionResult = "Cancelled" }
            }
            .focusable().focused($permissionFocused).focusEffectDisabled()
            .onKeyPress(.upArrow) {
                selectedPermission = max(0, selectedPermission - 1); return .handled
            }
            .onKeyPress(.downArrow) {
                selectedPermission = min(2, selectedPermission + 1); return .handled
            }
            .onKeyPress(.return) {
                permissionResult = options[selectedPermission]; return .handled
            }
            .onKeyPress(.escape) {
                permissionResult = "Cancelled"; return .handled
            }
            .onKeyPress(characters: CharacterSet(charactersIn: "123")) { key in
                guard let number = Int(key.characters), (1...3).contains(number) else { return .ignored }
                selectedPermission = number - 1
                permissionResult = options[selectedPermission]
                return .handled
            }
            Text(permissionResult).font(theme.font(size: 11)).accessibilityLabel(
                "Permission result: \(permissionResult)")
        }
        GallerySection("Provider command suggestions", detail: "Six visible rows, with native scrolling for the rest.")
        {
            ZZButton("Focus suggestions", flat: true) { suggestionsFocused = true }
            ZZAgentSuggestionList(rowCount: suggestions.count, selection: String(selectedSuggestion)) {
                ForEach(suggestions.indices, id: \.self) { index in
                    ZZAgentSuggestionRow(
                        suggestions[index].0, description: suggestions[index].1,
                        selected: selectedSuggestion == index
                    ) {
                        selectedSuggestion = index
                        suggestionResult = "/" + suggestions[index].0
                    }.id(String(index))
                }
            }.focusable().focused($suggestionsFocused).focusEffectDisabled()
                .onKeyPress(.upArrow) {
                    selectedSuggestion = max(0, selectedSuggestion - 1); return .handled
                }
                .onKeyPress(.downArrow) {
                    selectedSuggestion = min(suggestions.count - 1, selectedSuggestion + 1); return .handled
                }
                .onKeyPress(.return) {
                    suggestionResult = "/" + suggestions[selectedSuggestion].0; return .handled
                }
            Text(suggestionResult).font(theme.font(size: 11))
        }
    }
}

struct NavigationPresentationGallery: View {
    @Environment(\.zzTheme) private var theme
    @State private var result = "Choose a workspace action"
    @State private var connecting = false
    @State private var activeWindow = "0"
    @State private var windows = ["0", "1", "2", "3", "4", "5", "6"]
    @State private var renaming: String?
    @State private var renamed = ""
    @State private var names: [String: String] = [:]

    var body: some View {
        GallerySection("Sidebar indicators and actions") {
            ZZSwitch("Connecting", isOn: $connecting)
            VStack(spacing: 0) {
                ZZWorkspaceTreeRow(
                    "archbox", icon: "server.rack", connected: false, bell: true,
                    badge: theme.success, connecting: connecting,
                    connectionDetail: "SSH connection was closed",
                    showConnectionDetail: {
                        result = "SSH connection was closed"
                    }, action: { result = "Selected archbox" }
                ) {
                    ZZWindowLayoutMenu {
                        result = $0 == .horizontal ? "Split right requested" : "Split bottom requested"
                    }
                }.zzRenameMenu("Rename host") { result = "Rename host requested" }
                ZZWorkspaceTreeRow(
                    "dev", icon: "macwindow", depth: 1, bell: true, badge: theme.warning,
                    action: { result = "Selected dev" }
                ) {
                    ZZWindowLayoutMenu {
                        result = $0 == .horizontal ? "Split right requested" : "Split bottom requested"
                    }
                }
            }.frame(maxWidth: 360)
            Text(result).font(theme.font(size: 11))
        }
        GallerySection(
            "Status windows",
            detail: "Hover to close; right-click to rename. The overflow menu contains all supplied windows."
        ) {
            ZZWorkspaceStatusBar {
                ZZStatusSession("zz") { result = "Session focused" }
            } windows: {
                ForEach(windows.prefix(3), id: \.self) { id in
                    ZZWorkspaceStatusWindow(
                        names[id] ?? (id == "0" ? "dev" : "Window \(id)"), index: id, active: activeWindow == id,
                        close: {
                            windows.removeAll { $0 == id }; result = "Closed window \(id)"
                        },
                        rename: {
                            renaming = id; renamed = "Window \(id)"
                        }
                    ) { activeWindow = id }
                }
                ZZStatusWindowOverflow(
                    windows.map { id in
                        ZZStatusWindowEntry(
                            id, label: "\(id): \(names[id] ?? "Window \(id)")", active: activeWindow == id
                        ) {
                            activeWindow = id
                            result = "Selected window \(id)"
                        }
                    })
            } trailing: {
                ZZStatusAgentCount(2)
                ZZStatusClock("18:42")
            }
            ZZButton("Reset windows", flat: true) {
                windows = ["0", "1", "2", "3", "4", "5", "6"]; activeWindow = "0"
            }
        }
        .sheet(isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
            ZZDialog("Rename window") {
                ZZTextField("Name", text: $renamed)
            } actions: {
                ZZButton("Cancel") { renaming = nil }.keyboardShortcut(.cancelAction)
                ZZButton("Rename", variant: .primary) {
                    if let renaming { names[renaming] = renamed }
                    result = "Renamed window \(renaming ?? "") to \(renamed)"
                    renaming = nil
                }.keyboardShortcut(.defaultAction)
            }
        }
    }
}

struct FloatingCommandGallery: View {
    @Environment(\.zzTheme) private var theme
    @FocusState private var menuFocused: Bool
    @FocusState private var confirmFocused: Bool
    @State private var selected = 0
    @State private var result = "Choose an action"
    @State private var confirming = false
    private let items = ["Split right", "Split bottom", "Rename window"]

    var body: some View {
        GallerySection(
            "Floating command menu", detail: "Presentation uses caller-supplied rows, colors, and cell height."
        ) {
            ZZButton("Focus menu", flat: true) { menuFocused = true }
            VStack(spacing: 0) {
                ForEach(items.indices, id: \.self) { index in
                    ZZFloatingMenuRow(items[index], annotation: ["h", "v", ","][index], selected: selected == index) {
                        selected = index
                        result = items[index]
                    }
                }
                ZZFloatingMenuSeparator()
                ZZFloatingMenuRow("Unavailable action", enabled: false) {}
            }.padding(.vertical, 4).frame(maxWidth: 420).zzSurface()
                .focusable().focused($menuFocused).focusEffectDisabled()
                .onKeyPress(.upArrow) {
                    selected = max(0, selected - 1); return .handled
                }
                .onKeyPress(.downArrow) {
                    selected = min(items.count - 1, selected + 1); return .handled
                }
                .onKeyPress(.return) {
                    result = items[selected]; return .handled
                }
            Text(result).font(theme.font(size: 11))
            ZZButton("Show confirmation") {
                confirming = true; confirmFocused = true
            }
            if confirming {
                ZZConfirmPrompt("Close this fixture window? (y/n)").frame(height: 36).zzSurface()
                    .focusable().focused($confirmFocused).focusEffectDisabled()
                    .onAppear { confirmFocused = true }
                    .onKeyPress(characters: CharacterSet(charactersIn: "yn")) { key in
                        result = key.characters == "y" ? "Close confirmed" : "Close cancelled"
                        confirming = false
                        return .handled
                    }
                    .onKeyPress(.escape) {
                        result = "Close cancelled"; confirming = false; return .handled
                    }
            }
        }
    }
}
