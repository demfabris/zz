import SwiftUI
import ZZUI

struct OverlayGallery: View {
    @State private var showDialog = false
    @State private var showAlert = false
    @State private var showPopover = false
    @State private var sessionName = "Focus time"
    @State private var pinned = true
    @State private var action = "Ready"
    let toasts: ZZToastCenter

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            GallerySection(
                "Dialogs",
                detail: "Compact content and small actions, with native sheet focus and dismissal."
            ) {
                HStack(spacing: 8) {
                    ZZButton("Rename session", icon: "pencil") { showDialog = true }
                    ZZButton("Show alert", variant: .danger) { showAlert = true }
                    Text(action).font(.system(size: 12)).foregroundStyle(.secondary)
                }
            }
            GallerySection("Menus, popovers and tooltips") {
                HStack(spacing: 12) {
                    ZZMenu("Session", icon: "rectangle.split.2x2") {
                        Button("New window", systemImage: "plus") { action = "Created a window" }
                        Button("Rename", systemImage: "pencil") { showDialog = true }
                        Toggle("Pinned", isOn: $pinned)
                        Divider()
                        Menu("Move to workspace") {
                            Button("Personal") { action = "Moved to Personal" }
                            Button("Work") { action = "Moved to Work" }
                        }
                        Divider()
                        Button("Close session", role: .destructive) { showAlert = true }
                    }
                    ZZPopover(isPresented: $showPopover) {
                        Label("Session details", systemImage: "info.circle")
                            .font(.system(size: 13)).padding(.horizontal, 10)
                            .frame(height: 28).zzControlSurface(focused: showPopover)
                    } content: {
                        VStack(alignment: .leading, spacing: 10) {
                            Text(sessionName).font(.system(size: 13, weight: .semibold))
                            Text("3 windows · 8 panes").font(.system(size: 12))
                            ZZButton("Done") { showPopover = false }
                        }
                        .frame(width: 220, alignment: .leading)
                    }
                    ZZIconButton("New window", systemName: "plus") { action = "Created a window" }
                        .zzTooltip("New window", shortcut: "⌘N")
                }
                Text("Right-click this session row")
                    .font(.system(size: 13)).padding(12)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .zzControlSurface()
                    .zzContextMenu {
                        Button("Rename") { showDialog = true }
                        Toggle("Pinned", isOn: $pinned)
                        Button("Close", role: .destructive) { showAlert = true }
                    }
            }
            GallerySection(
                "Notifications",
                detail:
                    "Toasts appear at the top center, then dismiss after five seconds. Persistent notices can be dismissed by identity."
            ) {
                HStack(spacing: 8) {
                    ForEach(ZZToastKind.allCases, id: \.self) { kind in
                        ZZButton(kind.rawValue.capitalized) {
                            toasts.show(message(for: kind), title: title(for: kind), kind: kind)
                        }
                    }
                }
                HStack(spacing: 8) {
                    ZZButton("Persistent notice") {
                        toasts.show(
                            "Waiting for the remote host…", title: "Connecting", key: "connection", duration: nil)
                    }
                    ZZButton("Resolve notice") { toasts.dismiss(key: "connection") }
                    ZZButton("Burst of 15") {
                        for number in 1...15 {
                            toasts.show(
                                "Window \(number) is ready.", title: "Attached", kind: .info, duration: nil)
                        }
                    }
                    ZZButton("Dismiss all", variant: .ghost) { toasts.dismissAll() }
                }
                ZZButton("Archive preview session", icon: "archivebox") {
                    let previous = action
                    action = "Archived the preview session"
                    toasts.show(
                        "The preview session was archived.", title: "Session archived", kind: .success,
                        key: "archive-preview", duration: nil
                    ) { dismiss in
                        ZZButton("Undo", icon: "arrow.uturn.backward", variant: .ghost) {
                            action = previous
                            dismiss()
                        }
                        .padding(.top, 4)
                    }
                }
            }
        }
        .sheet(isPresented: $showDialog) {
            ZZDialog("Rename session", description: "Choose a name that helps you find this workspace.") {
                ZZTextField("Session name", text: $sessionName)
            } actions: {
                ZZButton("Cancel") { showDialog = false }.keyboardShortcut(.cancelAction)
                ZZButton("Rename", variant: .primary) {
                    action = "Renamed to \(sessionName)"
                    showDialog = false
                }.keyboardShortcut(.defaultAction)
            }
        }
        .sheet(isPresented: $showAlert) {
            ZZDialog("Close this session?", description: "The session contains 3 windows and 8 panes.") {
                ZZButton("Cancel") { showAlert = false }.keyboardShortcut(.cancelAction)
                ZZButton("Close session", variant: .danger) {
                    action = "Closed the preview session"
                    showAlert = false
                }.keyboardShortcut(.defaultAction)
            }
        }
    }

    private func title(for kind: ZZToastKind) -> String {
        switch kind {
        case .info: "Session attached"
        case .success: "Saved"
        case .warning: "Host unavailable"
        case .error: "Connection failed"
        }
    }

    private func message(for kind: ZZToastKind) -> String {
        switch kind {
        case .info: "Your windows are ready."
        case .success: "Your preferences have been saved."
        case .warning: "The remote host has not responded yet."
        case .error: "The remote host refused the connection."
        }
    }
}
