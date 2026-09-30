import Foundation

struct ChatMessage: Codable, Identifiable {
    var role: String
    var text: String
    var id: String { "\(role):\(text.prefix(80))" }
}

struct SessionSnapshot: Decodable {
    var id: String
    var title: String
    var messages: [ChatMessage]
    var engine: String?
}

struct SessionSummary: Decodable, Identifiable {
    var id: String
    var title: String
}

struct Project: Codable, Identifiable, Hashable {
    var id: UUID
    var name: String
    var folder: String
}

struct FileEntry: Decodable, Identifiable {
    var path: String
    var isDirectory: Bool
    var id: String { path }
    var name: String { URL(fileURLWithPath: path).lastPathComponent }
}

struct FileChange: Decodable {
    var path: String
    var before: String
    var after: String
    var diff: String
    var existed: Bool
}

struct ChangeReview: Identifiable {
    var id: String
    var change: FileChange
}

struct LoadedFile: Identifiable {
    var path: String
    var text: String
    var id: String { path }
}

struct Capabilities: Decodable {
    var fileAccess: String
    var process: String
    var pty: String
    var gitCommit: String
    var jailbreak: String
    var agentEngine: String
}

struct RuntimeEvent: Decodable {
    var type: String
    var session: SessionSnapshot?
    var sessions: [SessionSummary]?
    var files: [FileEntry]?
    var path: String?
    var text: String?
    var message: String?
    var status: String?
    var name: String?
    var id: String?
    var change: FileChange?
    var capabilities: Capabilities?
    var arch: String?
    var os: String?
}

struct ActivityEntry: Identifiable {
    var id = UUID()
    var date = Date()
    var category: String
    var text: String
}
