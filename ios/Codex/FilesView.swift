import SwiftUI

struct FilesView: View {
    @EnvironmentObject private var store: WorkspaceStore
    @State private var showProjectList = false
    var body: some View {
        List {
            Section {
                Button { showProjectList = true } label: { Label(store.project?.name ?? "Select an imported project", systemImage: "folder") }.disabled(store.working)
                if store.importing { ProgressView("Importing project…") }
                if store.project != nil {
                    Text("Working on an imported copy. Share edited files to export your changes.").font(.caption).foregroundStyle(.secondary)
                    if !store.folder.isEmpty {
                        Button { store.browse((store.folder as NSString).deletingLastPathComponent) } label: { Label("Parent directory", systemImage: "arrow.up") }
                        Text(store.folder).font(.caption.monospaced()).textSelection(.enabled)
                    }
                }
            }
            Section("Files") {
                ForEach(store.files) { file in
                    if file.isDirectory {
                        Button { store.browse(file.path) } label: { Label(file.name, systemImage: "folder").padding(.vertical, 8) }
                    } else {
                        NavigationLink { FileEditorView(path: file.path) } label: { Label(file.name, systemImage: "doc.text").padding(.vertical, 8) }
                    }
                }
                if store.project == nil { Text("Import a folder with the button above.").foregroundStyle(.secondary) }
            }
        }
        .navigationTitle("Files")
        .refreshable { store.browse(store.folder) }
        .sheet(isPresented: $showProjectList) {
            NavigationStack {
                List(store.projects) { project in
                    Button { store.selectProject(project); showProjectList = false } label: { Label(project.name, systemImage: "folder") }
                }
                .navigationTitle("Projects")
                .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Done") { showProjectList = false } } }
            }
        }
    }
}

struct FileEditorView: View {
    @EnvironmentObject private var store: WorkspaceStore
    var path: String
    @State private var text = ""
    @State private var editing = false

    var body: some View {
        Group {
            if store.file?.path == path {
                if editing {
                    TextEditor(text: $text).font(Design.code).padding(8)
                        .autocorrectionDisabled().textInputAutocapitalization(.never)
                        .accessibilityIdentifier("fileEditor")
                } else {
                    ScrollView([.horizontal, .vertical]) { Text(store.file?.text ?? "").font(Design.code).textSelection(.enabled).padding(16).frame(maxWidth: .infinity, alignment: .leading) }
                }
            } else { ProgressView("Reading file…") }
        }
        .navigationTitle((path as NSString).lastPathComponent)
        .navigationBarTitleDisplayMode(.inline)
        .onAppear { store.file = nil; store.command("readFile", ["path": path]) }
        .toolbar {
            ToolbarItemGroup(placement: .navigationBarTrailing) {
                if editing {
                    Button("Review") { store.command("previewChange", ["path": path, "after": text]); editing = false }.disabled(store.working)
                } else {
                    Button("Edit") { text = store.file?.text ?? ""; editing = true }.disabled(store.file?.path != path || store.working)
                }
                if let root = store.projectURL { ShareLink(item: root.appendingPathComponent(path)) { Image(systemName: "square.and.arrow.up") }.accessibilityLabel("Export file") }
            }
        }
    }
}

struct ReviewView: View {
    @EnvironmentObject private var store: WorkspaceStore
    let review: ChangeReview
    @State private var comparison = 0

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                Picker("Comparison", selection: $comparison) {
                    Text("Diff").tag(0); Text("Before").tag(1); Text("After").tag(2)
                }.pickerStyle(.segmented).padding(16)
                ScrollView([.horizontal, .vertical]) {
                    if comparison == 0 {
                        VStack(alignment: .leading, spacing: 2) {
                            ForEach(Array(review.change.diff.components(separatedBy: "\n").enumerated()), id: \.offset) { _, line in
                                Text(line.isEmpty ? " " : line).font(Design.code).foregroundStyle(color(line))
                            }
                        }.padding(16).frame(maxWidth: .infinity, alignment: .leading)
                    } else { Text(comparison == 1 ? review.change.before : review.change.after).font(Design.code).textSelection(.enabled).padding(16) }
                }
            }
            .navigationTitle(review.change.path)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Reject") { store.command("review", ["id": review.id, "decision": "reject"]) } }
                ToolbarItem(placement: .confirmationAction) { Button("Save change") { store.command("review", ["id": review.id, "decision": "approve"]) }.accessibilityIdentifier("approveChange") }
            }
        }
    }

    private func color(_ line: String) -> Color {
        if line.hasPrefix("+") { return .green }
        if line.hasPrefix("-") { return .red }
        return .primary
    }
}
