import SwiftUI
import SwiftTerm
import UIKit

struct TerminalPanel: View {
    @EnvironmentObject private var store: WorkspaceStore
    @State private var endpoint = ""
    @State private var token = ""
    @State private var directory = ""
    @State private var backend = "remote"
    @State private var mode = "shell"
    @State private var program = ""
    @State private var arguments = ""
    @State private var settings = false
    @State private var keyLoaded = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Label(store.terminalStatus.capitalized, systemImage: "terminal")
                    .font(.headline).fixedSize(horizontal: false, vertical: true)
                Spacer()
                Button { settings = true } label: { Label("Terminal connection", systemImage: "gearshape").labelStyle(.iconOnly).frame(width: 44, height: 44) }
            }.padding(.horizontal, 16)
            if !store.terminalMessage.isEmpty { Text(store.terminalMessage).font(.caption).padding(.horizontal, 16) }
            ScrollView([.horizontal, .vertical]) {
                TerminalCanvas(store: store)
                    .id(store.terminalGeneration)
                    .fixedSize()
            }.accessibilityIdentifier("nativeTerminal")
            HStack(spacing: 16) {
                Button { settings = true } label: { Label("Start terminal", systemImage: "play.fill").frame(minWidth: 44, minHeight: 44) }
                    .disabled(store.terminalStatus == "running" || store.terminalStatus == "starting")
                Button { store.command("terminal", ["request": ["type": "interrupt"]]) } label: { Label("Interrupt", systemImage: "hand.raised").frame(minWidth: 44, minHeight: 44) }
                    .disabled(store.terminalStatus != "running")
                Spacer()
                Button(role: .destructive) { store.command("terminal", ["request": ["type": "stop"]]) } label: { Label("Stop terminal", systemImage: "stop.fill").frame(minWidth: 44, minHeight: 44) }
                    .disabled(store.terminalStatus != "running" && store.terminalStatus != "starting")
            }.labelStyle(.iconOnly).buttonStyle(.bordered).padding(.horizontal, 16)
            Text("Remote terminal · 80 × 24 · stops when the app enters background")
                .font(.caption).foregroundStyle(.secondary).padding(.horizontal, 16)
        }.padding(.vertical, 12)
        .sheet(isPresented: $settings) {
            NavigationStack {
                Form {
                    Section("Execution connection") {
                        Picker("Backend", selection: $backend) { Text("Remote exec-server").tag("remote"); Text("Installed iOS helper").tag("enhancedHelper") }
                        TextField("WSS exec-server URL", text: $endpoint).keyboardType(.URL).textInputAutocapitalization(.never).autocorrectionDisabled()
                        SecureField("Connection token", text: $token).textInputAutocapitalization(.never).autocorrectionDisabled()
                        TextField("Executor directory (optional)", text: $directory).textInputAutocapitalization(.never).autocorrectionDisabled()
                        Text("The installed iOS helper requires a TLS endpoint on 127.0.0.1 or ::1 and must report an iOS executor. The helper is installed separately.")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    Section("Launch") {
                        Picker("Run", selection: $mode) { Text("Interactive shell (PTY)").tag("shell"); Text("Program").tag("program") }
                        if mode == "program" {
                            TextField("Program on executor", text: $program).textInputAutocapitalization(.never).autocorrectionDisabled()
                            TextField("Arguments, one per line", text: $arguments, axis: .vertical).lineLimit(3...8).autocorrectionDisabled()
                        }
                        Button("Save connection and start") {
                            do {
                                guard let url = URL(string: endpoint), url.scheme == "wss", url.host != nil,
                                      url.user == nil, url.password == nil, url.query == nil, url.fragment == nil,
                                      endpoint.utf8.count <= 2048, token.utf8.count <= 8192, directory.utf8.count <= 8192 else {
                                    store.error = "Use a WSS connection URL without credentials or query parameters."
                                    return
                                }
                                try Keychain.save(token, credential: .terminalToken)
                                UserDefaults.standard.set(endpoint, forKey: "terminalEndpoint")
                                UserDefaults.standard.set(directory, forKey: "terminalDirectory")
                                var launch: [String: Any] = ["type": mode]
                                if mode == "program" { launch["argv"] = [program] + arguments.split(separator: "\n").map(String.init) }
                                store.command("terminal", ["request": ["type": "start", "connection": ["endpoint": endpoint, "token": token, "directory": directory, "backend": backend], "program": launch]])
                                settings = false
                            } catch { store.error = error.localizedDescription }
                        }.disabled(!keyLoaded || endpoint.isEmpty || token.isEmpty || (mode == "program" && program.isEmpty))
                        Text("Commands run on the selected executor. Imported files remain in this app's container; they are not automatically uploaded.")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }.scrollDismissesKeyboard(.interactively)
                .scrollContentBackground(.hidden).background(Design.canvas)
                .navigationTitle("Terminal connection")
                .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Done") { settings = false } } }
            }
        }
        .onAppear {
            endpoint = UserDefaults.standard.string(forKey: "terminalEndpoint") ?? ""
            directory = UserDefaults.standard.string(forKey: "terminalDirectory") ?? ""
            do { token = try Keychain.load(.terminalToken); keyLoaded = true } catch { store.error = error.localizedDescription }
        }
    }

}

