import SwiftUI

@main
struct CodexApp: App {
    @StateObject private var store = WorkspaceStore()
    @Environment(\.scenePhase) private var phase

    private var testAppearance: ColorScheme? {
        #if DEBUG
        if ProcessInfo.processInfo.environment["CODEX_UI_TEST_APPEARANCE"] == "dark" { return .dark }
        #endif
        return nil
    }

    var body: some Scene {
        WindowGroup {
            WorkspaceView()
                .environmentObject(store)
                .tint(Design.accent)
                .preferredColorScheme(testAppearance)
                .onChange(of: phase) { phase in
                    if phase == .background { store.lifecycle("background") }
                    if phase == .active { store.lifecycle("foreground") }
                }
                .onReceive(NotificationCenter.default.publisher(for: UIApplication.didReceiveMemoryWarningNotification)) { _ in store.lifecycle("memoryPressure") }
        }
    }
}
