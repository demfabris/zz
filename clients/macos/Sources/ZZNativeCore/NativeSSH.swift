import AppKit
import CZZClient

let nativeSSHCallback: zz_ssh_prompt_callback = { _, prompt, response, capacity in
    guard let prompt else { return ZZ_SSH_PROMPT_CANCEL }
    let title = NativeClient.string(prompt.pointee.title)
    let message = NativeClient.string(prompt.pointee.message)
    let kind = prompt.pointee.kind.rawValue
    let echo = prompt.pointee.echo
    let answer: (UInt32, String) = DispatchQueue.main.sync {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = message
        if kind == ZZ_SSH_PROMPT_HOST_KEY.rawValue {
            alert.addButton(withTitle: "Trust once")
            alert.addButton(withTitle: "Trust and save")
            alert.addButton(withTitle: "Cancel")
            let result = alert.runModal()
            if result == .alertFirstButtonReturn { return (ZZ_SSH_PROMPT_TRUST_ONCE.rawValue, "") }
            if result == .alertSecondButtonReturn { return (ZZ_SSH_PROMPT_TRUST_AND_SAVE.rawValue, "") }
            return (ZZ_SSH_PROMPT_CANCEL.rawValue, "")
        }
        let field: NSTextField =
            echo
            ? NSTextField(frame: CGRect(x: 0, y: 0, width: 320, height: 24))
            : NSSecureTextField(frame: CGRect(x: 0, y: 0, width: 320, height: 24))
        if kind == ZZ_SSH_PROMPT_SECRET.rawValue { alert.accessoryView = field }
        alert.addButton(withTitle: kind == ZZ_SSH_PROMPT_SECRET.rawValue ? "Connect" : "Continue")
        alert.addButton(withTitle: "Cancel")
        alert.window.initialFirstResponder = kind == ZZ_SSH_PROMPT_SECRET.rawValue ? field : alert.buttons.first
        guard alert.runModal() == .alertFirstButtonReturn else { return (ZZ_SSH_PROMPT_CANCEL.rawValue, "") }
        return (ZZ_SSH_PROMPT_ANSWER.rawValue, kind == ZZ_SSH_PROMPT_SECRET.rawValue ? field.stringValue : "yes")
    }
    if answer.0 == ZZ_SSH_PROMPT_ANSWER.rawValue {
        let bytes = Array(answer.1.utf8)
        guard let response, bytes.count < capacity else { return ZZ_SSH_PROMPT_CANCEL }
        for (index, byte) in bytes.enumerated() { response[index] = CChar(bitPattern: byte) }
        response[bytes.count] = 0
    }
    return zz_ssh_prompt_reply(rawValue: answer.0)
}
