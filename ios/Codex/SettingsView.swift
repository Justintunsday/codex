import SwiftUI

struct SettingsView: View {
    @EnvironmentObject private var store: WorkspaceStore
    @State private var key = ""
    @State private var keyLoaded = false

    var body: some View {
        Form {
            Section("Model connection") {
                TextField("HTTPS API base URL", text: $store.endpoint)
                    .keyboardType(.URL).textInputAutocapitalization(.never).autocorrectionDisabled()
                SecureField("API key", text: $key).textInputAutocapitalization(.never).autocorrectionDisabled()
                TextField("Model identifier", text: $store.model).textInputAutocapitalization(.never).autocorrectionDisabled()
                if !store.models.isEmpty {
                    Picker("Available models", selection: $store.model) {
                        if !store.models.contains(store.model) { Text(store.model.isEmpty ? "Choose a model" : store.model).tag(store.model) }
                        ForEach(store.models, id: \.self) { Text($0).tag($0) }
                    }
                }
                Button("Save settings") { store.saveSettings(key: key) }.disabled(!keyLoaded || store.working)
                Button { store.fetchModels() } label: {
                    HStack { Text("Refresh model list"); if store.loadingModels { ProgressView() } }
                }.disabled(store.loadingModels)
                Text("Credentials are stored in Keychain. This build supports API key authentication; ChatGPT account sign-in is pending.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Section("Runtime") {
                Picker("Agent", selection: $store.engine) {
                    Text("Codex agent").tag("codexCore")
                    Text("Legacy mobile sessions").tag("responsesAdapter")
                }.disabled(store.working)
                Text("Create a new session to change agents. Older mobile sessions keep their original history format.").font(.caption).foregroundStyle(.secondary)
                LabeledContent("Task", value: store.status)
                LabeledContent("File access", value: "Imported projects")
                LabeledContent("Shell / PTY", value: store.capabilities?.process ?? "Detecting…")
                LabeledContent("Enhanced mode", value: "Adapter not installed")
                Text("Enhanced capabilities are isolated behind an adapter. This build does not acquire elevated access.").font(.caption).foregroundStyle(.secondary)
                Button("Cancel active task") { store.command("cancel") }.disabled(!store.working)
            }
            Section("Compatibility") {
                Text(CompatibilityLayer.diagnostic).textSelection(.enabled)
                Text("Minimum iOS 16.0 · iPhone and iPad · iOS 16 / 17 / 18 / 26")
                Button("Refresh diagnostics") { store.command("diagnostics") }
            }
        }
        .navigationTitle("Settings")
        .onAppear {
            do { key = try Keychain.load(); keyLoaded = true }
            catch { store.error = error.localizedDescription }
        }
    }
}
