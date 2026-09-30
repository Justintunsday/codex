//! Versioned C ABI. Swift owns handles; returned strings belong to Rust until freed.
//! Event polling avoids callback/context lifetimes crossing scene destruction.
use codex_ios_runtime::Command;
use codex_ios_runtime::Init;
use serde_json::Value;
use std::collections::HashMap;
use std::ffi::CStr;
use std::ffi::CString;
use std::os::raw::c_char;
use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use tokio::sync::mpsc;

struct Engine {
    commands: mpsc::Sender<Command>,
    events: Mutex<mpsc::Receiver<Value>>,
}

static ENGINES: OnceLock<Mutex<HashMap<u64, Engine>>> = OnceLock::new();
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

fn engines() -> &'static Mutex<HashMap<u64, Engine>> {
    ENGINES.get_or_init(Mutex::default)
}

/// Returns 1 for this JSON/C ownership contract.
#[unsafe(no_mangle)]
pub extern "C" fn codex_abi_version() -> u32 {
    1
}

/// Starts a dedicated Tokio host thread and returns immediately. Zero indicates invalid input.
/// Startup errors arrive as events. No network or disk work runs on the caller's UI thread.
///
/// # Safety
/// `config` must be a valid NUL-terminated UTF-8 string for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn codex_initialize(config: *const c_char) -> u64 {
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the caller supplies a valid borrowed C string according to this ABI.
        let input = unsafe { read_input(config) }?;
        let init: Init = serde_json::from_str(input).ok()?;
        let (commands, command_rx) = mpsc::channel(/*buffer*/ 64);
        let (event_tx, events) = mpsc::channel(/*buffer*/ 256);
        let handle = NEXT_HANDLE.fetch_add(/*val*/ 1, Ordering::Relaxed);
        let mut registry = engines().lock().ok()?;
        if registry.len() >= 4 {
            return None;
        }
        std::thread::Builder::new()
            .name("codex-ios-host".into())
            .stack_size(/*size*/ 8 * 1024 * 1024)
            .spawn(move || {
                let reporting = event_tx.clone();
                let result = catch_unwind(AssertUnwindSafe(|| {
                    let runtime = tokio::runtime::Builder::new_multi_thread()
                        .worker_threads(/*val*/ 2)
                        .thread_stack_size(/*val*/ 8 * 1024 * 1024)
                        .enable_all()
                        .build()
                        .map_err(|error| error.to_string())?;
                    runtime
                        .block_on(codex_ios_runtime::run(init, command_rx, event_tx))
                        .map_err(|error| error.to_string())
                }));
                let message = match result {
                    Ok(Ok(())) => return,
                    Ok(Err(error)) => error,
                    Err(_) => "Rust runtime panic; restart the runtime to recover saved sessions"
                        .to_owned(),
                };
                let _ = reporting.try_send(serde_json::json!({"type":"error", "message":message}));
            })
            .ok()?;
        registry.insert(
            handle,
            Engine {
                commands,
                events: Mutex::new(events),
            },
        );
        Some(handle)
    }))
    .ok()
    .flatten()
    .unwrap_or(/*default*/ 0)
}

/// Queues a JSON command. 0 = accepted, 1 = invalid handle/input, 2 = busy/closed, 3 = panic.
/// Inputs are copied before returning. A queue error must be shown to the caller.
///
/// # Safety
/// `command` must be a valid NUL-terminated UTF-8 string for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn codex_command(handle: u64, command: *const c_char) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: borrowed from the caller only within this call.
        let Some(input) = (unsafe { read_input(command) }) else {
            return 1;
        };
        let Ok(command) = serde_json::from_str::<Command>(input) else {
            return 1;
        };
        let Ok(registry) = engines().lock() else {
            return 3;
        };
        let Some(engine) = registry.get(&handle) else {
            return 1;
        };
        match engine.commands.try_send(command) {
            Ok(()) => 0,
            Err(_) => 2,
        }
    }))
    .unwrap_or(/*default*/ 3)
}

/// Returns one owned UTF-8 JSON event, or null if none is available.
/// Always release non-null results with `codex_string_free`, including decode failures.
#[unsafe(no_mangle)]
pub extern "C" fn codex_poll_event(handle: u64) -> *mut c_char {
    catch_unwind(AssertUnwindSafe(|| {
        let registry = engines().lock().ok()?;
        let engine = registry.get(&handle)?;
        let event = engine.events.lock().ok()?.try_recv().ok()?;
        CString::new(serde_json::to_string(&event).ok()?)
            .ok()
            .map(CString::into_raw)
    }))
    .ok()
    .flatten()
    .unwrap_or(std::ptr::null_mut())
}

/// # Safety
/// `string` must be null or an unfreed pointer returned by `codex_poll_event`.
/// It must be freed exactly once and never read after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn codex_string_free(string: *mut c_char) {
    if !string.is_null() {
        // SAFETY: the caller transfers this Rust-owned allocation back exactly once.
        drop(unsafe { CString::from_raw(string) });
    }
}

/// Invalidates the handle without joining a thread on the Swift main thread.
/// Closing both channels cancels active work; saved sessions survive shutdown.
#[unsafe(no_mangle)]
pub extern "C" fn codex_shutdown(handle: u64) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if let Ok(mut registry) = engines().lock() {
            registry.remove(&handle);
        }
    }));
}

unsafe fn read_input<'a>(input: *const c_char) -> Option<&'a str> {
    if input.is_null() {
        return None;
    }
    // SAFETY: guaranteed by the public ABI's valid NUL-terminated pointer precondition.
    let bytes = unsafe { CStr::from_ptr(input) }.to_bytes();
    if bytes.len() > codex_ios_runtime::MAX_COMMAND_BYTES {
        return None;
    }
    std::str::from_utf8(bytes).ok()
}

#[cfg(test)]
#[path = "ffi_tests.rs"]
mod tests;
