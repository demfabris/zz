import AppKit
import SwiftUI
import ZZUI

private struct AgentMessageFixture: Identifiable {
    let id: String
    let user: Bool
    let markdown: String
    let hasAttachment: Bool
}

struct AgentCompositionGallery: View {
    @Environment(\.zzTheme) private var theme
    @State private var text = ""
    @State private var running = false
    @State private var followsTail = true
    @State private var reasoning = false
    @State private var tool = true
    @State private var group = false
    @State private var showAttachment = true
    @State private var preview = false
    @State private var model = "Default model"
    @State private var permission = ""
    @State private var status = "Ready"
    @State private var revision = 0
    @State private var messages: [AgentMessageFixture] = []

    var body: some View {
        VStack(spacing: 20) {
            AgentPresentationGallery()
            GallerySection(
                "Agent pane",
                detail:
                    "Write a message, expand activity, change composer options, and preview attachments. Responses here are local fixtures."
            ) {
                ZZPane(active: true, gap: 0) {
                    VStack(spacing: 0) {
                        ZZAgentHeader {
                            HStack(spacing: 8) {
                                Image(systemName: "sparkles")
                                Text("Native client").font(.system(size: 13, weight: .medium))
                                ZZTag(running ? "Working" : status, tone: running ? .warning : .neutral)
                            }
                        } trailing: {
                            ZZIconButton("Clear conversation", systemName: "arrow.counterclockwise") {
                                messages = []
                                revision += 1
                                status = "Reset"
                            }
                            Menu {
                                Button(running ? "End active turn" : "Simulate active turn") { running.toggle() }
                                Toggle("Follow latest", isOn: $followsTail)
                            } label: {
                                Image(systemName: "ellipsis").frame(width: 24, height: 24)
                            }
                            .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
                        }
                        ZZAgentTimeline(revision: revision, followsTail: $followsTail) {
                            transcript
                            ForEach(messages) { message in
                                if message.user {
                                    ZZAgentUserMessage(message.markdown) {
                                        if message.hasAttachment {
                                            ZZAttachmentThumbnail(
                                                Image(systemName: "macwindow"), title: "Workspace reference",
                                                side: ZZAgentMetrics.transcriptAttachment, open: { preview = true })
                                        }
                                    }
                                } else {
                                    ZZAgentAssistantMessage(message.markdown, copy: { copy(message.markdown) })
                                }
                            }
                            if running { ZZAgentAssistantMessage("Checking the native components…", streaming: true) }
                        }
                        composer
                    }
                }.frame(height: 680)
            }
            GallerySection("Permission request") {
                if permission.isEmpty {
                    ZZAgentPermission("Allow this command?", detail: "cargo test -p zz-client") {
                        ZZButton("Deny", variant: .ghost) { permission = "Denied" }
                        ZZButton("Allow once", variant: .primary) { permission = "Allowed once" }
                        ZZButton("Always allow") { permission = "Always allowed for this fixture" }
                    }
                } else {
                    HStack {
                        ZZTag(permission, tone: permission == "Denied" ? .warning : .success)
                        ZZButton("Show request again", variant: .ghost) { permission = "" }
                    }
                }
            }
        }
        .sheet(isPresented: $preview) {
            ZZAttachmentPreview(Image(systemName: "macwindow"), title: "Workspace reference") { preview = false }
        }
    }

    private var transcript: some View {
        VStack(alignment: .leading, spacing: 16) {
            ZZAgentUserMessage("Build a native macOS client. Keep the backend in **Rust**.")
            ZZAgentActivity("Planning the component port", icon: "cpu", expanded: $reasoning) {
                ZZMarkdown(
                    "The existing client core already owns sessions, commands, and transport. Swift can focus on the native interface.",
                    fontSize: 12)
            }
            ZZAgentPlan {
                ZZMarkdown(
                    "1. Match the zz theme and control metrics.\n2. Build the component catalog.\n3. Connect the shared Rust client core.",
                    fontSize: 12)
            }
            ZZAgentActivity("Read files", icon: "doc.text.magnifyingglass", expanded: $group) {
                ZZAgentToolOutput(
                    "crates/zz-ui/src/navigation.rs\ncrates/zz-ui/src/agent/composer.rs\ncrates/zz-client-ffi/include/zz-client.h",
                    title: "Source references")
            }
            ZZAgentActivity("Edited Foundation.swift", icon: "square.and.pencil", expanded: $tool, running: running) {
                ZZAgentDiff(
                    "Sources/ZZUI/Foundation.swift",
                    lines: [
                        .init("context", text: "public struct ZZTheme {", oldLine: 1, newLine: 1),
                        .init("removed", text: "    var radius: CGFloat = 4", oldLine: 2, kind: .removed),
                        .init("added", text: "    var radius: CGFloat = 6", newLine: 2, kind: .added),
                        .init("end", text: "}", oldLine: 3, newLine: 3),
                    ]
                ) { status = "Opened Foundation.swift fixture" }
            }
            ZZAgentAssistantMessage(
                "The first components are ready. They share zz’s seven theme roots, compact spacing, and adaptive corners.\n\nSwiftUI handles native focus and accessibility; AppKit handles rich text editing.",
                copy: {
                    copy(
                        "The first components are ready. They share zz’s seven theme roots, compact spacing, and adaptive corners."
                    )
                })
        }
    }

    private var composer: some View {
        ZZAgentComposer(
            text: $text, canSend: !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || showAttachment,
            running: running, hint: "⌘ Return to send", send: send,
            stop: {
                running = false
                status = "Stopped"
            }
        ) {
            Menu {
                Button("Default model") { model = "Default model" }
                Button("Fast model") { model = "Fast model" }
                Button("Reasoning model") { model = "Reasoning model" }
            } label: {
                Text(model).font(.system(size: 11))
            }.fixedSize()
            ZZIconButton("Attach image", systemName: "paperclip") { showAttachment.toggle() }
        } attachments: {
            if showAttachment {
                ZZAttachmentStrip {
                    ZZAttachmentThumbnail(
                        Image(systemName: "macwindow"), title: "Workspace reference",
                        open: { preview = true }, remove: { showAttachment = false })
                }
            }
        } footer: {
            HStack(spacing: 8) {
                ZZButton(
                    "codex/native-macos", icon: "arrow.triangle.branch", variant: .ghost, size: .xSmall, flat: true
                ) { status = "Branch selected" }
                Spacer(minLength: 0)
                Text("12% context").font(.system(size: 10)).foregroundStyle(theme.foreground.muted().color)
                ZZButton("~/dev/zz", icon: "folder", variant: .ghost, size: .xSmall, flat: true) {
                    status = "Directory selected"
                }
            }
        }
    }

    private func send() {
        let message = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !message.isEmpty || showAttachment else { return }
        revision += 1
        messages.append(.init(id: "user-\(revision)", user: true, markdown: message, hasAttachment: showAttachment))
        messages.append(
            .init(
                id: "assistant-\(revision)", user: false,
                markdown:
                    "Received your message in the local gallery. A full client will send it through the shared Rust backend.",
                hasAttachment: false))
        text = ""
        showAttachment = false
        followsTail = true
        status = "Message sent"
    }

    private func copy(_ message: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(message, forType: .string)
        status = "Copied"
    }
}
