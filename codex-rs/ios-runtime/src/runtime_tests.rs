use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn session_is_restored_after_runtime_shutdown() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let (commands, receive) = mpsc::channel(8);
    let (events, mut output) = mpsc::channel(16);
    let runtime = tokio::spawn(run(Init { home:home.path().to_owned() }, receive, events));
    assert_eq!(output.recv().await.context("ready")?["type"], "ready");
    commands.send(Command::CreateSession).await?;
    let original = output.recv().await.context("session")?;
    let id = original["session"]["id"].as_str().context("id")?.to_owned();
    commands.send(Command::Shutdown).await?;
    runtime.await??;
    let (commands, receive) = mpsc::channel(8);
    let (events, mut output) = mpsc::channel(16);
    let runtime = tokio::spawn(run(Init { home:home.path().to_owned() }, receive, events));
    output.recv().await.context("ready")?;
    commands.send(Command::RestoreSession { id }).await?;
    assert_eq!(output.recv().await.context("restored")?, original);
    commands.send(Command::Shutdown).await?;
    runtime.await??;
    Ok(())
}

#[tokio::test]
async fn reviewed_file_change_is_visible_and_applied_only_after_approval() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let project = tempfile::tempdir()?;
    std::fs::write(project.path().join("hello.txt"), "Hello\n")?;
    let (commands, receive) = mpsc::channel(8);
    let (events, mut output) = mpsc::channel(16);
    let runtime = tokio::spawn(run(Init { home:home.path().to_owned() }, receive, events));
    output.recv().await.context("ready")?;
    commands.send(Command::OpenProject { path:project.path().to_owned() }).await?;
    output.recv().await.context("project")?;
    commands.send(Command::PreviewChange { path:"hello.txt".into(), after:"Hello iOS\n".into() }).await?;
    let review = output.recv().await.context("review")?;
    let id = review["id"].as_str().context("review id")?.to_owned();
    let mut visible = review.clone();
    visible["id"] = json!("REVIEW_ID");
    insta::assert_snapshot!("native_file_review", serde_json::to_string_pretty(&visible)?);
    assert_eq!(std::fs::read_to_string(project.path().join("hello.txt"))?, "Hello\n");
    commands.send(Command::Review { id, decision:Decision::Approve }).await?;
    assert_eq!(output.recv().await.context("resolved")?["message"], "change saved");
    assert_eq!(std::fs::read_to_string(project.path().join("hello.txt"))?, "Hello iOS\n");
    commands.send(Command::Shutdown).await?;
    runtime.await??;
    Ok(())
}

#[test]
fn interrupted_tool_recovery_appends_a_result_and_retains_existing_context() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let store = SessionStore::new(root.path().to_owned())?;
    let mut session = Session::new();
    let call = json!({"type":"function_call", "call_id":"call-1", "name":"propose_change", "arguments":"{}"});
    session.items.push(call.clone());
    store.save(&session)?;
    let recovered = store.load(&session.id)?;
    assert_eq!(recovered.items, vec![call, json!({"type":"function_call_output", "call_id":"call-1", "output":"Interrupted; no pending write was applied. Request review again."})]);
    assert_eq!(store.load(&session.id)?.items, recovered.items);
    Ok(())
}
