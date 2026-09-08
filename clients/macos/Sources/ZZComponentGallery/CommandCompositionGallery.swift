import SwiftUI
import ZZUI

private struct CommandFixture: Identifiable {
    let id: String
    let label: String
    let detail: String
    let kind: String
}

struct CommandCompositionGallery: View {
    @State private var query = ""
    @State private var selected = "split"
    @State private var executed = "Select a command to try it."
    @State private var paletteVisible = true
    @State private var chooserVisible = true
    @State private var chooserSearch = ""
    @State private var searching = false
    @State private var chooserSelected = "dev"
    @State private var help = false
    @State private var prompt = false
    @State private var buffer = "buffer0"
    @State private var buffersVisible = true
    @State private var taggedWindows: Set<String> = ["server"]
    @State private var taggedBuffers: Set<String> = ["buffer1"]
    @State private var entries = ["dev", "build", "server"]

    private let commands = [
        CommandFixture(id: "split", label: "split-window -h", detail: "Split horizontally", kind: "tmux"),
        CommandFixture(id: "new", label: "new-window", detail: "Open a new window", kind: "tmux"),
        CommandFixture(id: "agent", label: "New agent pane", detail: "Start a conversation", kind: "zz"),
        CommandFixture(id: "settings", label: "Open settings", detail: "Configure workspace", kind: "zz"),
        CommandFixture(id: "zoom", label: "resize-pane -Z", detail: "Toggle pane zoom", kind: "tmux"),
    ]
    private let buffers = ["buffer0", "buffer1"]

    private var visibleCommands: [CommandFixture] {
        query.isEmpty
            ? commands
            : commands.filter {
                $0.label.localizedStandardContains(query) || $0.detail.localizedStandardContains(query)
            }
    }
    private var visibleEntries: [String] {
        entries.filter { chooserSearch.isEmpty || $0.localizedStandardContains(chooserSearch) }
    }

    var body: some View {
        VStack(spacing: 20) {
            FloatingCommandGallery()
            paletteSection
            treeSection
            bufferSection
            PickerGallery()
        }
        .onChange(of: query) { selected = visibleCommands.first?.id ?? "" }
        .onChange(of: chooserSearch) {
            if !visibleEntries.contains(chooserSelected) { chooserSelected = visibleEntries.first ?? "" }
        }
    }

    private var paletteSection: some View {
        GallerySection(
            "Command palette",
            detail: "Type to filter, use the arrow keys to select, and press Return to run a fixture action."
        ) {
            if paletteVisible {
                ZZCommandPalette(
                    query: $query, rowCount: visibleCommands.count, submit: execute, cancel: { paletteVisible = false },
                    move: move
                ) {
                    ForEach(visibleCommands) { command in
                        ZZCommandPaletteRow(
                            command.label, detail: command.detail, kind: command.kind,
                            selected: selected == command.id
                        ) {
                            selected = command.id
                            execute()
                        }
                    }
                    if visibleCommands.isEmpty {
                        Text("No matching commands").font(.system(size: 12)).padding(20)
                    }
                }.frame(maxWidth: .infinity)
            } else {
                ZZButton("Open command palette", icon: "command") { paletteVisible = true }
            }
            Text(executed).font(.system(size: 11)).textSelection(.enabled)
        }
    }

    private var treeSection: some View {
        GallerySection(
            "Tree chooser",
            detail: "40-point rows, a compact header and footer, and the shared workspace selection fill."
        ) {
            HStack {
                ZZButton("Help", selected: help) { help.toggle() }
                ZZButton("Confirmation prompt", selected: prompt) { prompt.toggle() }
                ZZButton("Search", icon: "magnifyingglass", selected: searching) { searching.toggle() }
                if !chooserVisible { ZZButton("Open chooser") { chooserVisible = true } }
            }
            if chooserVisible {
                ZZChooserModal(
                    "Choose a window", subtitle: "\(entries.count) windows · local", rowCount: visibleEntries.count,
                    prompt: prompt ? "Kill selected window? (y/n)" : nil,
                    help: help ? "↑ ↓ move · Enter choose · / search · Space tag" : nil,
                    close: closeTree, move: moveTree, accept: acceptTree,
                    toggleTag: treeTagAction
                ) {
                    ForEach(visibleEntries, id: \.self) { entry in
                        ZZTreeChooserRow(
                            entry, detail: entry == "dev" ? "3 panes" : "1 pane",
                            target: "@\(entries.firstIndex(of: entry) ?? 0)",
                            icon: entry == "dev" ? "terminal" : "rectangle",
                            key: "\(entries.firstIndex(of: entry) ?? 0)",
                            active: entry == "dev", tagged: taggedWindows.contains(entry),
                            selected: chooserSelected == entry
                        ) {
                            chooserSelected = entry
                            executed = "Selected window: \(entry)"
                        }
                    }
                } footer: {
                    ZZChooserFooter(
                        search: searching ? $chooserSearch : nil,
                        hints: treeHints,
                        submit: acceptTree)
                }
                .onKeyPress("/") {
                    if consumeNotice() { return .handled }
                    guard !searching else { return .ignored }
                    searching = true
                    return .handled
                }
                .onKeyPress(characters: .decimalDigits) { key in
                    if consumeNotice() { return .handled }
                    guard !searching, let index = Int(key.characters), visibleEntries.indices.contains(index) else {
                        return .ignored
                    }
                    chooserSelected = visibleEntries[index]
                    acceptTree()
                    return .handled
                }
                .onKeyPress { key in
                    if help {
                        help = false
                        return .handled
                    }
                    guard prompt else { return .ignored }
                    if key.characters.lowercased() == "y" {
                        entries.removeAll { $0 == chooserSelected }
                        chooserSelected = visibleEntries.first ?? ""
                        executed = "Removed the selected fixture window."
                        prompt = false
                    } else if key.characters.lowercased() == "n" {
                        prompt = false
                    }
                    return .handled
                }
                .frame(maxWidth: .infinity)
            }
        }
    }

