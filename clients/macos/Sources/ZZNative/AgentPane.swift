import AppKit
import SwiftUI
import ZZNativeCore
import ZZUI

struct NativeAgentPaneView: View {
    let client: NativeClient
    let pane: NativePane
    @Bindable var model: NativeAgentPane
    @Environment(\.zzTheme) private var theme
    @State private var followsTail = true
    @State private var historyVisible = false
    @State private var newSessionVisible = false
    @State private var cwd = ""
    @State private var historySearch = ""
    @State private var pendingDelete: NativeAgentSession?
    private var state: NativeAgentSnapshot? { model.snapshot }
    private var running: Bool { model.running }
    private var canSend: Bool { model.canSend }

    var body: some View {
        VStack(spacing: 0) {
            ZZAgentHeader {
                Text(state?.agent_name ?? pane.agent?.provider ?? "Agent").font(theme.font(size: 12, weight: .medium))
                if let title = state?.title { Text(title).font(theme.font(size: 11)).lineLimit(1) }
                if state?.phase == "starting" || state?.busy == true { ZZSpinner(size: 12) }
            } trailing: {
                Menu {
                    Button("Codex") { client.execute("set-agent-provider", ["-t", "%\(pane.id)", "codex"]) }
                    Button("Claude Code") {
                        client.execute("set-agent-provider", ["-t", "%\(pane.id)", "claude-code"])
                    }
                } label: {
                    Image(systemName: "chevron.down")
                }.menuStyle(.borderlessButton).fixedSize()
                if state?.capabilities.list == true {
                    ZZIconButton("Session history", systemName: "clock.arrow.circlepath") {
                        model.action("list-sessions", ["cwd": NSNull(), "next": false])
                        historyVisible = true
                    }
                }
                ZZIconButton("New agent session", systemName: "plus") {
                    cwd = state?.cwd ?? pane.agent?.cwd ?? "/"
                    newSessionVisible = true
                }.disabled(state?.phase != "ready" || state?.busy == true)
            }
            ZZAgentTimeline(revision: Int(clamping: state?.revision ?? 0), followsTail: $followsTail) {
                if model.entries.isEmpty {
                    ZZAgentEmptyState(
                        state?.phase == "starting" ? "Starting the agent…" : "Start a conversation",
                        busy: state?.phase == "starting")
                }
                ForEach(model.entries) { entry in NativeAgentEntryView(entry: entry) }
                ForEach(state?.permissions ?? []) { permission in permissionCard(permission) }
                if let error = model.localError ?? state?.error {
                    VStack(alignment: .leading, spacing: 4) {
                        ZZAgentError(error)
                        Button("Dismiss") { model.clearError() }.buttonStyle(.plain)
                        if state?.phase == "failed" {
                            Button("Retry agent") { client.execute("restart-agent-pane", ["-t", "%\(pane.id)"]) }
                        }
                    }
                }
                ForEach(state?.auth_methods ?? []) { method in
                    Button("Sign in with \(method.name)") { model.action("authenticate", ["method": method.id]) }
                        .help(method.description ?? "").disabled(state?.busy == true)
                }
            }.id(state?.epoch ?? 0)
            if !model.completions.isEmpty {
                ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach(model.completions) { completion in
                            ZZAgentSuggestionRow(
                                completion.command.name, description: completion.command.description,
                                selected: model.completions.firstIndex(where: { $0.id == completion.id })
                                    == model.selectedCompletion
                            ) {
                                model.complete(completion)
                            }
                        }
                    }
                }.frame(maxHeight: 6 * 52)
            }
            ZZAgentComposer(
                text: $model.draft, canSend: canSend, running: running,
                hint: model.commandHint, focusRequest: model.focusRequest, send: { model.send() },
                stop: { model.action("cancel") }
            ) {
                settings
            } attachments: {
                if !model.attachments.isEmpty {
                    ScrollView(.horizontal) {
                        HStack {
                            ForEach(Array(model.attachments.enumerated()), id: \.offset) { index, attachment in
                                if let image = attachment.image {
                                    Image(nsImage: image).resizable().scaledToFit().frame(width: 56, height: 56)
                                        .overlay(alignment: .topTrailing) {
                                            Button {
                                                if model.attachments.indices.contains(index) {
                                                    model.attachments.remove(at: index)
                                                }
                                            } label: {
                                                Image(systemName: "xmark.circle.fill")
                                            }
                                            .buttonStyle(.plain).help("Remove image")
                                        }
                                }
                            }
                        }.padding(8)
                    }
                }
            } footer: {
                HStack(spacing: 8) {
                    Text(state?.cwd ?? "").lineLimit(1).truncationMode(.middle)
                    if let git = state?.git { Text("\(git.branch ?? "Detached") +\(git.additions) −\(git.deletions)") }
                    Spacer(minLength: 0)
                    if let usage = state?.usage, usage.count == 2 { Text("\(usage[0]) / \(usage[1])") }
                    if let queued = state?.queued_prompts, queued > 0 {
                        Button("\(queued) queued · restore") { model.action("unqueue") }
                    }
                }.font(theme.font(size: 10)).foregroundStyle(theme.foreground.muted().color)
            }
        }
        .background(theme.background.color)
        .onAppear { if pane.active { model.focusRequest += 1 } }
        .onChange(of: pane.active) { _, active in if active { model.focusRequest += 1 } }
        .onChange(of: model.draft) { model.updateCompletions() }
        .onReceive(NotificationCenter.default.publisher(for: NSTextView.didChangeSelectionNotification)) {
            notification in
            if pane.active, let editor = notification.object as? NSTextView, editor.string == model.draft {
                model.updateCompletions()
            }
        }
        .task {
            while !Task.isCancelled {
                do { try await Task.sleep(for: .seconds(1)) } catch { return }
                model.poll()
            }
        }
        .sheet(isPresented: $historyVisible, onDismiss: { model.focusRequest += 1 }) { history }
        .sheet(isPresented: $newSessionVisible, onDismiss: { model.focusRequest += 1 }) {
            ZZDialog("New agent session", description: "Choose the working directory for this conversation.") {
                ZZTextField("Working directory", text: $cwd)
            } actions: {
                ZZButton("Cancel") { newSessionVisible = false }
                ZZButton("Create", variant: .primary) {
                    if model.action("new-session", ["cwd": cwd]) { newSessionVisible = false }
                }
            }
        }
    }

    @ViewBuilder private var settings: some View {
        ForEach(state?.options ?? []) { option in
            Menu {
                ForEach(option.choices, id: \.value) { choice in
                    Button {
                        model.action("configure", ["option": option.id, "value": choice.value])
                    } label: {
                        if choice.value == option.current_value {
                            Label(choice.name, systemImage: "checkmark")
                        } else {
                            Text(choice.name)
                        }
                    }.help(choice.description ?? "")
                }
            } label: {
                Text(option.choices.first(where: { $0.value == option.current_value })?.name ?? option.name)
                    .font(theme.font(size: 11)).lineLimit(1)
            }.menuStyle(.borderlessButton).fixedSize().help(option.description ?? option.name)
                .disabled(state?.phase != "ready" || state?.busy == true)
        }
        if let modes = state?.modes, !modes.isEmpty {
            Menu(state?.modes.first(where: { $0.id == state?.mode })?.name ?? "Mode") {
                ForEach(modes) { mode in Button(mode.name) { model.action("mode", ["value": mode.id]) } }
            }.menuStyle(.borderlessButton).fixedSize().disabled(state?.phase != "ready" || state?.busy == true)
        }
        if state?.capabilities.images == true {
            ZZIconButton("Attach images", systemName: "paperclip") {
                let panel = NSOpenPanel()
                panel.allowedContentTypes = [.png, .jpeg, .gif, .webP]
                panel.allowsMultipleSelection = true
                if panel.runModal() == .OK { model.attachImages(panel.urls) }
            }
        }
    }

    private func permissionCard(_ permission: NativeAgentPermission) -> some View {
        ZZAgentPermissionCard(permission.title) {
            ForEach(Array(permission.options.enumerated()), id: \.element.id) { index, option in
                ZZAgentPermissionOption(index: index, highlighted: model.selectedPermission == index) {
                    Button(option.name) {
                        model.action("permission", ["request_id": permission.request_id, "option_id": option.id])
                    }
                    .buttonStyle(.plain).frame(maxWidth: .infinity, alignment: .leading)
                }
            }
        } cancel: {
            Button("Cancel") {
                model.action("permission", ["request_id": permission.request_id, "option_id": NSNull()])
            }
        }.padding(.vertical, 4)
    }

    private var history: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Agent sessions").font(.headline)
            TextField("Filter sessions", text: $historySearch)
            if let error = state?.history.error { ZZAgentError(error) }
            List {
                ForEach(
                    (state?.history.sessions ?? []).filter {
                        historySearch.isEmpty
                            || ($0.title ?? $0.sessionId).localizedCaseInsensitiveContains(historySearch)
                            || $0.cwd.localizedCaseInsensitiveContains(historySearch)
                    }
                ) { session in
                    HStack {
                        VStack(alignment: .leading) {
                            Text(session.title ?? session.sessionId).lineLimit(1)
                            Text(session.cwd).font(.caption).foregroundStyle(.secondary)
                        }
                        Spacer()
                        if state?.capabilities.load == true {
                            Button("Open") {
                                if model.action(
                                    "load-session",
                                    [
                                        "session_id": session.sessionId, "cwd": session.cwd,
                                        "directories": session.additionalDirectories,
                                    ])
                                {
                                    historyVisible = false
                                }
                            }
                        }
                        if state?.capabilities.delete == true {
                            Button("Delete", role: .destructive) { pendingDelete = session }
                        }
                    }
                }
            }
            HStack {
                if state?.history.loading == true { ProgressView().controlSize(.small) }
                if state?.history.next_cursor != nil {
                    Button("Load more") { model.action("list-sessions", ["cwd": NSNull(), "next": true]) }
                        .disabled(state?.history.loading == true)
                }
                Spacer()
                Button("Done") { historyVisible = false }.keyboardShortcut(.cancelAction)
            }
        }.padding(20).frame(width: 600, height: 440)
            .confirmationDialog(
                "Delete this agent session?",
                isPresented: Binding(get: { pendingDelete != nil }, set: { if !$0 { pendingDelete = nil } })
            ) {
                if let session = pendingDelete {
                    Button("Delete session", role: .destructive) {
                        model.action("delete-session", ["session_id": session.sessionId]); pendingDelete = nil
                    }
                }
            }
    }
}

