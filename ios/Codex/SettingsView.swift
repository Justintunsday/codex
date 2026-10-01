import SwiftUI

struct SettingsView: View {
    @EnvironmentObject private var store: WorkspaceStore
    @State private var key = ""
    @State private var keyLoaded = false

    var body: some View {
        Form {
            Section("ChatGPT account") {
                Label("Subscription sign-in is being added", systemImage: "person.crop.circle")
                Text("This build cannot use your ChatGPT Plus subscription yet.")
                    .font(.subheadline).foregroundStyle(.secondary)
            }
            Section("API connection") {
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
                Text("API credentials are stored in Keychain. API usage has separate billing from a ChatGPT subscription.")
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
                LabeledContent("App shell / PTY", value: store.capabilities?.process ?? "Detecting…")
                LabeledContent("Execution connection", value: store.terminalStatus)
                LabeledContent("Jailbreak capability", value: store.capabilities?.jailbreak ?? "Detecting…")
                Text("Configure a remote exec-server or a separately installed iOS helper in Activity → Terminal. Helper connectivity does not establish jailbreak status.").font(.caption).foregroundStyle(.secondary)
                Button("Cancel active task") { store.command("cancel") }.disabled(!store.working)
            }
            Section("Compatibility") {
                Text(CompatibilityLayer.diagnostic).textSelection(.enabled)
                Text("Minimum iOS 16.0 · iPhone and iPad · iOS 16 / 17 / 18 / 26")
                Button("Refresh diagnostics") { store.command("diagnostics") }
            }
            Section("Open source") {
                Link("Codex source · Apache 2.0 license", destination: URL(string: "https://github.com/Justintunsday/codex/tree/codex/ios-native")!)
                ForEach(["LICENSE", "NOTICE"], id: \.self) { name in
                    if let url = Bundle.main.url(forResource: name, withExtension: nil),
                       let content = try? String(contentsOf: url, encoding: .utf8) {
                        NavigationLink(name == "LICENSE" ? "Codex license" : "Codex notices") {
                            ScrollView { Text(content).font(.caption).textSelection(.enabled).padding(16) }.navigationTitle(name)
                        }
                    }
                }
                Link("SwiftTerm terminal emulator · MIT license", destination: URL(string: "https://github.com/migueldeicaza/SwiftTerm")!)
                if let url = Bundle.main.url(forResource: "ThirdPartyNotices", withExtension: "txt"),
                   let notices = try? String(contentsOf: url, encoding: .utf8) {
                    NavigationLink("Third-party notices") { ScrollView { Text(notices).font(.caption).padding(16) }.navigationTitle("Licenses") }
                }
            }
        }
        .scrollContentBackground(.hidden).background(Design.canvas)
        .navigationTitle("Settings")
        .onAppear {
            do { key = try Keychain.load(); keyLoaded = true }
            catch { store.error = error.localizedDescription }
        }
    }
}
