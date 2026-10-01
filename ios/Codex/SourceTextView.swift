import SwiftUI

/// Keep short documents at the viewport origin while long lines scroll without wrapping.
struct SourceTextView: View {
    enum Style: Equatable { case plain, diff }
    let text: String
    var style = Style.plain
    let identifier: String

    var body: some View {
        GeometryReader { viewport in
            ScrollView([.horizontal, .vertical]) {
                Group {
                    if style == .diff {
                        VStack(alignment: .leading, spacing: 2) {
                            ForEach(Array(text.components(separatedBy: "\n").enumerated()), id: \.offset) { index, line in
                                Text(line.isEmpty ? " " : line)
                                    .foregroundStyle(color(line))
                                    .accessibilityIdentifier(index == 0 ? identifier + "Start" : identifier + "Line\(index)")
                            }
                        }
                    } else {
                        Text(text).accessibilityIdentifier(identifier)
                    }
                }
                .font(Design.code)
                .textSelection(.enabled)
                .fixedSize(horizontal: true, vertical: true)
                .padding(16)
                .frame(minWidth: viewport.size.width, minHeight: viewport.size.height, alignment: .topLeading)
            }
        }
    }

    private func color(_ line: String) -> Color {
        if line.hasPrefix("+") { return Color("DiffAdded") }
        if line.hasPrefix("-") { return Color("DiffRemoved") }
        return .primary
    }
}
