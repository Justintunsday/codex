use crate::Review;
use crate::Reviews;
use crate::session::MAX_CONTEXT_BYTES;
use crate::session::Message;
use crate::session::Session;
use crate::session::SessionStore;
use anyhow::Context;
use anyhow::bail;
use codex_api::AuthProvider;
use codex_api::Compression;
use codex_api::Provider;
use codex_api::ReqwestTransport;
use codex_api::ResponseEvent;
use codex_api::ResponsesClient;
use codex_api::RetryConfig;
use codex_http_client::HttpClientBuilder;
use codex_ios_platform::Change;
use codex_ios_platform::FileSystemBackend;
use codex_ios_platform::ScopedFiles;
use codex_protocol::models::ResponseItem;
use futures::StreamExt;
use http::HeaderMap;
use http::HeaderValue;
use serde_json::Value;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use uuid::Uuid;

pub(crate) struct RequestConfig {
    model: String,
    endpoint: String,
    authorization: HeaderValue,
}

impl RequestConfig {
    pub(crate) fn new(model: String, endpoint: String, api_key: String) -> anyhow::Result<Self> {
        let url = url::Url::parse(&endpoint)?;
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            bail!("API endpoint must be HTTPS without credentials, query or fragment");
        }
        if model.is_empty() || model.len() > 128 || api_key.is_empty() || api_key.len() > 4096 {
            bail!("enter a model identifier and API key in Settings");
        }
        let mut authorization = HeaderValue::from_str(&format!("Bearer {api_key}"))?;
        authorization.set_sensitive(true);
        Ok(Self {
            model,
            endpoint,
            authorization,
        })
    }
}

struct KeyAuth(HeaderValue);
impl AuthProvider for KeyAuth {
    fn add_auth_headers(&self, headers: &mut HeaderMap) {
        headers.insert(http::header::AUTHORIZATION, self.0.clone());
    }
}

pub(crate) async fn request_review(
    change: Change,
    project: Arc<ScopedFiles>,
    reviews: &Reviews,
    events: &mpsc::Sender<Value>,
    answer: Option<oneshot::Sender<Result<(), String>>>,
) -> anyhow::Result<()> {
    let id = Uuid::new_v4().to_string();
    let event = json!({"type":"review", "id":id, "change":change});
    {
        let mut pending = reviews
            .lock()
            .map_err(|_| anyhow::anyhow!("review state poisoned"))?;
        if pending.len() >= 8 {
            bail!("review queue full; resolve existing reviews first");
        }
        pending.insert(
            id,
            Review {
                change,
                project,
                answer,
            },
        );
    }
    events.send(event).await?;
    Ok(())
}

fn tools() -> Value {
    json!([
        {"type":"function", "name":"list_files", "description":"List an authorized project directory (max 100 results).", "parameters":{"type":"object", "properties":{"path":{"type":"string"}}, "required":["path"], "additionalProperties":false}, "strict":true},
        {"type":"function", "name":"read_file", "description":"Read UTF-8 text in the authorized project (max 2048 characters).", "parameters":{"type":"object", "properties":{"path":{"type":"string"}}, "required":["path"], "additionalProperties":false}, "strict":true},
        {"type":"function", "name":"propose_change", "description":"Propose full replacement text for a project file. User must approve the diff before saving.", "parameters":{"type":"object", "properties":{"path":{"type":"string"}, "after":{"type":"string"}}, "required":["path","after"], "additionalProperties":false}, "strict":true}
    ])
}

