import AppKit
import CZZClient
import ZZNativeCore

final class NativeChrome {
    private var handle = zz_chrome_keymap_new()
    private var revision: UInt64?
    @MainActor func configure(_ settings: NativeSettings) {
        guard revision != settings.snapshot?.revision else { return }
        revision = settings.snapshot?.revision
        zz_chrome_keymap_free(handle)
        handle = settings.makeChromeKeymap()
    }
    deinit { zz_chrome_keymap_free(handle) }

    func resolve(_ event: NSEvent, table: String) -> String? {
        let value = table.withCString { table in
            (event.characters ?? "").withCString { text in
                zz_chrome_keymap_resolve(
                    handle, table, TerminalKey.code(event),
                    event.charactersIgnoringModifiers?.unicodeScalars.first?.value ?? 0,
                    TerminalKey.function(event), event.isARepeat ? 1 : 0,
                    TerminalKey.modifiers(event.modifierFlags), text)
            }
        }
        guard let bytes = value.ptr, value.len > 0 else { return nil }
        return String(decoding: UnsafeBufferPointer(start: bytes, count: value.len), as: UTF8.self)
    }
}

extension NativeBrowserEngine {
    func performChrome(_ action: String, pane: NativeBrowserPane, editingText: Bool) -> Bool {
        guard let tab = pane.activeTab else { return false }
        let edits = [
            "browser-undo": "Undo", "browser-redo": "Redo", "browser-cut": "Cut", "browser-copy": "Copy",
            "browser-paste": "Paste", "browser-paste-and-match-style": "PasteAndMatchStyle",
            "browser-select-all": "SelectAll",
        ]
        if let command = edits[action] {
            guard !editingText else { return false }
            self.action(tab, "edit", ["command": command])
            return true
        }
        let commands = [
            "browser-back": "back", "browser-forward": "forward", "browser-reload": "reload",
            "browser-zoom-in": "zoom-in", "browser-zoom-out": "zoom-out", "browser-zoom-reset": "zoom-reset",
            "browser-devtools": "devtools",
        ]
        if let command = commands[action] { self.action(tab, command); return true }
        switch action {
        case "browser-focus-address": pane.addressFocus += 1
        case "browser-new-tab": newTab(pane)
        case "browser-next-tab", "browser-previous-tab":
            let index = pane.tabs.firstIndex(where: { $0.id == pane.selection }) ?? 0
            let delta = action == "browser-next-tab" ? 1 : -1
            select(pane, tab: pane.tabs[(index + delta + pane.tabs.count) % pane.tabs.count])
        case "browser-select-last-tab": if let last = pane.tabs.last { select(pane, tab: last) }
        case "browser-element-selector": pane.pickRequest += 1
        default:
            if let index = Int(action.replacingOccurrences(of: "browser-select-tab-", with: "")),
                pane.tabs.indices.contains(index - 1)
            {
                select(pane, tab: pane.tabs[index - 1])
            } else {
                return false
            }
        }
        return true
    }
}
