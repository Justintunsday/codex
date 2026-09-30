#if DEBUG
import Foundation

/// A launch-only fixture for native UI tests. Release archives do not contain it.
enum UITestProject {
    static var enabled: Bool { ProcessInfo.processInfo.environment["CODEX_UI_TEST_PROJECT"] == "1" }

    static func prepare(support: URL, projects: URL, preferences: UserDefaults) throws {
        let gitFixture = ProcessInfo.processInfo.environment["CODEX_UI_TEST_GIT"] == "1"
        let project = Project(id: UUID(uuidString: "C1EF77AB-982B-4D49-A66D-9C9D0D8C0F0B")!,
                              name: "UI test project", folder: gitFixture ? "13F90202-66B7-4928-BB7E-247FE1A7C243" : "2A8DDE66-1D1D-44B2-9E09-40F28B39903F")
        let root = projects.appendingPathComponent(project.folder, isDirectory: true)
        let manager = FileManager.default
        try manager.createDirectory(at: root, withIntermediateDirectories: true)
        let file = root.appendingPathComponent("hello.txt")
        let reset = ProcessInfo.processInfo.environment["CODEX_UI_TEST_RESET"] == "1"
        if gitFixture && reset {
            let metadata = root.appendingPathComponent(".git", isDirectory: true)
            guard projects.lastPathComponent == "UITestProjects",
                  root.deletingLastPathComponent().standardizedFileURL == projects.standardizedFileURL,
                  try root.resourceValues(forKeys: [.isSymbolicLinkKey]).isSymbolicLink != true else { throw CocoaError(.fileWriteNoPermission) }
            if manager.fileExists(atPath: metadata.path) {
                guard try metadata.resourceValues(forKeys: [.isSymbolicLinkKey]).isSymbolicLink != true else { throw CocoaError(.fileWriteNoPermission) }
                try manager.removeItem(at: metadata)
            }
        }
        if !manager.fileExists(atPath: file.path) || reset {
            try Data("Hello from the project.\n".utf8).write(to: file, options: .atomic)
        }
        try JSONEncoder().encode([project]).write(to: support.appendingPathComponent("projects.json"), options: .atomic)
        preferences.set(project.id.uuidString, forKey: "projectID")
    }
}
#endif
