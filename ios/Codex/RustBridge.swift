import Foundation

/// All FFI access runs on one serial queue; UI delivery always hops to MainActor.
final class RustBridge {
    private final class State {
        var handle: UInt64 = 0
        var timer: DispatchSourceTimer?
    }
    private let state = State()
    private let queue = DispatchQueue(label: "codex.ffi", qos: .userInitiated)
    private let receive: @MainActor (RuntimeEvent) -> Void

    init(home: URL, receive: @escaping @MainActor (RuntimeEvent) -> Void) {
        self.receive = receive
        let state = self.state
        let queue = self.queue
        queue.async {
            guard codex_abi_version() == 1,
                  let data = try? JSONSerialization.data(withJSONObject: ["home": home.path]),
                  let json = String(data: data, encoding: .utf8) else { return }
            state.handle = json.withCString { codex_initialize($0) }
            guard state.handle != 0 else {
                Self.deliverError("Rust initialization failed", receive: receive)
                return
            }
            let timer = DispatchSource.makeTimerSource(queue: queue)
            timer.schedule(deadline: .now(), repeating: .milliseconds(100))
            timer.setEventHandler { [weak state] in
                guard let state, state.handle != 0 else { return }
                for _ in 0..<32 {
                    guard let pointer = codex_poll_event(state.handle) else { break }
                    let data = Data(String(cString: pointer).utf8)
                    codex_string_free(pointer)
                    do {
                        let event = try JSONDecoder().decode(RuntimeEvent.self, from: data)
                        // FIFO delivery preserves the order of deltas, states and terminal frames.
                        DispatchQueue.main.async { receive(event) }
                    } catch {
                        Self.deliverError("Invalid Rust event: \(error.localizedDescription)", receive: receive)
                    }
                }
            }
            state.timer = timer
            timer.resume()
        }
    }

    func send(_ command: [String: Any]) {
        let state = self.state
        let receive = self.receive
        queue.async {
            do {
                let data = try JSONSerialization.data(withJSONObject: command)
                guard data.count <= 131_072, let json = String(data: data, encoding: .utf8) else {
                    Self.deliverError("Command exceeds the mobile limit", receive: receive)
                    return
                }
                let status = json.withCString { codex_command(state.handle, $0) }
                if status != 0 { Self.deliverError("Runtime rejected command (\(status)). Try again after restarting the runtime.", receive: receive) }
            } catch { Self.deliverError(error.localizedDescription, receive: receive) }
        }
    }

    private static func deliverError(_ text: String, receive: @escaping @MainActor (RuntimeEvent) -> Void) {
        let event = RuntimeEvent(type: "error", message: text)
        DispatchQueue.main.async { receive(event) }
    }

    deinit {
        let state = self.state
        queue.async {
            state.timer?.cancel()
            state.timer = nil
            codex_shutdown(state.handle)
            state.handle = 0
        }
    }
}
