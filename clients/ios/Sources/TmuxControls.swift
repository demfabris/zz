import SwiftUI

struct ZZTmuxState: Decodable, Equatable {
    struct Prompt: Decodable, Equatable {
        var prompt: String
        var input: String
        var mode: String
    }
    struct Search: Decodable, Equatable {
        var query: String
        var reverse: Bool
    }
    struct Tree: Decodable, Equatable {
        struct Item: Decodable, Equatable {
            var label: String
            var detail: String
            var depth: UInt8
            var flags: UInt8
            var key: String
            var text: String
        }
        var items: [Item]
        var search: Search?
        var selected: Int
        var prompt: String
        var help: Bool
    }
    struct Buffers: Decodable, Equatable {
        struct Item: Decodable, Equatable {
            var name: String
            var preview: String
            var key: String
            var text: String
        }
        var items: [Item]
        var search: Search?
        var selected: Int
        var help: Bool
    }
    struct Display: Decodable, Equatable {
        struct Indicator: Decodable, Equatable {
            var pane: UInt64
            var index: UInt32
            var label: String
        }
        var indicators: [Indicator]
    }
    struct Confirm: Decodable, Equatable { var prompt: String }
    struct Menu: Decodable, Equatable {
        struct Item: Decodable, Equatable {
            var name: String
            var key: String?
            var enabled: Bool
        }
        var title: String
        var items: [Item?]
    }
    struct Output: Decodable, Equatable {
        var pane: UInt64
        var text: String
    }
    struct Copy: Decodable, Equatable {
        var pane: UInt64
        var position: UInt32
        var total: UInt32
        var matches: UInt32?
        var matchIndex: UInt32?
    }
    var prompt: Prompt?
    var tree: Tree?
    var buffers: Buffers?
    var display: Display?
    var confirm: Confirm?
    var menu: Menu?
    var output: Output?
    var copies: [Copy] = []

    var hasOverlay: Bool {
        prompt != nil || tree != nil || buffers != nil || display != nil ||
            confirm != nil || menu != nil || output != nil
    }
}

struct ZZTmuxKeyTable: Decodable, Equatable, Identifiable {
    struct Binding: Decodable, Equatable, Identifiable {
        var key: String
        var summary: String
        var note: String?
        var repeats: Bool
        var id: String { key }
    }
    var name: String
    var bindings: [Binding]
    var id: String { name }
}

private func readTmuxJSON<T: Decodable>(_ handle: OpaquePointer?, as type: T.Type) -> T? {
    guard let handle else { return nil }
    defer { zz_json_free(handle) }
    let bytes = zz_json_bytes(handle)
    guard let pointer = bytes.ptr else { return nil }
    let decoder = JSONDecoder()
    decoder.keyDecodingStrategy = .convertFromSnakeCase
    return try? decoder.decode(type, from: Data(bytes: pointer, count: bytes.len))
}

extension ZZStore {
    func refreshTmuxState() {
        guard let client,
              let next = readTmuxJSON(zz_client_tmux_state_json(client), as: ZZTmuxState.self),
              next != tmuxState else { return }
        tmuxState = next
    }

    func refreshTmuxKeyTables() {
        guard let client,
              let next = readTmuxJSON(zz_client_key_tables_json(client), as: [ZZTmuxKeyTable].self),
              next != tmuxKeyTables else { return }
        tmuxKeyTables = next
    }

    func drainTmuxCommands() {
        guard let client else { return }
        while let value = zz_client_gui_command_next(client) {
            let bytes = zz_json_bytes(value)
            if let pointer = bytes.ptr,
               let json = try? JSONSerialization.jsonObject(with: Data(bytes: pointer, count: bytes.len)) as? [String: Any],
               let pane = json["pane"] as? UInt64 {
                if json["kind"] as? String == "browser", let command = json["command"] {
                    handleBrowserCommand(pane: pane, command: command)
                } else if json["kind"] as? String == "terminal",
                          let command = json["command"] as? [String: Any],
                          let begin = command["BeginSearch"] as? [String: Any] {
                    tmuxSearchPane = pane
                    tmuxSearchReverse = begin["direction"] as? String == "Backward"
                }
            }
            zz_json_free(value)
        }
    }

    func tmuxAction(_ family: String, _ action: Any, pane: UInt64? = nil) {
        guard let client else { return }
        var payload: [String: Any] = ["action": action]
        if let pane { payload["pane"] = pane }
        guard let data = try? JSONSerialization.data(withJSONObject: [family: payload]),
              let json = String(data: data, encoding: .utf8) else { return }
        _ = json.withCString { zz_client_tmux_action_json(client, $0) }
    }

