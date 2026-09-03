import SwiftUI
import UIKit

/// Multiline prompt field with the desktop's key contract: Return submits,
/// Shift-Return inserts a newline, Command-Return submits from anywhere in the
/// field. `crates/zz-ui/src/widget/input/state.rs` carries the same rule for
/// the GPUI composer.
///
/// SwiftUI's `TextField` cannot express it: with `axis: .vertical` a hardware
/// Return always reaches `onSubmit`, so a prompt can never hold a second line.
struct AgentPromptEditor: UIViewRepresentable {
    @Binding var text: String
    @Binding var height: CGFloat
    let enabled: Bool
    /// Bumped to ask the field to take focus. A one-shot token rather than a
    /// two-way binding, which would loop against the first-responder state.
    let focusRequest: Int
    let submit: () -> Void

    /// Clamped the way the previous `TextField(axis:)` was.
    static let minimumLines = 1
    static let maximumLines = 6

    func makeUIView(context: Context) -> AgentPromptTextView {
        let view = AgentPromptTextView()
        view.delegate = context.coordinator
        view.onSubmit = { submit() }
        view.font = UIFont.preferredFont(forTextStyle: .body)
        view.adjustsFontForContentSizeCategory = true
        view.backgroundColor = .clear
        view.textContainerInset = UIEdgeInsets(top: 10, left: 8, bottom: 10, right: 8)
        view.textContainer.lineFragmentPadding = 0
        view.isScrollEnabled = false
        view.setContentHuggingPriority(.defaultHigh, for: .vertical)
        view.setContentCompressionResistancePriority(.required, for: .vertical)
        return view
    }

    func updateUIView(_ view: AgentPromptTextView, context: Context) {
        // The coordinator outlives every render, so it must be handed the
        // current view value or its bindings and closures go stale.
        context.coordinator.parent = self
        if view.text != text {
            view.text = text
        }
        view.isEditable = enabled
        view.onSubmit = { submit() }
        if focusRequest != context.coordinator.servedFocusRequest {
            context.coordinator.servedFocusRequest = focusRequest
            if focusRequest > 0, !view.isFirstResponder {
                view.becomeFirstResponder()
            }
        }
        recalculateHeight(view)
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(self)
    }

    private func recalculateHeight(_ view: AgentPromptTextView) {
        let lineHeight = view.font?.lineHeight ?? 20
        let insets = view.textContainerInset.top + view.textContainerInset.bottom
        let fitted = view.sizeThatFits(
            CGSize(width: view.bounds.width, height: .greatestFiniteMagnitude)
        ).height
        let minimum = lineHeight * CGFloat(Self.minimumLines) + insets
        let maximum = lineHeight * CGFloat(Self.maximumLines) + insets
        let resolved = min(max(fitted, minimum), maximum)
        // Past the cap the field stops growing and scrolls instead.
        view.isScrollEnabled = fitted > maximum
        guard abs(resolved - height) > 0.5 else {
            return
        }
        Task { @MainActor in
            height = resolved
        }
    }

    @MainActor
    final class Coordinator: NSObject, UITextViewDelegate {
        var parent: AgentPromptEditor
        var servedFocusRequest = 0

        init(_ parent: AgentPromptEditor) {
            self.parent = parent
            servedFocusRequest = parent.focusRequest
        }

        func textViewDidChange(_ textView: UITextView) {
            parent.text = textView.text
            if let view = textView as? AgentPromptTextView {
                parent.recalculateHeight(view)
            }
        }

        func textView(
            _ textView: UITextView,
            shouldChangeTextIn range: NSRange,
            replacementText text: String
        ) -> Bool {
            guard text == "\n" else {
                return true
            }
            // An open IME composition owns Return; confirming it must not send.
            guard textView.markedTextRange == nil else {
                return true
            }
            parent.submit()
            return false
        }
    }
}

/// Shift-Return and Command-Return arrive as key commands so the delegate only
/// ever sees a bare Return, which it treats as submit.
final class AgentPromptTextView: UITextView {
    var onSubmit: (() -> Void)?

    override var keyCommands: [UIKeyCommand]? {
        let newline = UIKeyCommand(
            input: "\r",
            modifierFlags: .shift,
            action: #selector(insertNewlineFromShortcut)
        )
        let send = UIKeyCommand(
            input: "\r",
            modifierFlags: .command,
            action: #selector(submitFromShortcut)
        )
        newline.wantsPriorityOverSystemBehavior = true
        send.wantsPriorityOverSystemBehavior = true
        return [newline, send]
    }

    @objc private func insertNewlineFromShortcut() {
        insertText("\n")
    }

    @objc private func submitFromShortcut() {
        onSubmit?()
    }
}
