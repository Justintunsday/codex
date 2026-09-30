import SwiftUI

struct ActivityView: View {
    @EnvironmentObject private var store: WorkspaceStore
    @State private var tab = 0
    var body: some View {
        VStack(spacing: 0) {
            Picker("Activity", selection: $tab) { Text("Log").tag(0); Text("Terminal").tag(1); Text("Git").tag(2) }
                .pickerStyle(.segmented).padding(16)
            if tab == 0 {
                List(store.activity.reversed()) { entry in
                    VStack(alignment: .leading, spacing: 8) {
                        HStack { Text(entry.category.uppercased()).font(.caption.weight(.semibold)); Spacer(); Text(entry.date, style: .time).font(.caption).foregroundStyle(.secondary) }
                        Text(entry.text).font(.system(.subheadline, design: .monospaced)).textSelection(.enabled)
                    }.padding(.vertical, 8)
                }
            } else {
                Form {
                    Section(tab == 1 ? "Terminal / PTY" : "Git") {
                        Label("Backend unavailable", systemImage: "info.circle")
                        Text(tab == 1 ? "The installed runtime cannot launch a shell or PTY in the iOS app sandbox. A remote execution adapter is required." : "Git status, diff and commit require a native Git or remote adapter. File review is available in Files.")
                            .foregroundStyle(.secondary)
                    }
                    Section("Runtime capability") {
                        Text(tab == 1 ? (store.capabilities?.process ?? "Detecting…") : (store.capabilities?.gitCommit ?? "Detecting…"))
                            .font(Design.code)
                    }
                }
            }
        }
        .navigationTitle("Activity")
    }
}
