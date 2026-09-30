use super::*;
use pretty_assertions::assert_eq;
use std::time::Duration;
use std::time::Instant;

#[test]
fn abi_copies_commands_frees_events_and_invalidates_shutdown_handles() -> Result<(), Box<dyn std::error::Error>> {
    let home = tempfile::tempdir()?;
    let config = CString::new(serde_json::json!({"home":home.path()}).to_string())?;
    // SAFETY: these CString allocations remain valid for each synchronous ABI call.
    let handle = unsafe { codex_initialize(config.as_ptr()) };
    drop(config);
    assert_ne!(handle, 0);
    let command = CString::new("{\"type\":\"createSession\"}")?;
    assert_eq!(unsafe { codex_command(handle, command.as_ptr()) }, 0);
    drop(command);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut types = Vec::new();
    while types.len() < 2 && Instant::now() < deadline {
        let event = codex_poll_event(handle);
        if event.is_null() { std::thread::sleep(Duration::from_millis(10)); continue; }
        // SAFETY: poll returned a Rust-owned C string; decode before freeing exactly once.
        let json = unsafe { CStr::from_ptr(event) }.to_str()?.to_owned();
        unsafe { codex_string_free(event) };
        let event: Value = serde_json::from_str(&json)?;
        types.push(event["type"].as_str().unwrap_or_default().to_owned());
    }
    assert_eq!(types, vec!["ready", "session"]);
    codex_shutdown(handle);
    assert!(codex_poll_event(handle).is_null());
    let command = CString::new("{\"type\":\"cancel\"}")?;
    assert_eq!(unsafe { codex_command(handle, command.as_ptr()) }, 1);
    Ok(())
}
