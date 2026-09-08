import SwiftUI
import ZZUI

private struct PickerPathFixture: Identifiable {
    let id: String
    let icon: String
}

private struct PickerHistoryFixture: Identifiable {
    let id: String
    let title: String
    let directory: String
    let updatedAt: String
    var current = false
}

struct PickerGallery: View {
    @Environment(\.zzTheme) private var theme
    @State private var historyMode = false
    @State private var presented = true
    @State private var query = ""
    @State private var selected = "clients"
    @State private var allProjects = false
    @State private var result = "Choose a file, folder, or session."

    private let paths = [
        PickerPathFixture(id: "clients", icon: "folder"),
        PickerPathFixture(id: "crates", icon: "folder"),
        PickerPathFixture(id: "knowledge", icon: "folder"),
        PickerPathFixture(id: "scripts", icon: "folder"),
        PickerPathFixture(id: "Cargo.toml", icon: "doc.text"),
        PickerPathFixture(id: "README.md", icon: "doc.text"),
    ]
    private let sessions = [
        PickerHistoryFixture(
            id: "native", title: "Native macOS client", directory: "zz", updatedAt: "Today, 14:05", current: true),
        PickerHistoryFixture(
            id: "input", title: "Keyboard input and reconnect", directory: "zz", updatedAt: "Yesterday, 18:42"),
        PickerHistoryFixture(
            id: "site", title: "Documentation navigation", directory: "zzmux.sh", updatedAt: "Sep 7, 10:20"),
        PickerHistoryFixture(
            id: "theme", title: "Theme colors and pane corners", directory: "zz", updatedAt: "Sep 6, 09:15"),
    ]

    private var visiblePaths: [PickerPathFixture] {
        paths.filter { query.isEmpty || $0.id.localizedStandardContains(query) }
    }

    private var visibleSessions: [PickerHistoryFixture] {
        sessions.filter {
            (allProjects || $0.directory == "zz")
                && (query.isEmpty || ($0.title + " " + $0.directory).localizedStandardContains(query))
        }
    }

    private var visibleIDs: [String] {
        historyMode ? visibleSessions.map(\.id) : visiblePaths.map(\.id)
    }

    var body: some View {
        GallerySection(
            "Directory and history pickers",
            detail: "Search the local examples, move with ↑ ↓, and press Return to choose."
        ) {
            HStack(spacing: 6) {
                ZZButton("Files and directories", icon: "folder", selected: !historyMode) { open(history: false) }
                ZZButton("Session history", icon: "clock", selected: historyMode) { open(history: true) }
                ZZButton("Empty state") {
                    presented = true; query = "no matching fixture"
                }
            }
            ZStack {
                if presented {
                    ZZPickerOverlay(dismiss: close) {
                        ZZPickerModal(emptyMessage: emptyMessage) {
                            ZZPickerSearch(
                                historyMode ? "Search by title or project…" : "Search files and directories…",
                                query: $query, submit: accept, move: move, cancel: close
                            )
                            if historyMode {
                                HStack(spacing: 6) {
                                    ZZButton(
                                        allProjects ? "All projects" : "This project",
                                        icon: allProjects ? "globe" : "folder", variant: .secondary, size: .xSmall
                                    ) { allProjects.toggle() }
                                    ZZButton("Refresh", icon: "arrow.clockwise", variant: .secondary, size: .xSmall) {
                                        result = "Refreshed the session examples."
                                    }
                                    Spacer(minLength: 0)
                                }
                            }
                        } rows: {
                            if historyMode {
                                ForEach(visibleSessions) { session in
                                    ZZHistoryRow(
                                        session.title, directory: session.directory, updatedAt: session.updatedAt,
                                        selected: selected == session.id, current: session.current,
                                        action: {
                                            selected = session.id; accept()
                                        }, hover: { selected = session.id }
                                    )
                                }
                            } else {
                                ForEach(visiblePaths) { path in
                                    if path.icon == "folder" {
                                        ZZDirectoryRow(
                                            path.id, selected: selected == path.id,
                                            action: {
                                                selected = path.id; accept()
                                            }, hover: { selected = path.id }
                                        )
                                    } else {
                                        ZZPathRow(
                                            path.id, icon: path.icon, selected: selected == path.id,
                                            action: {
                                                selected = path.id; accept()
                                            }, hover: { selected = path.id }
                                        )
                                    }
                                }
                            }
                        } footer: {
                            ZZShortcutHints(
                                [.init("↑ ↓", "select"), .init("Enter", "open"), .init("Esc", "close")], spacing: 6)
                            Spacer(minLength: 0)
                        }
                    }
                } else {
                    ZZButton("Open picker", icon: historyMode ? "clock" : "folder") { presented = true }
                }
            }
            .frame(height: 460)
            Text(result).font(theme.font(size: 11)).foregroundStyle(theme.foreground.muted().color)
        }
        .onChange(of: query) { selectFirst() }
        .onChange(of: allProjects) { selectFirst() }
    }

    private var emptyMessage: String? {
        guard visibleIDs.isEmpty else { return nil }
        return historyMode ? "No sessions match that search." : "Nothing matches that search."
    }

    private func open(history: Bool) {
        historyMode = history
        query = ""
        presented = true
        selectFirst()
    }

    private func selectFirst() { selected = visibleIDs.first ?? "" }

    private func close() { presented = false }

    private func move(_ direction: Int) {
        let ids = visibleIDs
        guard !ids.isEmpty else { return }
        let index = ids.firstIndex(of: selected) ?? 0
        selected = ids[min(ids.count - 1, max(0, index + direction))]
    }

    private func accept() {
        guard visibleIDs.contains(selected) else { return }
        let label = historyMode ? sessions.first(where: { $0.id == selected })?.title ?? selected : selected
        result = "Selected: \(label)"
        close()
    }
}
