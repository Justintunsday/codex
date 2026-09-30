#if DEBUG
import Foundation

/// A launch-only fixture for native UI tests. Release archives do not contain it.
enum UITestProject {
    static var enabled: Bool { ProcessInfo.processInfo.environment["CODEX_UI_TEST_PROJECT"] == "1" }

    static func prepare(support: URL, projects: URL, preferences: UserDefaults) throws {
        let project = Project(id: UUID(uuidString: "C1EF77AB-982B-4D49-A66D-9C9D0D8C0F0B")!,
                              name: "UI test project", folder: "2A8DDE66-1D1D-44B2-9E09-40F28B39903F")
        let root = projects.appendingPathComponent(project.folder, isDirectory: true)
        let manager = FileManager.default
        try manager.createDirectory(at: root, withIntermediateDirectories: true)
        let file = root.appendingPathComponent("hello.txt")
        if !manager.fileExists(atPath: file.path) || ProcessInfo.processInfo.environment["CODEX_UI_TEST_RESET"] == "1" {
            try Data("Hello from the project.\n".utf8).write(to: file, options: .atomic)
        }
        try JSONEncoder().encode([project]).write(to: support.appendingPathComponent("projects.json"), options: .atomic)
        preferences.set(project.id.uuidString, forKey: "projectID")
    }
}
#endif
