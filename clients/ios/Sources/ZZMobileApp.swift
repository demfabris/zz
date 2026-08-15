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
                .onChange(of: scenePhase) { _, phase in
                    if phase == .active {
                        store.start()
                    } else if phase == .background {
                        store.stop()
                    }
                }
        }
    }
}
