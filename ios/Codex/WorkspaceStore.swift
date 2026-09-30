import Foundation
import SwiftUI

@MainActor
final class WorkspaceStore: ObservableObject {
    @Published var session: SessionSnapshot?
    @Published var sessions: [SessionSummary] = []
    @Published var projects: [Project] = []
    @Published var project: Project?
    @Published var files: [FileEntry] = []
    @Published var folder = ""
    @Published var file: LoadedFile?
    @Published var review: ChangeReview?
    @Published var activity: [ActivityEntry] = []
    @Published var status = "starting"
    @Published var thinking = ""
    @Published var error: String?
    @Published var capabilities: Capabilities?
    @Published var importing = false
    @Published var endpoint: String
    @Published var model: String
    @Published var models: [String] = []
    @Published var loadingModels = false
    private var bridge: RustBridge?
    private let support: URL
    let projectRoot: URL
    var working: Bool { status == "working" }
    var projectURL: URL? { project.map { projectRoot.appendingPathComponent($0.folder) } }

    init() {
        let manager = FileManager.default
        support = manager.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("Codex", isDirectory: true)
        projectRoot = manager.urls(for: .documentDirectory, in: .userDomainMask)[0].appendingPathComponent("Projects", isDirectory: true)
        endpoint = UserDefaults.standard.string(forKey: "endpoint") ?? "https://api.openai.com/v1"
        model = UserDefaults.standard.string(forKey: "model") ?? ""
        do {
            try manager.createDirectory(at: support, withIntermediateDirectories: true)
            try manager.createDirectory(at: projectRoot, withIntermediateDirectories: true)
            if let data = try? Data(contentsOf: support.appendingPathComponent("projects.json")) {
                guard data.count <= 1_048_576 else { throw CocoaError(.fileReadTooLarge) }
                projects = try JSONDecoder().decode([Project].self, from: data)
                guard projects.count <= 50, projects.allSatisfy({ UUID(uuidString: $0.folder) != nil }) else { throw CocoaError(.fileReadCorruptFile) }
                if let id = UserDefaults.standard.string(forKey: "projectID") {
                    project = projects.first { $0.id.uuidString == id }
                }
            }
            bridge = RustBridge(home: support.appendingPathComponent("Sessions")) { [weak self] event in self?.receive(event) }
        } catch { self.error = error.localizedDescription }
    }

    func command(_ type: String, _ fields: [String: Any] = [:]) {
        guard let bridge else { error = "Runtime is unavailable"; return }
        var command = fields
        command["type"] = type
        bridge.send(command)
    }

    func send(_ prompt: String) -> Bool {
        do {
            let key = try Keychain.load()
            guard !key.isEmpty, !model.isEmpty else { error = "Set an API key and choose a model in Settings."; return false }
            guard !working, session != nil else { return false }
            thinking = ""
            command("sendPrompt", ["prompt": prompt, "model": model, "endpoint": endpoint, "apiKey": key])
            return true
        } catch { self.error = error.localizedDescription; return false }
    }

    func saveSettings(key: String) {
        do {
            guard let url = URL(string: endpoint), url.scheme == "https", url.host != nil,
                  url.user == nil, url.password == nil, url.query == nil, url.fragment == nil else {
                error = "Use an HTTPS API base URL without credentials or query parameters."
                return
            }
            try Keychain.save(key)
            UserDefaults.standard.set(endpoint, forKey: "endpoint")
            UserDefaults.standard.set(model, forKey: "model")
            log("configuration", "Settings saved; credential is in Keychain.")
        } catch { self.error = error.localizedDescription }
    }

