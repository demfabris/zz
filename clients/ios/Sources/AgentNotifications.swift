import UIKit
import UserNotifications

extension Notification.Name {
    static let zzNotificationRoute = Notification.Name("zz.notification-route")
    static let zzShortcutCommand = Notification.Name("zz.shortcut-command")
}

final class ZZAppDelegate: NSObject, UIApplicationDelegate, UNUserNotificationCenterDelegate {
    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        UNUserNotificationCenter.current().delegate = self
        return true
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification
    ) async -> UNNotificationPresentationOptions {
        [.banner, .sound]
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse
    ) async {
        let info = response.notification.request.content.userInfo
        guard let session = info["session"] as? NSNumber,
              let pane = info["pane"] as? NSNumber else {
            return
        }
        await MainActor.run {
            NotificationCenter.default.post(
                name: .zzNotificationRoute,
                object: nil,
                userInfo: [
                    "session": session.uint64Value,
                    "pane": pane.uint64Value,
                ]
            )
        }
    }
}

@MainActor
final class ZZAgentNotifications {
    private var requestedAuthorization = false

    func schedule(
        kind: ZZAgentAttentionKind,
        pane: UInt64,
        session: UInt64,
        title: String,
        permission: UInt64?
    ) {
        Task {
            let center = UNUserNotificationCenter.current()
            if !requestedAuthorization {
                requestedAuthorization = true
                _ = try? await center.requestAuthorization(options: [.alert, .sound])
            }
            let settings = await center.notificationSettings()
            guard settings.authorizationStatus == .authorized
                    || settings.authorizationStatus == .provisional else {
                return
            }
            let content = UNMutableNotificationContent()
            content.title = title
            switch kind {
            case .blocked:
                content.body = "An Agent needs your approval."
            case .done:
                content.body = "An Agent finished its work."
            case .failed:
                content.body = "An Agent stopped with an error."
            case .working:
                return
            }
            content.sound = .default
            content.userInfo = ["session": session, "pane": pane]
            let identity = permission.map(String.init) ?? kind.label
            let request = UNNotificationRequest(
                identifier: "zz.agent.\(pane).\(identity)",
                content: content,
                trigger: nil
            )
            try? await center.add(request)
        }
    }

    func clear(pane: UInt64) {
        Task {
            let center = UNUserNotificationCenter.current()
            let pending = await center.pendingNotificationRequests()
                .filter { ($0.content.userInfo["pane"] as? NSNumber)?.uint64Value == pane }
                .map(\.identifier)
            let delivered = await center.deliveredNotifications()
                .filter { ($0.request.content.userInfo["pane"] as? NSNumber)?.uint64Value == pane }
                .map(\.request.identifier)
            center.removePendingNotificationRequests(withIdentifiers: pending)
            center.removeDeliveredNotifications(withIdentifiers: delivered)
        }
    }
}
