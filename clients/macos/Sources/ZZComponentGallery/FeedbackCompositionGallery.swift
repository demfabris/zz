import SwiftUI
import ZZUI

private enum FeedbackFixture: String, Identifiable {
    case host, secret, confirm, clear, importConfig, attachment
    var id: Self { self }
}

struct FeedbackCompositionGallery: View {
    @Environment(\.zzTheme) private var theme
    @State private var sheet: FeedbackFixture?
    @State private var host = "archbox"
    @State private var secret = ""
    @State private var hostError: String?
    @State private var result = "Choose a prompt to preview it."
    @State private var attached = true

    var body: some View {
        VStack(spacing: 20) {
            GallerySection(
                "Connection prompts", detail: "Native sheets contain the same compact forms as the GPUI client."
            ) {
                HStack(spacing: 12) {
                    ZZButton("Add host", icon: "plus") { sheet = .host }
                    ZZButton("SSH password", icon: "key") { sheet = .secret }
                    ZZButton("Host key confirmation", icon: "checkmark.shield") { sheet = .confirm }
                }
                Text(result).font(.system(size: 12)).foregroundStyle(theme.foreground.muted().color)
            }
            GallerySection("Confirmations") {
                HStack(spacing: 12) {
                    ZZButton("Clear site data", icon: "trash", variant: .danger) { sheet = .clear }
                    ZZButton("Import configuration", icon: "square.and.arrow.down", variant: .warning) {
                        sheet = .importConfig
                    }
                }
            }
            GallerySection(
                "Attachments",
                detail:
                    "56-point composer tiles and 140-point transcript images share a native preview with zoom controls."
            ) {
                HStack(alignment: .top, spacing: 24) {
                    VStack(alignment: .leading, spacing: 12) {
                        Text("Composer").font(.system(size: 11)).foregroundStyle(theme.foreground.muted().color)
                        if attached {
                            ZZAttachmentThumbnail(
                                Image(systemName: "macwindow"), title: "Workspace reference",
                                open: { sheet = .attachment }, remove: { attached = false })
                        } else {
                            ZZButton("Attach again", icon: "paperclip") { attached = true }
                        }
                    }
                    VStack(alignment: .leading, spacing: 12) {
                        Text("Transcript").font(.system(size: 11)).foregroundStyle(theme.foreground.muted().color)
                        ZZAttachmentThumbnail(
                            Image(systemName: "macwindow"), title: "Workspace reference",
                            side: ZZAgentMetrics.transcriptAttachment, open: { sheet = .attachment })
                    }
                    Spacer()
                }
            }
        }
        .sheet(item: $sheet) { item in
            switch item {
            case .host:
                ZZAddHostPrompt(
                    host: $host, error: hostError,
                    submit: {
                        if host.contains(" ") {
                            hostError = "Enter a host name, SSH alias, or user@host."
                        } else {
                            complete("Added fixture host: \(host)")
                        }
                    }, cancel: cancel)
            case .secret:
                ZZSSHSecretPrompt(
                    "SSH authentication", question: "Enter the password for fabrico@archbox.",
                    secret: $secret,
                    submit: {
                        secret = ""
                        complete("Authentication prompt answered.")
                    },
                    cancel: {
                        secret = ""
                        cancel()
                    })
            case .confirm:
                ZZSSHConfirmPrompt(
                    "Trust this host?",
                    question:
                        "The host key for archbox is unknown.\nFingerprint: SHA256:gallery-example\nContinue connecting?",
                    confirm: { complete("Trusted the fixture host.") }, cancel: cancel)
            case .clear:
                ZZClearSiteDataPrompt(confirm: { complete("Cleared the fixture site data.") }, cancel: cancel)
            case .importConfig:
                ZZImportConfigurationPrompt(
                    description: "Copy the sample Ghostty and tmux settings into the gallery configuration.",
                    confirm: { complete("Imported the fixture configuration.") }, cancel: cancel)
            case .attachment:
                ZZAttachmentPreview(Image(systemName: "macwindow"), title: "Workspace reference") { sheet = nil }
            }
        }
    }

    private func complete(_ message: String) {
        result = message
        hostError = nil
        sheet = nil
    }
    private func cancel() {
        result = "Cancelled."
        hostError = nil
        sheet = nil
    }
}