pub(crate) async fn run_turn(
    mut session: Session,
    prompt: String,
    request: RequestConfig,
    store: SessionStore,
    project: Option<Arc<ScopedFiles>>,
    reviews: Reviews,
    events: mpsc::Sender<Value>,
) -> anyhow::Result<Session> {
    if prompt.trim().is_empty() || prompt.len() > 8192 {
        bail!("prompt must contain 1–8192 UTF-8 bytes");
    }
    if session.messages.is_empty() {
        session.title = prompt.chars().take(/*n*/ 60).collect();
    }
    session.messages.push(Message {
        role: "user".into(),
        text: prompt.clone(),
    });
    session.items.push(
        json!({"type":"message", "role":"user", "content":[{"type":"input_text", "text":prompt}]}),
    );
    store.save(&session)?;
    events
        .send(json!({"type":"session", "session":session}))
        .await?;
    let client = ResponsesClient::new(
        ReqwestTransport::from_http_client(
            HttpClientBuilder::new()
                .without_redirects()
                .without_request_logging()
                .build_direct()?,
        ),
        Provider {
            name: "iOS".into(),
            base_url: request.endpoint,
            query_params: None,
            headers: HeaderMap::new(),
            retry: RetryConfig {
                max_attempts: 2,
                base_delay: Duration::from_millis(/*millis*/ 300),
                retry_429: true,
                retry_5xx: true,
                retry_transport: true,
            },
            stream_idle_timeout: Duration::from_secs(/*secs*/ 60),
        },
        Arc::new(KeyAuth(request.authorization)),
    );
    for _ in 0..12 {
        if serde_json::to_vec(&session.items)?.len() > MAX_CONTEXT_BYTES {
            bail!("context limit reached; start a new session");
        }
        let body = json!({"model":request.model, "input":session.items, "stream":true, "store":false, "include":["reasoning.encrypted_content"], "tools": if project.is_some() { tools() } else { json!([]) }});
        let mut stream = client
            .stream(
                body,
                HeaderMap::new(),
                Compression::None,
                /*turn_state*/ None,
            )
            .await?;
        let mut calls = Vec::new();
        let mut completed = false;
        let mut message_index: Option<usize> = None;
        let mut reasoning_bytes = 0;
        while let Some(event) = stream.next().await {
            match event? {
                ResponseEvent::OutputTextDelta(delta) => {
                    let index = *message_index.get_or_insert_with(|| {
                        session.messages.push(Message {
                            role: "assistant".into(),
                            text: String::new(),
                        });
                        session.messages.len() - 1
                    });
                    if session.messages[index].text.len() + delta.len() > 32_768 {
                        bail!("response exceeds mobile output limit");
                    }
                    session.messages[index].text.push_str(&delta);
                    // Checkpoint partial output before emitting it; app termination is recoverable.
                    store.save(&session)?;
                    events.send(json!({"type":"delta", "text":delta})).await?;
                }
                ResponseEvent::OutputItemDone(item) => {
                    let encoded = serde_json::to_value(&item)?;
                    session.items.push(encoded);
                    if let ResponseItem::FunctionCall {
                        name,
                        arguments,
                        call_id,
                        ..
                    } = item
                    {
                        calls.push((name, arguments, call_id));
                    }
                    store.save(&session)?;
                }
                ResponseEvent::Completed { .. } => completed = true,
                ResponseEvent::ReasoningSummaryDelta { delta, .. } => {
                    reasoning_bytes += delta.len();
                    if reasoning_bytes > 8192 {
                        bail!("reasoning summary exceeds mobile limit");
                    }
                    events
                        .send(json!({"type":"thinking", "text":delta}))
                        .await?;
                }
                ResponseEvent::Created { .. }
                | ResponseEvent::OutputItemAdded(_)
                | ResponseEvent::ServerModel(_)
                | ResponseEvent::ModelVerifications(_)
                | ResponseEvent::TurnModerationMetadata(_)
                | ResponseEvent::ServerReasoningIncluded(_)
                | ResponseEvent::ToolCallInputDelta { .. }
                | ResponseEvent::ReasoningSummaryDone { .. }
                | ResponseEvent::ReasoningContentDelta { .. }
                | ResponseEvent::ReasoningSummaryPartAdded { .. }
                | ResponseEvent::RateLimits(_)
                | ResponseEvent::ModelsEtag(_)
                | ResponseEvent::SafetyBuffering(_) => {}
            }
        }
        if !completed {
            bail!("stream ended before response.completed; partial output was saved");
        }
        if calls.is_empty() {
            store.save(&session)?;
            return Ok(session);
        }
        for (name, arguments, call_id) in calls {
            events
                .send(json!({"type":"tool", "name":name, "status":"running"}))
                .await?;
            let output = execute_tool(&name, &arguments, project.as_ref(), &reviews, &events)
                .await
                .unwrap_or_else(|error| format!("Tool error: {error}"));
            session
                .items
                .push(json!({"type":"function_call_output", "call_id":call_id, "output":output}));
            store.save(&session)?;
            events
                .send(json!({"type":"tool", "name":name, "status":"finished"}))
                .await?;
        }
    }
    bail!("mobile tool iteration limit reached (12); continue with a new prompt")
}

async fn execute_tool(
    name: &str,
    arguments: &str,
    project: Option<&Arc<ScopedFiles>>,
    reviews: &Reviews,
    events: &mpsc::Sender<Value>,
) -> anyhow::Result<String> {
    if arguments.len() > 70_000 {
        bail!("tool arguments exceed limit");
    }
    let args: Value = serde_json::from_str(arguments)?;
    let path = args["path"].as_str().context("missing path")?;
    let project = project.context("select a project first")?;
    match name {
        "list_files" => Ok(serde_json::to_string(
            &project
                .list(path)?
                .into_iter()
                .take(/*n*/ 100)
                .collect::<Vec<_>>(),
        )?),
        "read_file" => {
            let text = project.read(path)?;
            let mut limited: String = text.chars().take(/*n*/ 2048).collect();
            if limited.len() < text.len() {
                limited.push_str("\n[File truncated at 2048 characters]");
            }
            Ok(limited)
        }
        "propose_change" => {
            let change = project.prepare(
                path,
                args["after"]
                    .as_str()
                    .context("missing replacement text")?
                    .to_owned(),
            )?;
            let (answer, receive) = oneshot::channel();
            request_review(change, project.clone(), reviews, events, Some(answer)).await?;
            receive
                .await
                .context("review interrupted")?
                .map_err(anyhow::Error::msg)?;
            Ok("Reviewed change saved.".into())
        }
        _ => bail!("tool is unsupported in the mobile runtime"),
    }
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