    func enterCopyMode(pane: UInt64) {
        requestKeyboard(for: pane)
        tmuxAction("TerminalView", "EnterCopyMode", pane: pane)
    }

    func copyModeCommand(_ command: String, pane: UInt64) {
        _ = execute("send-keys", args: ["-t", "%\(pane)", "-X", command])
    }

    func searchTerminal(_ text: String, pane: UInt64, reverse: Bool) {
        tmuxAction("TerminalView", ["SearchBegin": [
            "text": text, "mode": "Literal", "case": "Smart",
            "direction": reverse ? "Backward" : "Forward"
        ]], pane: pane)
        tmuxSearchPane = nil
        requestKeyboard(for: pane)
    }
}

struct TmuxBindingsView: View {
    @ObservedObject var store: ZZStore
    @State private var query = ""

    var body: some View {
        List {
            ForEach(store.tmuxKeyTables) { table in
                let bindings = table.bindings.filter {
                    query.isEmpty || "\(table.name) \($0.key) \($0.summary) \($0.note ?? "")"
                        .localizedCaseInsensitiveContains(query)
                }
                if !bindings.isEmpty {
                    Section(table.name) {
                        ForEach(bindings) { binding in
                            HStack(alignment: .top) {
                                Text(binding.key).font(.system(.body, design: .monospaced)).frame(minWidth: 70, alignment: .leading)
                                VStack(alignment: .leading, spacing: 4) {
                                    Text(binding.note.flatMap { $0.isEmpty ? nil : $0 } ?? binding.summary)
                                    if binding.note != nil { Text(binding.summary).font(.caption).foregroundStyle(.secondary) }
                                    if binding.repeats { Text("Repeats after prefix").font(.caption).foregroundStyle(.secondary) }
                                }
                            }
                        }
                    }
                }
            }
        }
        .navigationTitle("Keyboard Bindings")
        .searchable(text: $query)
    }
}

private struct TmuxToolbarButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .frame(minWidth: 44, minHeight: 44)
            .contentShape(.rect)
            .foregroundStyle(.tint)
            .opacity(configuration.isPressed ? 0.5 : 1)
    }
}

struct TmuxCopyBar: View {
    @ObservedObject var store: ZZStore
    let pane: UInt64

    var body: some View {
        if let copy = store.tmuxState.copies.first(where: { $0.pane == pane }) {
            ScrollView(.horizontal) {
                HStack(spacing: 14) {
                    Text("Copy \(copy.position)/\(copy.total)").font(.caption.monospacedDigit())
                    Button("Select", systemImage: "selection.pin.in.out") { store.copyModeCommand("begin-selection", pane: pane) }
                    Menu("Move Cursor", systemImage: "arrow.up.and.down.and.arrow.left.and.right") {
                        Button("Left", systemImage: "arrow.left") { store.copyModeCommand("cursor-left", pane: pane) }
                        Button("Right", systemImage: "arrow.right") { store.copyModeCommand("cursor-right", pane: pane) }
                        Button("Up", systemImage: "arrow.up") { store.copyModeCommand("cursor-up", pane: pane) }
                        Button("Down", systemImage: "arrow.down") { store.copyModeCommand("cursor-down", pane: pane) }
                        Button("Select Line") { store.copyModeCommand("select-line", pane: pane) }
                        Button("Rectangle Selection") { store.copyModeCommand("rectangle-toggle", pane: pane) }
                    }
                    Button("Copy", systemImage: "doc.on.doc") { store.copySelection(pane: pane) }
                    Button("Find", systemImage: "magnifyingglass") {
                        store.tmuxSearchReverse = false
                        store.tmuxSearchPane = pane
                    }
                    .accessibilityIdentifier("copy-mode-find-\(pane)")
                    if let matches = copy.matches {
                        Text("\(copy.matchIndex ?? 0)/\(matches)").font(.caption.monospacedDigit())
                        Button("Previous match", systemImage: "chevron.up") { store.tmuxAction("TerminalView", "SearchPrevious", pane: pane) }.labelStyle(.iconOnly)
                        Button("Next match", systemImage: "chevron.down") { store.tmuxAction("TerminalView", "SearchNext", pane: pane) }.labelStyle(.iconOnly)
                    }
                    Button("Page up", systemImage: "arrow.up.to.line") { store.copyModeCommand("page-up", pane: pane) }.labelStyle(.iconOnly)
                    Button("Page down", systemImage: "arrow.down.to.line") { store.copyModeCommand("page-down", pane: pane) }.labelStyle(.iconOnly)
                    Button("Done") { store.copyModeCommand("cancel", pane: pane) }
                        .accessibilityIdentifier("copy-mode-done-\(pane)")
                }
                .buttonStyle(TmuxToolbarButtonStyle())
                .fixedSize(horizontal: true, vertical: false)
                .frame(minHeight: 44)
                .padding(.horizontal, 12)
            }
            .scrollIndicators(.hidden)
            .frame(height: 44)
            .background(.bar)
            .accessibilityIdentifier("copy-mode-bar-\(pane)")
        }
    }
}

