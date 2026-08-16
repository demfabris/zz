import SwiftUI

@main
struct ZZMobileApp: App {
    @Environment(\.scenePhase) private var scenePhase
    @StateObject private var store = ZZStore()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(store)
                .preferredColorScheme(.dark)
                .onAppear {
                    store.setSceneActive(scenePhase == .active)
                }
                .onChange(of: scenePhase) { _, phase in
                    store.setSceneActive(phase == .active)
                }
        }
    }
}
