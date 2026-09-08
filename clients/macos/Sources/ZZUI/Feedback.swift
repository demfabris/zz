import SwiftUI

public struct ZZAddHostPrompt: View {
    @Environment(\.zzTheme) private var theme
    @Binding private var host: String
    private let error: String?
    private let submit: () -> Void
    private let cancel: () -> Void

    public init(host: Binding<String>, error: String? = nil, submit: @escaping () -> Void, cancel: @escaping () -> Void)
    {
        _host = host
        self.error = error
        self.submit = submit
        self.cancel = cancel
    }

    public var body: some View {
        ZZDialog("Add host", description: "Connects over SSH · your ~/.ssh/config still applies.") {
            VStack(alignment: .leading, spacing: 8) {
                ZZTextField("Host", text: $host, placeholder: "user@hostname", invalid: error != nil).onSubmit(submit)
                if let error { Text(error).font(theme.font(size: 11)).foregroundStyle(theme.warning.color) }
            }
        } actions: {
            ZZButton("Cancel", action: cancel).keyboardShortcut(.cancelAction)
            ZZButton("Add", variant: .primary, action: submit)
                .disabled(host.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty).keyboardShortcut(.defaultAction)
        }
    }
}

public struct ZZSSHSecretPrompt: View {
    @Binding private var secret: String
    private let title: String
    private let question: String
    private let submit: () -> Void
    private let cancel: () -> Void

    public init(
        _ title: String, question: String, secret: Binding<String>, submit: @escaping () -> Void,
        cancel: @escaping () -> Void
    ) {
        self.title = title
        self.question = question
        _secret = secret
        self.submit = submit
        self.cancel = cancel
    }

    public var body: some View {
        ZZDialog(title, description: question) {
            ZZTextField("Secret", text: $secret, contentType: .password).onSubmit(submit)
        } actions: {
            ZZButton("Cancel", action: cancel).keyboardShortcut(.cancelAction)
            ZZButton("Continue", variant: .primary, action: submit).keyboardShortcut(.defaultAction)
        }
    }
}

public struct ZZSSHConfirmPrompt: View {
    private let title: String
    private let question: String
    private let confirm: () -> Void
    private let cancel: () -> Void

    public init(_ title: String, question: String, confirm: @escaping () -> Void, cancel: @escaping () -> Void) {
        self.title = title
        self.question = question
        self.confirm = confirm
        self.cancel = cancel
    }

    public var body: some View {
        ZZDialog(title, description: question) {
            ZZButton("No", action: cancel).keyboardShortcut(.cancelAction)
            ZZButton("Yes, connect", variant: .warning, action: confirm).keyboardShortcut(.defaultAction)
        }
    }
}

public struct ZZClearSiteDataPrompt: View {
    private let confirm: () -> Void
    private let cancel: () -> Void
    public init(confirm: @escaping () -> Void, cancel: @escaping () -> Void) {
        self.confirm = confirm
        self.cancel = cancel
    }
    public var body: some View {
        ZZDialog(
            "Clear site data?",
            description:
                "This clears cookies and persistent storage owned by the current site. You may be signed out. This cannot be undone."
        ) {
            ZZButton("Cancel", action: cancel).keyboardShortcut(.cancelAction)
            ZZButton("Clear data", variant: .danger, action: confirm)
        }
    }
}

public struct ZZImportConfigurationPrompt: View {
    private let title: String
    private let description: String
    private let confirm: () -> Void
    private let cancel: () -> Void

    public init(
        _ title: String = "Import from Ghostty and tmux?", description: String,
        confirm: @escaping () -> Void, cancel: @escaping () -> Void
    ) {
        self.title = title
        self.description = description
        self.confirm = confirm
        self.cancel = cancel
    }

    public var body: some View {
        ZZDialog(title, description: description) {
            ZZButton("Cancel", action: cancel).keyboardShortcut(.cancelAction)
            ZZButton("Import", variant: .warning, action: confirm).keyboardShortcut(.defaultAction)
        }
    }
}
