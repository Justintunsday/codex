import SwiftUI

enum WorkspacePage: String, CaseIterable, Identifiable {
    case task = "Task", files = "Files", activity = "Activity", settings = "Settings"
    var id: String { rawValue }
    var symbol: String {
        switch self {
        case .task: return "bubble.left.and.bubble.right"
        case .files: return "folder"
        case .activity: return "terminal"
        case .settings: return "slider.horizontal.3"
        }
    }
}

struct WorkspaceView: View {
    @EnvironmentObject private var store: WorkspaceStore
    @Environment(\.horizontalSizeClass) private var size
    @State private var page = WorkspacePage.task
    @State private var showProjects = false
    @State private var showSessions = false

    var body: some View {
        Group {
            if size == .regular {
                NavigationSplitView {
                    List {
                        Section("Workspace") {
                            ForEach(WorkspacePage.allCases) { item in
                                Button { page = item } label: { Label(item.rawValue, systemImage: item.symbol).padding(.vertical, 8) }
                                    .listRowBackground(page == item ? Design.accent.opacity(0.1) : nil)
                            }
                        }
                        Section("Projects") {
                            ForEach(store.projects) { project in
                                Button { store.selectProject(project); page = .files } label: { Label(project.name, systemImage: "folder") }
                                    .disabled(store.working)
                            }
                            Button { showProjects = true } label: { Label("Import project", systemImage: "plus") }
                                .disabled(store.working || store.importing)
                        }
                        Section("Sessions") {
                            Button { store.command("createSession"); page = .task } label: { Label("New session", systemImage: "square.and.pencil") }.disabled(store.working)
                            ForEach(store.sessions) { session in
                                Button { store.restore(session.id); page = .task } label: { Text(session.title).lineLimit(2) }.disabled(store.working)
                            }
                        }
                    }
                    .navigationTitle("Codex")
                } detail: { NavigationStack { content(page) } }
                .navigationSplitViewStyle(.balanced)
            } else {
                TabView(selection: $page) {
                    ForEach(WorkspacePage.allCases) { item in
                        NavigationStack { content(item) }
                            .tabItem { Label(item.rawValue, systemImage: item.symbol) }
                            .tag(item)
                    }
                }
            }
        }
        .sheet(isPresented: $showProjects) { ProjectPicker { url in showProjects = false; store.importProject(url) } }
        .sheet(isPresented: $showSessions) { sessionPicker }
        .sheet(item: $store.review) { ReviewView(review: $0).interactiveDismissDisabled() }
        .alert("Codex", isPresented: Binding(get: { store.error != nil }, set: { if !$0 { store.error = nil } })) {
            Button("OK") { store.error = nil }
        } message: { Text(store.error ?? "") }
    }

    @ViewBuilder
    private func content(_ page: WorkspacePage) -> some View {
        Group {
            switch page {
            case .task: ConversationView()
            case .files: FilesView()
            case .activity: ActivityView()
            case .settings: SettingsView()
            }
        }
        .toolbar {
            if page == .task {
                ToolbarItem(placement: .navigationBarLeading) {
                    Button { store.command("listSessions"); showSessions = true } label: { Image(systemName: "clock.arrow.circlepath").frame(width: 44, height: 44) }
                        .accessibilityLabel("Session history")
                }
                ToolbarItem(placement: .navigationBarTrailing) {
                    Button { store.command("createSession") } label: { Image(systemName: "square.and.pencil").frame(width: 44, height: 44) }
                        .disabled(store.working).accessibilityLabel("New session")
                }
            }
            if page == .files {
                ToolbarItem(placement: .navigationBarTrailing) {
                    Button { showProjects = true } label: { Image(systemName: "folder.badge.plus").frame(width: 44, height: 44) }
                        .disabled(store.working || store.importing).accessibilityLabel("Import project")
                }
            }
        }
    }

    private var sessionPicker: some View {
        NavigationStack {
            List(store.sessions) { session in
                Button { store.restore(session.id); showSessions = false } label: { Text(session.title).padding(.vertical, 8) }.disabled(store.working)
            }
            .navigationTitle("Session history")
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Done") { showSessions = false } } }
        }
    }
}
