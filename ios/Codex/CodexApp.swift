import SwiftUI

@main
struct CodexApp: App {
    @StateObject private var store = WorkspaceStore()
    @Environment(\.scenePhase) private var phase

    var body: some Scene {
        WindowGroup {
            WorkspaceView()
                .environmentObject(store)
                .tint(Design.accent)
                .onChange(of: phase) { phase in
                    if phase == .background { store.lifecycle("background") }
                    if phase == .active { store.lifecycle("foreground") }
                }
                .onReceive(NotificationCenter.default.publisher(for: UIApplication.didReceiveMemoryWarningNotification)) { _ in store.lifecycle("memoryPressure") }
        }
    }
}