    private var treeTagAction: (() -> Void)? {
        if searching { return nil }
        return { toggleTreeTag() }
    }

    private var treeHints: [ZZShortcutHint] {
        if searching { return [.init("↵", "choose"), .init("esc", "end search")] }
        return [.init("↑ ↓", "move"), .init("↵", "choose"), .init("space", "tag"), .init("/", "search")]
    }

    private var bufferSection: some View {
        GallerySection("Paste buffers") {
            if buffersVisible {
                ZZChooserModal(
                    "Choose a buffer", subtitle: "2 buffers", maxWidth: 640, rowCount: 2,
                    close: { buffersVisible = false }, move: moveBuffer, accept: pasteBuffer,
                    toggleTag: { toggleTag(buffer, in: &taggedBuffers) }
                ) {
                    ZZBufferChooserRow(
                        "buffer0", preview: "cargo test -p zz-client", size: "24 bytes", age: "1m", key: "0",
                        selected: buffer == "buffer0", tagged: taggedBuffers.contains("buffer0")
                    ) {
                        buffer = "buffer0"
                        executed = "Selected buffer0"
                    }
                    ZZBufferChooserRow(
                        "buffer1", preview: "ssh archbox", size: "11 bytes", age: "4m", key: "1",
                        selected: buffer == "buffer1", tagged: taggedBuffers.contains("buffer1")
                    ) {
                        buffer = "buffer1"
                        executed = "Selected buffer1"
                    }
                } footer: {
                    ZZChooserFooter(hints: [
                        .init("↑ ↓", "move"), .init("↵", "paste"), .init("space", "tag"), .init("esc", "cancel"),
                    ])
                }
                .onKeyPress(characters: .decimalDigits) { key in
                    guard let index = Int(key.characters), buffers.indices.contains(index) else { return .ignored }
                    buffer = buffers[index]
                    pasteBuffer()
                    return .handled
                }
                .frame(maxWidth: .infinity)
            } else {
                ZZButton("Open buffer chooser") { buffersVisible = true }
            }
        }
    }

    private func execute() {
        if let command = visibleCommands.first(where: { $0.id == selected }) {
            executed = "Ran fixture action: \(command.label)"
        }
    }

    private func move(_ offset: Int) {
        guard !visibleCommands.isEmpty else { return }
        let current = visibleCommands.firstIndex(where: { $0.id == selected }) ?? 0
        selected = visibleCommands[min(visibleCommands.count - 1, max(0, current + offset))].id
    }

    private func consumeNotice() -> Bool {
        if help {
            help = false
            return true
        }
        return prompt
    }

    private func moveTree(_ offset: Int) {
        guard !consumeNotice(), !visibleEntries.isEmpty else { return }
        let current = visibleEntries.firstIndex(of: chooserSelected) ?? 0
        chooserSelected = visibleEntries[min(visibleEntries.count - 1, max(0, current + offset))]
    }

    private func acceptTree() {
        guard !consumeNotice(), visibleEntries.contains(chooserSelected) else { return }
        executed = "Selected window: \(chooserSelected)"
        chooserVisible = false
    }

    private func closeTree() {
        if help {
            help = false
        } else if prompt {
            prompt = false
        } else if searching {
            searching = false
            chooserSearch = ""
        } else {
            chooserVisible = false
        }
    }

    private func toggleTreeTag() {
        guard !consumeNotice(), visibleEntries.contains(chooserSelected) else { return }
        toggleTag(chooserSelected, in: &taggedWindows)
    }

    private func toggleTag(_ item: String, in set: inout Set<String>) {
        if set.contains(item) { set.remove(item) } else { set.insert(item) }
    }

    private func moveBuffer(_ offset: Int) {
        let current = buffers.firstIndex(of: buffer) ?? 0
        buffer = buffers[min(buffers.count - 1, max(0, current + offset))]
    }

    private func pasteBuffer() {
        executed = "Pasted fixture \(buffer)."
        buffersVisible = false
    }
}