    func fetchModels() {
        guard !loadingModels else { return }
        loadingModels = true
        Task {
            defer { loadingModels = false }
            do {
                guard let base = URL(string: endpoint), base.scheme == "https", base.host != nil,
                      base.user == nil, base.password == nil, base.query == nil, base.fragment == nil else { throw URLError(.badURL) }
                var request = URLRequest(url: base.appendingPathComponent("models"))
                request.setValue("Bearer \(try Keychain.load())", forHTTPHeaderField: "Authorization")
                struct ModelList: Decodable {
                    struct Model: Decodable { var id: String }
                    var data: [Model]
                }
                // URLSession uses platform TLS/ATS. Model discovery never reads project files.
                let connection = URLSession(configuration: .ephemeral, delegate: ModelConnectionDelegate(), delegateQueue: nil)
                defer { connection.invalidateAndCancel() }
                let (data, response) = try await connection.data(for: request)
                guard let response = response as? HTTPURLResponse, response.statusCode == 200,
                      data.count <= 1_048_576 else { throw URLError(.badServerResponse) }
                models = Array(try JSONDecoder().decode(ModelList.self, from: data).data.map(\.id).sorted().prefix(200))
            } catch { self.error = "Model discovery failed: \(error.localizedDescription)" }
        }
    }

    func importProject(_ url: URL) {
        guard !working, !importing, projects.count < 50 else { error = "Cancel the task first, or remove old imported projects if the 50-project limit is reached."; return }
        importing = true
        let destination = projectRoot
        Task {
            defer { importing = false }
            do {
                let imported = try await Task.detached(priority: .userInitiated) { try ProjectImporter.copy(url, into: destination) }.value
                projects.append(imported)
                try JSONEncoder().encode(projects).write(to: support.appendingPathComponent("projects.json"), options: [.atomic, .completeFileProtectionUnlessOpen])
                selectProject(imported)
            } catch { self.error = "Project import failed: \(error.localizedDescription)" }
        }
    }

    func selectProject(_ selected: Project) {
        guard !working else { error = "Cancel the task before changing projects."; return }
        project = selected
        UserDefaults.standard.set(selected.id.uuidString, forKey: "projectID")
        folder = ""
        file = nil
        command("openProject", ["path": projectRoot.appendingPathComponent(selected.folder).path])
        command("listFiles", ["path": ""])
    }

    func browse(_ path: String) {
        folder = path
        command("listFiles", ["path": path])
    }

    func restore(_ id: String) { command("restoreSession", ["id": id]) }

    func lifecycle(_ state: String) {
        if state != "foreground" { review = nil }
        command("lifecycle", ["state": state])
    }

    private func receive(_ event: RuntimeEvent) {
        switch event.type {
        case "ready":
            capabilities = event.capabilities
            status = "idle"
            if let project { selectProject(project) }
            command("listSessions")
            if let id = UserDefaults.standard.string(forKey: "sessionID") { restore(id) }
            else { command("createSession") }
        case "session":
            session = event.session
            if let id = session?.id { UserDefaults.standard.set(id, forKey: "sessionID") }
        case "sessions": sessions = event.sessions ?? []
        case "files": files = event.files ?? []
        case "file": file = LoadedFile(path: event.path ?? "", text: event.text ?? "")
        case "delta":
            if session?.messages.last?.role != "assistant" { session?.messages.append(ChatMessage(role: "assistant", text: "")) }
            if let index = session?.messages.indices.last { session?.messages[index].text += event.text ?? "" }
        case "thinking": thinking = String((thinking + (event.text ?? "")).suffix(8192))
        case "status":
            status = event.status ?? "idle"
            if !working { review = nil; command("listSessions") }
        case "review":
            if let id = event.id, let change = event.change { review = ChangeReview(id: id, change: change) }
        case "reviewResolved":
            if review?.id == event.id { review = nil }
            log("file", event.message ?? "")
            if let file { command("readFile", ["path": file.path]) }
            browse(folder)
        case "tool": log("tool", "\(event.name ?? "Tool") · \(event.status ?? "")")
        case "error": error = event.message; log("error", event.message ?? "Unknown runtime error")
        case "diagnostics": capabilities = event.capabilities
        case "project": log("project", "Authorized imported project opened.")
        default: log("runtime", event.type)
        }
    }

    private func log(_ category: String, _ text: String) {
        activity.append(ActivityEntry(category: category, text: String(text.prefix(2048))))
        if activity.count > 500 { activity.removeFirst(activity.count - 500) }
    }
}

private final class ModelConnectionDelegate: NSObject, URLSessionTaskDelegate {
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) {
        completionHandler(nil)
    }
}
