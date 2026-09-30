import SwiftUI

struct GitView: View {
    @EnvironmentObject private var store: WorkspaceStore
    @State private var name = ""
    @State private var email = ""
    @State private var message = ""
    @State private var confirm = false
    @State private var proposed: GitReport?
    @FocusState private var editing: Bool

    var body: some View {
        Form {
            Section("Repository") {
                Label(store.project?.name ?? "Select a project in Files", systemImage: "folder")
                if let git = store.git { Text(git.head).font(Design.code) }
                Button("Refresh Git status") { store.command("gitStatus") }.disabled(store.project == nil || store.working)
                if store.git == nil {
                    Button("Initialize Git in this project") { store.command("gitInit") }
                        .accessibilityIdentifier("gitInitialize").disabled(store.project == nil || store.working)
                }
                Text("Git works on the imported copy. Network operations, merge conflicts, submodules and repository filters require another backend.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            if let git = store.git {
                Section("Changed files") {
                    ForEach(git.changes) { file in
                        NavigationLink { GitDiffView(file: file) } label: {
                            HStack(spacing: 12) {
                                Text("\(file.index.isEmpty ? "·" : file.index) \(file.working.isEmpty ? "·" : file.working)").font(Design.code)
                                Text(file.path).lineLimit(2)
                            }.padding(.vertical, 8)
                        }.accessibilityIdentifier("gitFile_\(file.path)").disabled(store.working)
                    }
                    if git.changes.isEmpty { Text("Working tree is clean").foregroundStyle(.secondary) }
                    Text("Left: staged · Right: working tree · ?: untracked").font(.caption).foregroundStyle(.secondary)
                }
                Section("Commit staged changes") {
                    TextField("Author name", text: $name).focused($editing).accessibilityIdentifier("gitAuthorName")
                    TextField("Author email", text: $email).keyboardType(.emailAddress).textInputAutocapitalization(.never).autocorrectionDisabled()
                        .focused($editing).accessibilityIdentifier("gitAuthorEmail")
                    TextField("Commit message", text: $message, axis: .vertical).lineLimit(2...6).focused($editing).accessibilityIdentifier("gitCommitMessage")
                    Button("Review commit") { proposed = git; confirm = true }
                        .accessibilityIdentifier("gitReviewCommit")
                        .disabled(store.working || !git.changes.contains(where: { !$0.index.isEmpty }) || name.isEmpty || email.isEmpty || message.isEmpty)
                    Text("Commits contain staged content, are unsigned and skip repository hooks. Working files are preserved.")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
        }
        .scrollDismissesKeyboard(.interactively)
        .toolbar { ToolbarItemGroup(placement: .keyboard) { Spacer(); Button("Done") { editing = false } } }
        .confirmationDialog("Commit the staged changes shown above?", isPresented: $confirm, titleVisibility: .visible) {
            Button("Create commit") {
                if let git = proposed {
                    store.command("gitCommit", ["request": ["expectedHead": git.headId, "expectedIndex": git.indexId,
                        "name": name, "email": email, "message": message]])
                }
            }
            Button("Cancel", role: .cancel) {}
        }
    }
}

private struct GitDiffView: View {
    @EnvironmentObject private var store: WorkspaceStore
    @Environment(\.dismiss) private var dismiss
    let file: GitChange
    @State private var layer = "working"

    var body: some View {
        VStack(spacing: 0) {
            Picker("Git diff", selection: $layer) {
                Text("Working tree").tag("working")
                Text("Staged").tag("index")
            }.pickerStyle(.segmented).padding(16)
            if let diff = store.gitDiff, diff.path == file.path, diff.layer == layer {
                ScrollView([.horizontal, .vertical]) {
                    Text(diff.diff.isEmpty ? "No text changes" : diff.diff).font(Design.code).textSelection(.enabled)
                        .accessibilityIdentifier("gitDiffText")
                        .padding(16).frame(maxWidth: .infinity, alignment: .leading)
                }
                if layer == "working" && !file.working.isEmpty {
                    Button("Stage reviewed file") {
                        store.command("gitStage", ["path": file.path, "expectedCurrent": diff.currentId])
                        dismiss()
                    }.accessibilityIdentifier("gitStageFile").buttonStyle(.borderedProminent).controlSize(.large).padding(16).disabled(store.working)
                }
            } else { ProgressView("Reading diff…").frame(maxWidth: .infinity, maxHeight: .infinity) }
        }
        .navigationTitle(file.path).navigationBarTitleDisplayMode(.inline)
        .onAppear { layer = file.working.isEmpty ? "index" : "working"; load() }
        .onChange(of: layer) { _ in load() }
    }

    private func load() {
        store.gitDiff = nil
        store.command("gitDiff", ["path": file.path, "layer": layer])
    }
}
