import SwiftUI

@main
struct ZZMobileApp: App {
    @UIApplicationDelegateAdaptor(ZZAppDelegate.self) private var appDelegate
    @Environment(\.scenePhase) private var scenePhase
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
                    store.setSceneActive(scenePhase == .active)
                }
                .onChange(of: scenePhase) { _, phase in
                    store.setSceneActive(phase == .active)
                }
                .onOpenURL { url in
                    store.open(url)
                }
        }
    }
}