private struct TerminalCanvas: UIViewRepresentable {
    @ObservedObject var store: WorkspaceStore
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    func makeCoordinator() -> Coordinator { Coordinator(store: store) }
    func makeUIView(context: Context) -> TerminalContainer {
        let container = TerminalContainer()
        container.terminal.terminalDelegate = context.coordinator
        return container
    }
    func sizeThatFits(_ proposal: ProposedViewSize, uiView: TerminalContainer, context: Context) -> CGSize? {
        uiView.terminal.getOptimalFrameSize().size
    }
    func updateUIView(_ container: TerminalContainer, context: Context) {
        let view = container.terminal
        // Reading SwiftUI's value makes live Dynamic Type changes update the UIKit view.
        _ = dynamicTypeSize
        let font = UIFontMetrics(forTextStyle: .body).scaledFont(for: UIFont.monospacedSystemFont(ofSize: 14, weight: .regular), compatibleWith: container.traitCollection)
        if view.font.pointSize != font.pointSize {
            view.frame = .zero
            view.font = font
            container.invalidateIntrinsicContentSize()
            container.setNeedsLayout()
        }
        let coordinator = context.coordinator
        if coordinator.generation != store.terminalGeneration {
            coordinator.generation = store.terminalGeneration; coordinator.sequence = 0
            view.feed(text: "\u{1b}c")
        }
        for frame in store.terminalFrames where frame.id > coordinator.sequence {
            if frame.id > coordinator.sequence + 1 { view.feed(text: "\r\n[Earlier output omitted]\r\n") }
            view.feed(byteArray: Array(frame.data)[...]); coordinator.sequence = frame.id
        }
        if store.terminalStatus != "running", view.isFirstResponder { view.resignFirstResponder() }
    }
    @MainActor final class Coordinator: NSObject, @preconcurrency TerminalViewDelegate {
        weak var store: WorkspaceStore?
        var sequence: UInt64 = 0
        var generation: UUID?
        init(store: WorkspaceStore) { self.store = store }
        func send(source: SwiftTerm.TerminalView, data: ArraySlice<UInt8>) {
            guard store?.terminalStatus == "running" else { return }
            for chunk in stride(from: 0, to: data.count, by: 4096) {
                let start = data.startIndex + chunk, end = min(start + 4096, data.endIndex)
                store?.command("terminal", ["request": ["type": "write", "chunk": Data(data[start..<end]).base64EncodedString()]])
            }
        }
        func sizeChanged(source: SwiftTerm.TerminalView, newCols: Int, newRows: Int) {}
        func setTerminalTitle(source: SwiftTerm.TerminalView, title: String) {}
        func hostCurrentDirectoryUpdate(source: SwiftTerm.TerminalView, directory: String?) {}
        func scrolled(source: SwiftTerm.TerminalView, position: Double) {}
        func requestOpenLink(source: SwiftTerm.TerminalView, link: String, params: [String: String]) {}
        func bell(source: SwiftTerm.TerminalView) {}
        func clipboardCopy(source: SwiftTerm.TerminalView, content: Data) {}
        func clipboardRead(source: SwiftTerm.TerminalView) -> Data? { nil }
        func iTermContent(source: SwiftTerm.TerminalView, content: ArraySlice<UInt8>) {}
        func rangeChanged(source: SwiftTerm.TerminalView, startY: Int, endY: Int) {}
    }
}

private final class TerminalContainer: UIView {
    let terminal: SwiftTerm.TerminalView
    init() {
        let font = UIFontMetrics(forTextStyle: .body).scaledFont(for: UIFont.monospacedSystemFont(ofSize: 14, weight: .regular))
        terminal = SwiftTerm.TerminalView(frame: .zero, font: font,
            options: TerminalOptions(cols: 80, rows: 24, scrollback: 500, enableSixelReported: false, kittyImageCacheLimitBytes: 8 * 1024 * 1024))
        super.init(frame: .zero)
        terminal.nativeBackgroundColor = .systemBackground
        terminal.nativeForegroundColor = .label
        addSubview(terminal)
    }
    required init?(coder: NSCoder) { return nil }
    override var intrinsicContentSize: CGSize { terminal.getOptimalFrameSize().size }
    override func layoutSubviews() {
        super.layoutSubviews()
        terminal.frame = terminal.getOptimalFrameSize()
    }
}
