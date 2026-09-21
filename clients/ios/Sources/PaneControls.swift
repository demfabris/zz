import SwiftUI

extension ZZStore {
    func choosePaneKind(_ kind: ZZPaneKind, pane: UInt64) {
        guard [.terminal, .agent, .browser].contains(kind) else { return }
        _ = execute("select-pane-kind", args: ["-t", "%\(pane)", kind.label.lowercased()])
    }

    func splitPane(_ pane: UInt64, horizontal: Bool) {
        _ = execute("split-window", args: ["--kind", "picker", "-t", "%\(pane)", horizontal ? "-h" : "-v"])
    }

    func togglePaneZoom(_ pane: UInt64) {
        _ = execute("resize-pane", args: ["-t", "%\(pane)", "-Z"])
    }

    func resizePane(_ pane: UInt64, direction: ZZPaneResizeDirection, cells: Int) {
        _ = execute("resize-pane", args: ["-t", "%\(pane)", direction.argument, String(max(1, cells))])
    }

    func arrangePanes(_ pane: UInt64, layout: ZZPaneArrangement) {
        _ = execute("select-layout", args: ["-t", "%\(pane)", layout.rawValue])
    }
}

enum ZZPaneResizeDirection: String, CaseIterable, Identifiable {
    case left, down, up, right

    var id: Self { self }

    var argument: String {
        switch self {
        case .left: "-L"
        case .down: "-D"
        case .up: "-U"
        case .right: "-R"
        }
    }

    var symbol: String { "arrow.\(rawValue)" }
}

enum ZZPaneArrangement: String, CaseIterable, Identifiable {
    case columns = "even-horizontal"
    case rows = "even-vertical"
    case mainLeft = "main-vertical"
    case mainTop = "main-horizontal"
    case tiled

    var id: Self { self }

    var label: String {
        switch self {
        case .columns: "Even Columns"
        case .rows: "Even Rows"
        case .mainLeft: "Main Pane on Left"
        case .mainTop: "Main Pane on Top"
        case .tiled: "Tiled"
        }
    }
}

struct PaneKindPicker: View {
    @EnvironmentObject private var store: ZZStore
    @Environment(ZZClientSettings.self) private var settings
    let pane: ZZPane

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text("New Pane")
                    .font(.title2.bold())
                Text("Choose what to open here.")
                    .foregroundStyle(settings.chromeSecondaryForeground)
                choice(.terminal, detail: "Open a shell on this host.")
                choice(.browser, detail: "Browse using this host’s network.")
                choice(.agent, detail: "Start an Agent conversation.")
                Text("Agent panes require experimental-agent-pane on the host.")
                    .font(.caption)
                    .foregroundStyle(settings.chromeSecondaryForeground)
            }
            .frame(maxWidth: 420, alignment: .leading)
            .padding(24)
            .frame(maxWidth: .infinity)
        }
        .defaultScrollAnchor(.center)
        .disabled(!store.isConnected)
        .accessibilityIdentifier("pane-kind-picker-\(pane.id)")
    }

    private func choice(_ kind: ZZPaneKind, detail: String) -> some View {
        Button {
            store.choosePaneKind(kind, pane: pane.id)
        } label: {
            HStack(spacing: 16) {
                Image(systemName: kind.symbol)
                    .font(.title2)
                    .frame(width: 32)
                VStack(alignment: .leading, spacing: 4) {
                    Text(kind.label)
                        .font(.headline)
                    Text(detail)
                        .font(.subheadline)
                        .foregroundStyle(settings.chromeSecondaryForeground)
                }
                Spacer(minLength: 0)
                Image(systemName: "chevron.right")
                    .foregroundStyle(settings.chromeSecondaryForeground)
            }
            .padding(16)
            .frame(maxWidth: .infinity, minHeight: 64, alignment: .leading)
            .foregroundStyle(settings.chromeForeground)
            .background(settings.chromeSurface, in: .rect(cornerRadius: settings.widgetCornerRadius))
            .overlay {
                RoundedRectangle(cornerRadius: settings.widgetCornerRadius)
                    .stroke(settings.chromeBorder, lineWidth: 1)
            }
            .shadow(color: .black.opacity(settings.shadowOpacity), radius: 6, y: 2)
            .contentShape(.rect(cornerRadius: settings.widgetCornerRadius))
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("pane-kind-\(kind.rawValue)-\(pane.id)")
    }
}

struct PaneActionsMenu: View {
    @EnvironmentObject private var store: ZZStore
    @Environment(ZZClientSettings.self) private var settings
    let pane: ZZPane
    var headerStyle = false
    @State private var showsBindings = false
    @State private var showsResize = false
    @State private var confirmsClose = false

