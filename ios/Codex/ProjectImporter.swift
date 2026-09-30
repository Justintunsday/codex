import Foundation
import SwiftUI
import UniformTypeIdentifiers

struct ProjectPicker: UIViewControllerRepresentable {
    var selected: (URL) -> Void
    func makeCoordinator() -> Coordinator { Coordinator(selected: selected) }
    func makeUIViewController(context: Context) -> UIDocumentPickerViewController {
        let picker = UIDocumentPickerViewController(forOpeningContentTypes: [.folder])
        picker.delegate = context.coordinator
        picker.allowsMultipleSelection = false
        return picker
    }
    func updateUIViewController(_ controller: UIDocumentPickerViewController, context: Context) {}
    final class Coordinator: NSObject, UIDocumentPickerDelegate {
        var selected: (URL) -> Void
        init(selected: @escaping (URL) -> Void) { self.selected = selected }
        func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
            if let url = urls.first { selected(url) }
        }
    }
}

enum ProjectImporter {
    /// Work on an app-owned copy. Provider access ends after import; later Rust writes
    /// do not bypass NSFileCoordinator or depend on expired security-scoped URLs.
    static func copy(_ source: URL, into projects: URL) throws -> Project {
        let accessed = source.startAccessingSecurityScopedResource()
        defer { if accessed { source.stopAccessingSecurityScopedResource() } }
        let project = Project(id: UUID(), name: source.lastPathComponent, folder: UUID().uuidString)
        let destination = projects.appendingPathComponent(project.folder, isDirectory: true)
        let manager = FileManager.default
        try manager.createDirectory(at: destination, withIntermediateDirectories: true)
        do {
            let coordinator = NSFileCoordinator()
            var coordinationError: NSError?
            var copyError: Error?
            coordinator.coordinate(readingItemAt: source, options: .withoutChanges, error: &coordinationError) { root in
                do {
                    guard let iterator = manager.enumerator(at: root, includingPropertiesForKeys: [.isDirectoryKey, .isSymbolicLinkKey, .fileSizeKey], errorHandler: { _, error in copyError = error; return false }) else {
                        throw CocoaError(.fileReadNoPermission)
                    }
                    var count = 0
                    var size = 0
                    for case let file as URL in iterator {
                        let properties = try file.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey, .fileSizeKey])
                        guard properties.isSymbolicLink != true else { throw CocoaError(.fileReadUnsupportedScheme) }
                        count += 1
                        size += properties.fileSize ?? 0
                        guard count <= 10_000, size <= 256 * 1024 * 1024 else { throw CocoaError(.fileReadTooLarge) }
                        let rootPath = root.standardizedFileURL.path + "/"
                        guard file.standardizedFileURL.path.hasPrefix(rootPath) else { throw CocoaError(.fileReadNoPermission) }
                        let relative = String(file.standardizedFileURL.path.dropFirst(rootPath.count))
                        let target = destination.appendingPathComponent(relative)
                        if properties.isDirectory == true {
                            try manager.createDirectory(at: target, withIntermediateDirectories: true)
                        } else { try manager.copyItem(at: file, to: target) }
                    }
                } catch { copyError = error }
            }
            if let error = coordinationError { throw error }
            if let error = copyError { throw error }
            return project
        } catch {
            // Only the fresh UUID directory created above can be cleaned up.
            if destination.deletingLastPathComponent().standardizedFileURL == projects.standardizedFileURL {
                try? manager.removeItem(at: destination)
            }
            throw error
        }
    }
}
