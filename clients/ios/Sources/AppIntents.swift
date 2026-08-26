import AppIntents
import Foundation

enum ZZShortcutCommand: String {
    case reconnect
    case attention

    static let key = "zz.shortcut-command"

    func enqueue() {
        UserDefaults.standard.set(rawValue, forKey: Self.key)
        NotificationCenter.default.post(name: .zzShortcutCommand, object: nil)
    }
}

struct OpenZZIntent: AppIntent {
    static let title: LocalizedStringResource = "Open zz"
    static let description = IntentDescription("Open your zz sessions.")
    static var supportedModes: IntentModes { .foreground(.immediate) }

    func perform() async throws -> some IntentResult {
        .result()
    }
}

struct ReconnectZZIntent: AppIntent {
    static let title: LocalizedStringResource = "Reconnect zz"
    static let description = IntentDescription("Open zz and reconnect to the saved host.")
    static var supportedModes: IntentModes { .foreground(.immediate) }

    func perform() async throws -> some IntentResult {
        ZZShortcutCommand.reconnect.enqueue()
        return .result()
    }
}

struct OpenZZAttentionIntent: AppIntent {
    static let title: LocalizedStringResource = "Open zz Agent Attention"
    static let description = IntentDescription("Open the Agent that most needs your attention.")
    static var supportedModes: IntentModes { .foreground(.immediate) }

    func perform() async throws -> some IntentResult {
        ZZShortcutCommand.attention.enqueue()
        return .result()
    }
}

struct ZZAppShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(
            intent: OpenZZIntent(),
            phrases: ["Open \(.applicationName)"],
            shortTitle: "Open zz",
            systemImageName: "terminal"
        )
        AppShortcut(
            intent: ReconnectZZIntent(),
            phrases: ["Reconnect \(.applicationName)"],
            shortTitle: "Reconnect",
            systemImageName: "arrow.clockwise"
        )
        AppShortcut(
            intent: OpenZZAttentionIntent(),
            phrases: ["Show \(.applicationName) Agent attention"],
            shortTitle: "Agent Attention",
            systemImageName: "sparkles"
        )
    }
}
