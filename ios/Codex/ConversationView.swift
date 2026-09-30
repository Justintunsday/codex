import SwiftUI

struct ConversationView: View {
    @EnvironmentObject private var store: WorkspaceStore
    @State private var prompt = ""

    var body: some View {
        ScrollViewReader { scroll in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 24) {
                    if store.session?.messages.isEmpty != false {
                        VStack(alignment: .leading, spacing: 16) {
                            Text("Build something.").font(Design.display)
                            Text("Ask a question, or import a project and work with its files.").foregroundStyle(.secondary)
                            Label(store.project?.name ?? "No project selected", systemImage: "folder").font(.subheadline)
                        }
                        .padding(.vertical, 32)
                    }
                    ForEach(Array((store.session?.messages ?? []).enumerated()), id: \.offset) { index, message in
                        VStack(alignment: .leading, spacing: 8) {
                            Text(message.role == "user" ? "YOU" : "CODEX")
                                .font(.caption.weight(.semibold)).tracking(1.5).foregroundStyle(.secondary)
                            Text(message.text).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                        }
                        .padding(message.role == "user" ? 16 : 0)
                        .background(message.role == "user" ? Design.accent.opacity(0.07) : .clear)
                        .clipShape(RoundedRectangle(cornerRadius: 12))
                        .id(index)
                    }
                    if !store.thinking.isEmpty {
                        DisclosureGroup("Reasoning summary") { Text(store.thinking).font(.subheadline).foregroundStyle(.secondary).textSelection(.enabled) }
                    }
                    if store.working {
                        HStack(spacing: 8) { ProgressView(); Text("Working…").foregroundStyle(.secondary) }
                            .accessibilityIdentifier("taskProgress")
                    }
                    Color.clear.frame(height: 1).id("end")
                }
                .padding(Design.gap)
                .frame(maxWidth: 800, alignment: .leading)
                .frame(maxWidth: .infinity)
            }
            .onChange(of: store.session?.messages.last?.text) { _ in scroll.scrollTo("end", anchor: .bottom) }
        }
        .background(Design.canvas)
        .navigationTitle(store.session?.title ?? "Codex")
        .navigationBarTitleDisplayMode(.inline)
        .safeAreaInset(edge: .bottom) {
            VStack(alignment: .leading, spacing: 8) {
                if let project = store.project { Label(project.name, systemImage: "folder").font(.caption).foregroundStyle(.secondary) }
                HStack(alignment: .bottom, spacing: 8) {
                    TextField("Ask Codex…", text: $prompt, axis: .vertical)
                        .lineLimit(1...6).textFieldStyle(.plain).padding(12)
                        .background(Design.canvas).clipShape(RoundedRectangle(cornerRadius: 12))
                        .accessibilityIdentifier("promptField")
                    Button {
                        if store.working { store.command("cancel") }
                        else if store.send(prompt) { prompt = "" }
                    } label: {
                        Image(systemName: store.working ? "stop.fill" : "arrow.up")
                            .font(.headline).frame(width: 44, height: 44)
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(!store.working && (prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || store.session == nil))
                    .accessibilityLabel(store.working ? "Cancel task" : "Send prompt")
                }
            }
            .padding(16).background(.regularMaterial)
        }
    }
}
