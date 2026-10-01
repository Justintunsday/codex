import SwiftUI

struct ActivityView: View {
    @EnvironmentObject private var store: WorkspaceStore
    @State private var tab = 0
    var body: some View {
        VStack(spacing: 0) {
            Picker("Activity", selection: $tab) { Text("Log").tag(0); Text("Terminal").tag(1); Text("Git").tag(2) }
                .pickerStyle(.segmented).padding(16).accessibilityIdentifier("activitySelection")
            if tab == 0 {
                List(store.activity.reversed()) { entry in
                    VStack(alignment: .leading, spacing: 8) {
                        HStack { Text(entry.category.uppercased()).font(.caption.weight(.semibold)); Spacer(); Text(entry.date, style: .time).font(.caption).foregroundStyle(.secondary) }
                        Text(entry.text).font(.system(.subheadline, design: .monospaced)).textSelection(.enabled)
                    }.padding(.vertical, 8)
                }
                .scrollContentBackground(.hidden).background(Design.canvas)
                .overlay {
                    if store.activity.isEmpty {
                        VStack(alignment: .leading, spacing: 16) {
                            Label("Activity", systemImage: "text.alignleft").font(.title2.weight(.semibold))
                            Text("Task progress, tools and errors appear here.").foregroundStyle(.secondary)
                        }.padding(24)
                    }
                }
            } else if tab == 2 {
                GitView()
            } else {
                TerminalPanel()
            }
        }
        .background(Design.canvas)
        .navigationTitle("Activity")
    }
}
