use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn session_is_restored_after_runtime_shutdown() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let (commands, receive) = mpsc::channel(/*buffer*/ 8);
    let (events, mut output) = mpsc::channel(/*buffer*/ 16);
    let runtime = tokio::spawn(run(
        Init {
            home: home.path().to_owned(),
        },
        receive,
        events,
    ));
    assert_eq!(output.recv().await.context("ready")?["type"], "ready");
    commands.send(Command::CreateSession).await?;
    let original = output.recv().await.context("session")?;
    let id = original["session"]["id"].as_str().context("id")?.to_owned();
    commands.send(Command::Shutdown).await?;
    runtime.await??;
    let (commands, receive) = mpsc::channel(/*buffer*/ 8);
    let (events, mut output) = mpsc::channel(/*buffer*/ 16);
    let runtime = tokio::spawn(run(
        Init {
            home: home.path().to_owned(),
        },
        receive,
        events,
    ));
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
    let (commands, receive) = mpsc::channel(/*buffer*/ 8);
    let (events, mut output) = mpsc::channel(/*buffer*/ 16);
    let runtime = tokio::spawn(run(
        Init {
            home: home.path().to_owned(),
        },
        receive,
        events,
    ));
    output.recv().await.context("ready")?;
    commands
        .send(Command::OpenProject {
            path: project.path().to_owned(),
        })
        .await?;
    output.recv().await.context("project")?;
    commands
        .send(Command::PreviewChange {
            path: "hello.txt".into(),
            after: "Hello iOS\n".into(),
        })
        .await?;
    let review = output.recv().await.context("review")?;
    let id = review["id"].as_str().context("review id")?.to_owned();
    let mut visible = review.clone();
    visible["id"] = json!("REVIEW_ID");
    insta::assert_snapshot!(
        "native_file_review",
        serde_json::to_string_pretty(&visible)?
    );
    assert_eq!(
        std::fs::read_to_string(project.path().join("hello.txt"))?,
        "Hello\n"
    );
    commands
        .send(Command::Review {
            id,
            decision: Decision::Approve,
        })
        .await?;
    assert_eq!(
        output.recv().await.context("resolved")?["message"],
        "change saved"
    );
    assert_eq!(
        std::fs::read_to_string(project.path().join("hello.txt"))?,
        "Hello iOS\n"
    );
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
    assert_eq!(
        recovered.items,
        vec![
            call,
            json!({"type":"function_call_output", "call_id":"call-1", "output":"Interrupted; no pending write was applied. Request review again."})
        ]
    );
    assert_eq!(store.load(&session.id)?.items, recovered.items);
    Ok(())
}

#[tokio::test]
async fn json_git_commands_commit_reviewed_index_and_preserve_working_changes() -> anyhow::Result<()>
{
    let home = tempfile::tempdir()?;
    let project = tempfile::tempdir()?;
    std::fs::write(project.path().join("hello.txt"), "Reviewed\n")?;
    let (commands, receive) = mpsc::channel(/*buffer*/ 8);
    let (events, mut output) = mpsc::channel(/*buffer*/ 16);
    let runtime = tokio::spawn(run(
        Init {
            home: home.path().to_owned(),
        },
        receive,
        events,
    ));
    output.recv().await.context("ready")?;
    commands
        .send(serde_json::from_value(
            json!({"type":"openProject", "path":project.path()}),
        )?)
        .await?;
    output.recv().await.context("project")?;
    commands
        .send(serde_json::from_value(json!({"type":"gitInit"}))?)
        .await?;
    output.recv().await.context("initialized")?;
    commands
        .send(serde_json::from_value(json!({"type":"gitStatus"}))?)
        .await?;
    let report = output.recv().await.context("initial status")?;
    assert_eq!(
        report["git"]["changes"],
        json!([{"path":"hello.txt", "index":"", "working":"?"}])
    );
    commands
        .send(serde_json::from_value(
            json!({"type":"gitDiff", "path":"hello.txt", "layer":"working"}),
        )?)
        .await?;
    let diff = output.recv().await.context("diff")?;
    commands.send(serde_json::from_value(json!({"type":"gitStage", "path":"hello.txt", "expectedCurrent":diff["gitDiff"]["currentId"]}))?).await?;
    output.recv().await.context("staged")?;
    commands
        .send(serde_json::from_value(json!({"type":"gitStatus"}))?)
        .await?;
    let report = output.recv().await.context("staged status")?;
    assert_eq!(
        report["git"]["changes"],
        json!([{"path":"hello.txt", "index":"A", "working":""}])
    );
    std::fs::write(project.path().join("hello.txt"), "Unstaged\n")?;
    commands
        .send(serde_json::from_value(
            json!({"type":"gitCommit", "request":{
                "expectedHead":report["git"]["headId"], "expectedIndex":report["git"]["indexId"],
                "name":"Native Tester", "email":"native@example.com", "message":"Reviewed commit"
            }}),
        )?)
        .await?;
    let committed = output.recv().await.context("committed")?;
    assert_eq!(committed["type"], "gitUpdated");
    commands
        .send(serde_json::from_value(json!({"type":"gitStatus"}))?)
        .await?;
    let status = output.recv().await.context("final status")?;
    assert_eq!(
        status["git"]["changes"],
        json!([{"path":"hello.txt", "index":"", "working":"M"}])
    );
    assert_eq!(
        std::fs::read_to_string(project.path().join("hello.txt"))?,
        "Unstaged\n"
    );
    commands.send(Command::Shutdown).await?;
    runtime.await??;
    Ok(())
}
