use super::*;
use http::HeaderValue;
use pretty_assertions::assert_eq;
use std::sync::Mutex;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

fn sse(events: &[Value]) -> String {
    events
        .iter()
        .map(|event| {
            format!(
                "event: {}\ndata: {event}\n\n",
                event["type"].as_str().unwrap_or_default()
            )
        })
        .collect()
}

#[tokio::test]
async fn upstream_agent_streams_and_resumes_its_own_rollout_without_persisting_credentials()
-> anyhow::Result<()> {
    let server = MockServer::start().await;
    let item = json!({"id":"message-1", "type":"message", "role":"assistant", "content":[{"type":"output_text", "text":"Hello from Core"}]});
    let body = sse(&[
        json!({"type":"response.created", "response":{"id":"response-1"}}),
        json!({"type":"response.output_item.added", "output_index":0, "item":{"id":"message-1", "type":"message", "role":"assistant", "content":[]}}),
        json!({"type":"response.output_text.delta", "item_id":"message-1", "output_index":0, "content_index":0, "delta":"Hello from Core"}),
        json!({"type":"response.output_item.done", "output_index":0, "item":item}),
        json!({"type":"response.completed", "response":{"id":"response-1"}}),
    ]);
    // The resumed provider response only contains a completed item, with no delta.
    let completed_body = sse(&[
        json!({"type":"response.created", "response":{"id":"response-2"}}),
        json!({"type":"response.output_item.done", "output_index":0, "item":item}),
        json!({"type":"response.completed", "response":{"id":"response-2"}}),
    ]);
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(
            ResponseTemplate::new(/*s*/ 200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .up_to_n_times(/*n*/ 1)
        .with_priority(/*p*/ 1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(
            ResponseTemplate::new(/*s*/ 200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(completed_body),
        )
        .with_priority(/*p*/ 2)
        .mount(&server)
        .await;
    let root = tempfile::tempdir()?;
    let store = SessionStore::new(root.path().to_owned())?;
    let mut session = Session::new();
    session.engine = Some(crate::AgentEngine::CodexCore);
    let id = session.id.clone();
    let (events, mut output) = mpsc::channel::<Value>(/*buffer*/ 64);
    let collector = tokio::spawn(async move {
        let mut deltas = Vec::new();
        while let Some(event) = output.recv().await {
            if event["type"] == "delta" {
                deltas.push(event["text"].clone());
            }
        }
        deltas
    });
    for prompt in ["First question", "Second question"] {
        let request = RequestConfig {
            model: "gpt-6-sol".into(),
            endpoint: server.uri(),
            authorization: HeaderValue::from_static("Bearer fixture-key"),
        };
        let completed = tokio::time::timeout(
            Duration::from_secs(/*secs*/ 30),
            run_turn(
                session,
                prompt.into(),
                request,
                CoreHost {
                    store: store.clone(),
                    project: None,
                    reviews: Arc::new(Mutex::new(HashMap::new())),
                    events: events.clone(),
                    cancellation: CancellationToken::new(),
                },
            ),
        )
        .await??;
        session = store.load(&id)?;
        assert_eq!(session, completed);
    }
    drop(events);
    assert_eq!(collector.await?, vec![json!("Hello from Core")]);
    assert!(session.core_rollout.is_some());
    assert_eq!(
        session
            .messages
            .iter()
            .map(|message| (message.role.as_str(), message.text.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("user", "First question"),
            ("assistant", "Hello from Core"),
            ("user", "Second question"),
            ("assistant", "Hello from Core")
        ]
    );
    let requests = server.received_requests().await.context("requests")?;
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests
            .iter()
            .map(|request| request.headers["authorization"].to_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["Bearer fixture-key", "Bearer fixture-key"]
    );
    let resumed: Value = serde_json::from_slice(&requests[1].body)?;
    let input = resumed["input"].to_string();
    assert!(
        input.contains("First question")
            && input.contains("Second question")
            && input.contains("Hello from Core")
    );
    assert!(!root.path().join("Core/auth.json").exists());
    let rollout = std::fs::read(
        root.path()
            .join("Core")
            .join(session.core_rollout.context("rollout")?),
    )?;
    assert!(!String::from_utf8_lossy(&rollout).contains("fixture-key"));
    Ok(())
}

#[tokio::test]
async fn cancelling_core_at_file_review_preserves_rollout_and_never_writes() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    let body = sse(&[
        json!({"type":"response.created", "response":{"id":"response-tool"}}),
        json!({"type":"response.output_item.done", "output_index":0, "item":{"type":"function_call", "name":"propose_change", "call_id":"change-1", "arguments":"{\"path\":\"hello.txt\",\"after\":\"Unapproved change\\n\"}"}}),
        json!({"type":"response.completed", "response":{"id":"response-tool"}}),
    ]);
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(
            ResponseTemplate::new(/*s*/ 200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .mount(&server)
        .await;
    let root = tempfile::tempdir()?;
    let project_root = tempfile::tempdir()?;
    std::fs::write(project_root.path().join("hello.txt"), "Original\n")?;
    let store = SessionStore::new(root.path().to_owned())?;
    let mut session = Session::new();
    session.engine = Some(crate::AgentEngine::CodexCore);
    let id = session.id.clone();
    let reviews = Arc::new(Mutex::new(HashMap::new()));
    let cancellation = CancellationToken::new();
    let (events, mut output) = mpsc::channel(/*buffer*/ 64);
    let request = RequestConfig {
        model: "gpt-6-sol".into(),
        endpoint: server.uri(),
        authorization: HeaderValue::from_static("Bearer fixture-key"),
    };
    let turn = tokio::spawn(run_turn(
        session,
        "Update hello.txt".into(),
        request,
        CoreHost {
            store: store.clone(),
            project: Some(Arc::new(ScopedFiles::new(project_root.path())?)),
            reviews: reviews.clone(),
            events,
            cancellation: cancellation.clone(),
        },
    ));
    let review = tokio::time::timeout(Duration::from_secs(/*secs*/ 30), async {
        while let Some(event) = output.recv().await {
            if event["type"] == "review" {
                return Ok(event);
            }
        }
        bail!("core stopped before file review");
    })
    .await??;
    assert_eq!(review["change"]["before"], "Original\n");
    assert_eq!(review["change"]["after"], "Unapproved change\n");
    assert_eq!(
        std::fs::read_to_string(project_root.path().join("hello.txt"))?,
        "Original\n"
    );
    cancellation.cancel();
    reviews
        .lock()
        .map_err(|_| anyhow::anyhow!("reviews"))?
        .clear();
    let error = tokio::time::timeout(Duration::from_secs(/*secs*/ 10), turn)
        .await??
        .expect_err("cancelled turn must fail");
    assert!(error.to_string().contains("cancelled"));
    let restored = store.load(&id)?;
    assert_eq!(
        restored.messages,
        vec![Message {
            role: "user".into(),
            text: "Update hello.txt".into()
        }]
    );
    let rollout_path = root
        .path()
        .join("Core")
        .join(restored.core_rollout.context("core rollout")?);
    let history = RolloutRecorder::get_rollout_history(&rollout_path).await?;
    assert_eq!(
        history.session_cwd(),
        Some(root.path().join("Core").canonicalize()?)
    );
    assert!(!history.get_rollout_items().is_empty());
    assert_eq!(
        std::fs::read_to_string(project_root.path().join("hello.txt"))?,
        "Original\n"
    );
    let requests = server.received_requests().await.context("requests")?;
    assert_eq!(requests.len(), 1);
    let body: Value = serde_json::from_slice(&requests[0].body)?;
    // Responses Lite carries tool definitions in a developer input item.
    let tools = body
        .get("tools")
        .cloned()
        .or_else(|| {
            body["input"]
                .as_array()?
                .iter()
                .find(|item| item["type"] == "additional_tools")
                .map(|item| item["tools"].clone())
        })
        .context("provider tool definitions")?
        .to_string();
    assert!(tools.contains("propose_change"));
    assert!(!tools.contains("apply_patch") && !tools.contains("exec_command"));
    assert!(!root.path().join("Core/auth.json").exists());
    Ok(())
}
