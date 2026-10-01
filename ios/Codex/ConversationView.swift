import SwiftUI

struct ConversationView: View {
    @EnvironmentObject private var store: WorkspaceStore
    var openSettings: () -> Void = {}
    @State private var prompt = ""

    var body: some View {
        ScrollViewReader { scroll in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: Design.sectionGap) {
                    if store.session?.messages.isEmpty != false {
                        newTask
                    }
                    ForEach(Array((store.session?.messages ?? []).enumerated()), id: \.offset) { index, message in
                        VStack(alignment: .leading, spacing: 8) {
                            Text(message.role == "user" ? "YOU" : "CODEX")
                                .font(.caption.weight(.semibold)).tracking(1.5).foregroundStyle(.secondary)
                            Text(message.text).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                        }
                        .padding(message.role == "user" ? 16 : 0)
                        .background(message.role == "user" ? Design.surface : .clear)
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
                .frame(maxWidth: 760, alignment: .leading)
                .frame(maxWidth: .infinity)
            }
            .onChange(of: store.session?.messages.last?.text) { _ in scroll.scrollTo("end", anchor: .bottom) }
        }
        .background(Design.canvas)
        .navigationTitle(store.session?.title ?? "Codex")
        .navigationBarTitleDisplayMode(.inline)
        .safeAreaInset(edge: .bottom) {
            VStack(alignment: .leading, spacing: 8) {
                ViewThatFits(in: .horizontal) {
                    HStack {
                        modelSelection
                        Spacer(minLength: 16)
                        runtimeStatus
                    }
                    VStack(alignment: .leading, spacing: 8) { modelSelection; runtimeStatus }
                }
                HStack(alignment: .bottom, spacing: 8) {
                    TextField("Ask Codex…", text: $prompt, axis: .vertical)
                        .lineLimit(1...6).textFieldStyle(.plain).padding(16)
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
                    .padding(.trailing, 8).padding(.bottom, 8)
                }
                .background(Design.surface)
                .clipShape(RoundedRectangle(cornerRadius: Design.corner))
                .overlay(RoundedRectangle(cornerRadius: Design.corner).strokeBorder(Design.divider, lineWidth: 1))
            }
            .padding(16).background(Design.canvas)
        }
    }

    private var newTask: some View {
        VStack(alignment: .leading, spacing: Design.sectionGap) {
            VStack(alignment: .leading, spacing: 16) {
                Label("CODEX WORKSPACE", systemImage: "chevron.left.forwardslash.chevron.right")
                    .font(.caption.weight(.semibold)).tracking(1.5).foregroundStyle(Design.accent)
                Text("New task").font(Design.display)
                Text("Describe a change, ask for a review, or work through a problem.")
                    .foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            Divider()
            VStack(alignment: .leading, spacing: 8) {
                Text("PROJECT").font(.caption.weight(.semibold)).tracking(1.5).foregroundStyle(.secondary)
                Label(store.project?.name ?? "No project selected", systemImage: "folder")
                    .font(.headline).fixedSize(horizontal: false, vertical: true)
                Text(store.project == nil ? "Import a folder in Files to browse and review changes." : "File changes are reviewed before they are saved.")
                    .font(.subheadline).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            if store.sessions.contains(where: { $0.id != store.session?.id }) {
                VStack(alignment: .leading, spacing: 8) {
                    Text("RECENT SESSIONS").font(.caption.weight(.semibold)).tracking(1.5).foregroundStyle(.secondary)
                    ForEach(Array(store.sessions.filter { $0.id != store.session?.id }.prefix(3))) { session in
                        Button { store.restore(session.id) } label: {
                            HStack(spacing: 12) {
                                Image(systemName: "clock.arrow.circlepath")
                                Text(session.title).lineLimit(2).frame(maxWidth: .infinity, alignment: .leading)
                                Image(systemName: "chevron.right").font(.caption)
                            }.frame(minHeight: 44)
                        }.disabled(store.working)
                    }
                }
            }
        }.padding(.top, 24).padding(.bottom, 16)
    }

    private var modelSelection: some View {
        Button(action: openSettings) {
            Label(store.model.isEmpty ? "Connection settings" : store.model, systemImage: "slider.horizontal.3")
                .font(.caption.weight(.medium)).lineLimit(2).frame(minHeight: 44)
        }.disabled(store.working).accessibilityIdentifier("modelConnection")
    }

    private var runtimeStatus: some View {
        Label(store.status == "starting" ? "Starting" : store.working ? "Working" : "Ready",
            systemImage: store.working ? "circle.dotted" : "circle")
            .font(.caption.monospaced()).foregroundStyle(.secondary)
    }
}