    private var isZoomed: Bool {
        store.sessions.lazy.flatMap(\.windows).first { window in
            window.panes.contains { $0.id == pane.id }
        }?.zoomedPane == pane.id
    }

    var body: some View {
        Menu {
            Section {
                Button("Split Right", systemImage: "rectangle.split.2x1") {
                    store.splitPane(pane.id, horizontal: true)
                }
                Button("Split Down", systemImage: "rectangle.split.1x2") {
                    store.splitPane(pane.id, horizontal: false)
                }
                Button(isZoomed ? "Unzoom Pane" : "Zoom Pane", systemImage: "arrow.up.left.and.arrow.down.right") {
                    store.togglePaneZoom(pane.id)
                }
                Button("Resize Pane", systemImage: "arrow.up.and.down.and.arrow.left.and.right") {
                    store.releaseTerminalInput()
                    showsResize = true
                }
                .disabled(isZoomed)
                Menu("Arrange Panes", systemImage: "rectangle.3.group") {
                    ForEach(ZZPaneArrangement.allCases) { layout in
                        Button(layout.label) {
                            store.arrangePanes(pane.id, layout: layout)
                        }
                    }
                }
            }
            if pane.kind == .terminal {
                Section {
                    Button("Prefix", systemImage: "keyboard") {
                        store.sendPrefix(to: pane.id)
                    }
                    Button("Keyboard Bindings", systemImage: "list.bullet") {
                        store.releaseTerminalInput()
                        showsBindings = true
                    }
                    Button("Copy Mode", systemImage: "doc.on.doc") {
                        store.enterCopyMode(pane: pane.id)
                    }
                    Button("Paste Buffer", systemImage: "doc.on.clipboard") {
                        _ = store.execute("paste-buffer", args: ["-t", "%\(pane.id)"])
                    }
                }
            }
            Button("Close Pane", systemImage: "xmark", role: .destructive) {
                confirmsClose = true
            }
        } label: {
            if headerStyle {
                Image(systemName: "ellipsis")
                    .frame(width: 44, height: 44)
                    .contentShape(.rect)
            } else {
                Image(systemName: "ellipsis")
                    .frame(width: 32, height: 32)
                    .background(settings.chromeSurface, in: .rect(cornerRadius: settings.widgetCornerRadius))
                    .overlay {
                        RoundedRectangle(cornerRadius: settings.widgetCornerRadius)
                            .stroke(settings.chromeBorder, lineWidth: 1)
                    }
                    .shadow(color: .black.opacity(settings.shadowOpacity), radius: 3, y: 1)
                    .frame(width: 44, height: 44)
                    .contentShape(.rect)
            }
        }
        .disabled(!store.isConnected)
        .accessibilityLabel("Pane Actions")
        .accessibilityIdentifier("pane-actions-\(pane.id)")
        .sheet(isPresented: $showsResize) {
            PaneResizeSheet(pane: pane)
        }
        .sheet(isPresented: $showsBindings) {
            NavigationStack {
                TmuxBindingsView(store: store)
                    .toolbar {
                        ToolbarItem(placement: .confirmationAction) {
                            Button("Done") { showsBindings = false }
                        }
                    }
            }
        }
        .confirmationDialog("Close pane?", isPresented: $confirmsClose, titleVisibility: .visible) {
            Button("Close Pane", role: .destructive) { store.closePane(pane.id) }
        } message: {
            Text("The process running in this pane will stop.")
        }
    }
}

private struct PaneResizeSheet: View {
    @EnvironmentObject private var store: ZZStore
    @Environment(\.dismiss) private var dismiss
    let pane: ZZPane
    @State private var cells = 5

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Stepper("Step: \(cells) cells", value: $cells, in: 1...50)
                    HStack {
                        ForEach(ZZPaneResizeDirection.allCases) { direction in
                            Button {
                                store.resizePane(pane.id, direction: direction, cells: cells)
                            } label: {
                                Image(systemName: direction.symbol)
                                    .frame(maxWidth: .infinity, minHeight: 48)
                            }
                            .buttonStyle(.bordered)
                            .accessibilityLabel("Resize \(direction.rawValue)")
                            .accessibilityIdentifier("pane-resize-\(direction.rawValue)")
                        }
                    }
                } footer: {
                    Text("Move the pane boundary in the arrow’s direction. Sizes are measured in terminal cells.")
                }
            }
            .disabled(!store.isConnected)
            .navigationTitle("Resize Pane")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
        .presentationDetents([.height(240), .medium])
        .presentationBackgroundInteraction(.enabled)
    }
}
