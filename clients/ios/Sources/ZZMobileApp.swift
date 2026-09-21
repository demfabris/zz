import SwiftUI

@main
struct ZZMobileApp: App {
    @UIApplicationDelegateAdaptor(ZZAppDelegate.self) private var appDelegate
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.colorScheme) private var colorScheme
    @StateObject private var store = ZZStore()
    @State private var settings = ZZClientSettings()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(store)
                .environment(settings)
                .environment(\.zzTerminalPresentation, settings.terminalPresentation)
                .preferredColorScheme(settings.appearance.colorScheme)
                .onAppear {
                    store.settings = settings
                    settings.shared?.dark = (settings.appearance.colorScheme ?? colorScheme) == .dark
                    store.setSceneActive(scenePhase == .active)
                }
                .onChange(of: scenePhase) { _, phase in
                    store.setSceneActive(phase == .active)
                }
                .onChange(of: settings.appearance) {
                    settings.shared?.dark = (settings.appearance.colorScheme ?? colorScheme) == .dark
                    store.refreshTerminalPreferences()
                }
                .onChange(of: colorScheme) {
                    settings.shared?.dark = (settings.appearance.colorScheme ?? colorScheme) == .dark
                    store.refreshTerminalPreferences()
                }
                .onChange(of: settings.shared?.revision) { store.refreshTerminalPreferences() }
                .onOpenURL { url in
                    store.open(url)
                }
        }
    }
}