struct TmuxOverlay: View {
    @ObservedObject var store: ZZStore

    var body: some View {
        if store.tmuxState.hasOverlay || store.tmuxSearchPane != nil {
            ZStack {
                Color.black.opacity(0.25).ignoresSafeArea()
                VStack(spacing: 0) { overlayContent }
                    .frame(maxWidth: 600, maxHeight: 520)
                    .background(.regularMaterial, in: .rect(cornerRadius: 18))
                    .padding(20)
            }
        }
    }

    @ViewBuilder private var overlayContent: some View {
        if let confirm = store.tmuxState.confirm {
            VStack(spacing: 20) {
                Text(confirm.prompt).font(.headline)
                HStack {
                    Button("Cancel") { store.tmuxAction("Confirm", ["Reply": false]) }
                    Spacer()
                    Button("Confirm", role: .destructive) { store.tmuxAction("Confirm", ["Reply": true]) }
                }
            }.padding()
        } else if let prompt = store.tmuxState.prompt {
            TmuxPromptEditor(store: store, prompt: prompt)
        } else if let pane = store.tmuxSearchPane {
            TmuxSearchEditor(store: store, pane: pane)
        } else if let tree = store.tmuxState.tree {
            chooserHeader("Sessions, Windows & Panes", family: "ChooseTree")
            if tree.help { TmuxChooserHelp(store: store, table: "choose-tree") }
            if !tree.prompt.isEmpty {
                HStack {
                    Text(tree.prompt)
                    Button("Yes", role: .destructive) { store.sendText("y", to: store.selectedPaneID ?? store.selectedSession?.activeWindow?.activePane ?? 0) }
                    Button("No") { store.sendText("n", to: store.selectedPaneID ?? store.selectedSession?.activeWindow?.activePane ?? 0) }
                }.padding()
            }
            if let search = tree.search { chooserSearch(search.query, family: "ChooseTree") }
            List(Array(tree.items.enumerated()), id: \.offset) { index, item in
                Button { store.tmuxAction("ChooseTree", ["ActivateIndex": index]) } label: {
                    HStack {
                        Text(item.key).font(.caption.monospaced()).frame(width: 24)
                        VStack(alignment: .leading) {
                            Text(item.text.isEmpty ? item.label : item.text)
                            if !item.detail.isEmpty { Text(item.detail).font(.caption).foregroundStyle(.secondary) }
                        }
                        .padding(.leading, CGFloat(item.depth) * 12)
                        Spacer()
                        if index == tree.selected { Image(systemName: "checkmark") }
                    }.frame(minHeight: 44)
                }.tint(.primary)
            }.scrollContentBackground(.hidden)
            HStack {
                Button("Collapse", systemImage: "chevron.left") { store.tmuxAction("ChooseTree", "Collapse") }
                Spacer()
                Button("Expand", systemImage: "chevron.right") { store.tmuxAction("ChooseTree", "Expand") }
            }.padding()
        } else if let buffers = store.tmuxState.buffers {
            chooserHeader("Paste Buffers", family: "ChooseBuffer")
            if buffers.help { TmuxChooserHelp(store: store, table: "choose-buffer") }
            if let search = buffers.search { chooserSearch(search.query, family: "ChooseBuffer") }
            List(Array(buffers.items.enumerated()), id: \.offset) { index, item in
                Button { store.tmuxAction("ChooseBuffer", ["PasteIndex": index]) } label: {
                    HStack {
                        Text(item.key).font(.caption.monospaced()).frame(width: 24)
                        VStack(alignment: .leading) {
                            Text(item.text.isEmpty ? item.name : item.text)
                            Text(item.preview).font(.caption.monospaced()).foregroundStyle(.secondary).lineLimit(3)
                        }
                        Spacer()
                        if index == buffers.selected { Image(systemName: "checkmark") }
                    }.frame(minHeight: 44)
                }.tint(.primary)
            }.scrollContentBackground(.hidden)
        } else if let display = store.tmuxState.display {
            chooserHeader("Select Pane", family: "DisplayPanes", search: false)
            List(display.indicators, id: \.pane) { indicator in
                Button(indicator.label.isEmpty ? "Pane \(indicator.index)" : indicator.label) {
                    store.tmuxAction("DisplayPanes", ["Select": indicator.pane])
                }.frame(minHeight: 44)
            }.scrollContentBackground(.hidden)
        } else if let menu = store.tmuxState.menu {
            HStack { Text(menu.title).font(.headline); Spacer(); Button("Close") { store.tmuxAction("Menu", "Cancel") } }.padding()
            List(Array(menu.items.enumerated()), id: \.offset) { index, item in
                if let item {
                    Button(item.name) { store.tmuxAction("Menu", ["Choose": index]) }.disabled(!item.enabled)
                } else { Divider() }
            }.scrollContentBackground(.hidden)
        } else if let output = store.tmuxState.output {
            HStack {
                Text("Command Output").font(.headline)
                Spacer()
                Button("Done") { store.tmuxAction("CommandOutputView", ["CopyMode": "Cancel"]) }
            }.padding()
            ScrollView([.horizontal, .vertical]) {
                Text(output.text).font(.system(.body, design: .monospaced)).textSelection(.enabled).padding()
            }
            HStack {
                Button("Page up") { store.tmuxAction("CommandOutputView", ["ScrollPages": -1]) }
                Spacer()
                Button("Page down") { store.tmuxAction("CommandOutputView", ["ScrollPages": 1]) }
            }.padding()
        }
    }

