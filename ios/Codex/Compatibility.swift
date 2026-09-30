import SwiftUI

/// All generation checks live here. UI uses the iOS 16 API baseline.
enum CompatibilityLayer {
    static var generation: String {
        if #available(iOS 26.0, *) { return "iOS 26 generation" }
        if #available(iOS 18.0, *) { return "iOS 18 generation" }
        if #available(iOS 17.0, *) { return "iOS 17 generation" }
        return "iOS 16 generation"
    }

    static var diagnostic: String {
        "\(generation) · \(ProcessInfo.processInfo.operatingSystemVersionString) · arm64"
    }
}

enum Design {
    static let accent = Color("AccentColor")
    static let canvas = Color("Canvas")
    static let display = Font.system(.largeTitle, design: .serif).weight(.semibold)
    static let code = Font.system(.body, design: .monospaced)
    static let gap: CGFloat = 16
}