private struct NativeAgentEntryView: View {
    let entry: NativeAgentEntry
    @State private var expanded = false
    var body: some View {
        Group {
            switch entry.type {
            case "user":
                ZZAgentUserMessage(entry.markdown ?? "") {
                    if let images = entry.images {
                        HStack {
                            ForEach(images.indices, id: \.self) { index in
                                if let image = images[index].image {
                                    Image(nsImage: image).resizable().scaledToFit().frame(maxHeight: 140)
                                }
                            }
                        }
                    }
                }
            case "assistant":
                ZZAgentAssistantMessage(entry.markdown ?? "") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(entry.markdown ?? "", forType: .string)
                }
            case "reasoning":
                ZZAgentActivity(entry.label ?? "Thinking", icon: "brain", expanded: $expanded) {
                    ZZMarkdown(entry.markdown ?? "")
                }
            case "plan": ZZAgentPlan { ZZMarkdown(entry.markdown ?? "") }
            case "tool":
                ZZAgentActivity(entry.label ?? "Tool", expanded: $expanded, running: entry.status == "Running") {
                    VStack(alignment: .leading, spacing: 8) {
                        if let location = entry.location { Text(location).font(.caption).textSelection(.enabled) }
                        if let input = entry.input { payload(input) }
                        ForEach(Array((entry.output ?? []).enumerated()), id: \.offset) { _, value in payload(value) }
                        if entry.status == "Failed" || entry.status == "Canceled" {
                            Text(entry.status ?? "").foregroundStyle(.secondary)
                        }
                    }
                }
            default: EmptyView()
            }
        }.onAppear { expanded = entry.default_expanded ?? false }
    }

    @ViewBuilder private func payload(_ value: NativeAgentPayload) -> some View {
        if let diff = value.diff {
            VStack(alignment: .leading, spacing: 4) {
                Text(diff.path).font(.caption).textSelection(.enabled)
                if let old = diff.old { ZZAgentToolOutput(old, title: "Before") }
                ZZAgentToolOutput(diff.new, title: "After")
            }
        } else if let text = value.text {
            ZZAgentToolOutput(text)
        }
    }
}