    private func chooserHeader(_ title: String, family: String, search: Bool = true) -> some View {
        HStack {
            Text(title).font(.headline)
            Spacer()
            if search {
                Button("Search", systemImage: "magnifyingglass") { store.tmuxAction(family, ["SearchStart": ["reverse": false]]) }.labelStyle(.iconOnly)
            }
            Button("Close") { store.tmuxAction(family, "Close") }
        }.padding()
    }

    private func chooserSearch(_ query: String, family: String) -> some View {
        HStack {
            Text("Search: \(query)").font(.body.monospaced())
            Spacer()
            Button("Done") { store.tmuxAction(family, "SearchAccept") }
            Button("Cancel") { store.tmuxAction(family, "SearchCancel") }
        }.padding(.horizontal)
    }
}

private struct TmuxChooserHelp: View {
    @ObservedObject var store: ZZStore
    let table: String

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    Text("Keyboard Help").font(.headline)
                    Spacer()
                    Button("Done") {
                        if let pane = store.selectedPaneID ?? store.selectedSession?.activeWindow?.activePane {
                            store.sendShortcutKey(UInt32(ZZ_KEY_ESCAPE.rawValue), to: pane)
                        }
                    }
                }
                if let bindings = store.tmuxKeyTables.first(where: { $0.name == table })?.bindings {
                    ForEach(bindings) { binding in
                        Text("\(binding.key)  \(binding.note ?? binding.summary)").font(.caption.monospaced())
                    }
                }
            }.padding()
        }.frame(maxHeight: 220)
    }
}

private struct TmuxPromptEditor: View {
    @ObservedObject var store: ZZStore
    let prompt: ZZTmuxState.Prompt
    @State private var text = ""
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(prompt.prompt).font(.headline)
            TextField("Value", text: $text)
                .textFieldStyle(.roundedBorder)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .focused($focused)
                .onSubmit(submit)
                .onChange(of: text) { _, value in
                    store.tmuxAction("CommandPrompt", ["Update": ["input": value, "cursor": value.unicodeScalars.count]])
                    if prompt.mode == "Single", !value.isEmpty { submit() }
                }
            HStack {
                Button("Cancel") { store.tmuxAction("CommandPrompt", "Close") }
                Spacer()
                Button("Submit", action: submit)
            }
        }
        .padding()
        .onAppear { text = prompt.input; focused = true }
        .onChange(of: prompt.input) { _, value in if text != value { text = value } }
        .onDisappear {
            if let pane = store.selectedPaneID ?? store.selectedSession?.activeWindow?.activePane {
                store.requestKeyboard(for: pane)
            }
        }
    }

    private func submit() { store.tmuxAction("CommandPrompt", ["Submit": ["input": text]]) }
}

private struct TmuxSearchEditor: View {
    @ObservedObject var store: ZZStore
    let pane: UInt64
    @State private var text = ""
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Find in Terminal").font(.headline)
            TextField("Search text", text: $text)
                .textFieldStyle(.roundedBorder).textInputAutocapitalization(.never).autocorrectionDisabled()
                .focused($focused).onSubmit(submit)
            Toggle("Search backward", isOn: $store.tmuxSearchReverse)
            HStack {
                Button("Cancel") { store.tmuxSearchPane = nil; store.requestKeyboard(for: pane) }
                Spacer()
                Button("Find", action: submit).disabled(text.isEmpty)
                    .accessibilityIdentifier("terminal-search-submit")
            }
        }.padding().onAppear { focused = true }
    }

    private func submit() { store.searchTerminal(text, pane: pane, reverse: store.tmuxSearchReverse) }
}
