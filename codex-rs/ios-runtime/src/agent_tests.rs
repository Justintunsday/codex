use super::*;
use pretty_assertions::assert_eq;
use std::collections::HashMap;
use std::sync::Mutex;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

#[tokio::test]
async fn streams_with_upstream_transport_and_restores_incremental_context() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    let output_item = json!({"type":"message", "role":"assistant", "content":[{"type":"output_text", "text":"Hello iOS"}]});
    let payload: String = [
        json!({"type":"response.output_text.delta", "delta":"Hello iOS"}),
        json!({"type":"response.output_item.done", "item":output_item}),
        json!({"type":"response.completed", "response":{"id":"response-1"}}),
    ]
    .iter()
    .map(|event| {
        format!(
            "event: {}\ndata: {event}\n\n",
            event["type"].as_str().unwrap_or_default()
        )
    })
    .collect();
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(payload),
        )
        .mount(&server)
        .await;
    let home = tempfile::tempdir()?;
    let store = SessionStore::new(home.path().to_owned())?;
    let session = Session::new();
    let id = session.id.clone();
    let (events, mut output) = mpsc::channel(/*buffer*/ 32);
    let request = RequestConfig {
        model: "fixture-model".into(),
        endpoint: server.uri(),
        authorization: HeaderValue::from_static("Bearer fixture-key"),
    };
    let result = run_turn(
        session,
        "Hello".into(),
        request,
        store.clone(),
        /*project*/ None,
        Arc::new(Mutex::new(HashMap::new())),
        events,
    )
    .await?;
    let mut kinds = Vec::new();
    while let Some(event) = output.recv().await {
        kinds.push(event["type"].as_str().unwrap_or_default().to_owned());
    }
    assert_eq!(kinds, vec!["session", "delta"]);
    let expected = vec![
        json!({"type":"message", "role":"user", "content":[{"type":"input_text", "text":"Hello"}]}),
        output_item,
    ];
    assert_eq!(result.items, expected);
    assert_eq!(store.load(&id)?.items, result.items);
    let requests = server
        .received_requests()
        .await
        .context("recorded requests")?;
    assert_eq!(requests.len(), 1);
    let body: Value = serde_json::from_slice(&requests[0].body)?;
    assert_eq!(body["input"], json!([expected[0]]));
    assert_eq!(requests[0].headers["authorization"], "Bearer fixture-key");
    Ok(())
}

#[tokio::test]
async fn cancelled_review_does_not_apply_an_agent_write() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    std::fs::write(root.path().join("file.txt"), "original")?;
    let project = Arc::new(ScopedFiles::new(root.path())?);
    let reviews = Arc::new(Mutex::new(HashMap::new()));
    let (events, mut output) = mpsc::channel(/*buffer*/ 8);
    let (answer, receive) = oneshot::channel();
    request_review(
        project.prepare("file.txt", "proposed".into())?,
        project,
        &reviews,
        &events,
        Some(answer),
    )
    .await?;
    assert_eq!(output.recv().await.context("review")?["type"], "review");
    reviews
        .lock()
        .map_err(|_| anyhow::anyhow!("poisoned"))?
        .clear();
    assert!(receive.await.is_err());
    assert_eq!(
        std::fs::read_to_string(root.path().join("file.txt"))?,
        "original"
    );
    Ok(())
}
