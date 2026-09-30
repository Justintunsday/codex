//! Embed the unchanged upstream agent with no desktop executor environment.
//! Native file tools use Core's dynamic-tool protocol and the existing review gate.
use crate::Reviews;
use crate::agent;
use crate::agent::RequestConfig;
use crate::session::Message;
use crate::session::Session;
use crate::session::SessionStore;
use anyhow::Context;
use anyhow::bail;
use codex_core::CodexAppsToolsCache;
use codex_core::CodexThread;
use codex_core::RolloutRecorder;
use codex_core::StartIfIdleSubmission;
use codex_core::StartThreadOptions;
use codex_core::ThreadManager;
use codex_core::TurnInputRequest;
use codex_core::config::ConfigBuilder;
use codex_core::config::ConfigOverrides;
use codex_exec_server::EnvironmentManager;
use codex_extension_api::LoadInstructionsFuture;
use codex_extension_api::LoadedUserInstructions;
use codex_extension_api::UserInstructionsProvider;
use codex_extension_api::empty_extension_registry;
use codex_features::Feature;
use codex_ios_platform::ScopedFiles;
use codex_login::AuthCredentialsStoreMode;
use codex_login::AuthKeyringBackendKind;
use codex_login::AuthManager;
use codex_model_provider_info::ModelProviderInfo;
use codex_protocol::ThreadId;
use codex_protocol::dynamic_tools::DynamicToolCallOutputContentItem;
use codex_protocol::dynamic_tools::DynamicToolFunctionSpec;
use codex_protocol::dynamic_tools::DynamicToolResponse;
use codex_protocol::dynamic_tools::DynamicToolSpec;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::SessionSource;
use codex_protocol::user_input::UserInput;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::path::Component;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

struct HostInstructions;
impl UserInstructionsProvider for HostInstructions {
    fn load_user_instructions(&self) -> LoadInstructionsFuture<'_> {
        Box::pin(async { LoadedUserInstructions::default() })
    }
}

/// Only the upstream in-memory credential store receives the Keychain copy.
struct EphemeralCredential(PathBuf);
impl Drop for EphemeralCredential {
    fn drop(&mut self) {
        let _ = codex_login::logout(
            &self.0,
            AuthCredentialsStoreMode::Ephemeral,
            AuthKeyringBackendKind::default(),
        );
    }
}

/// A panic or forced host cancellation still schedules upstream thread shutdown.
struct ShutdownGuard(Option<Arc<ThreadManager>>);
impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        if let Some(manager) = self.0.take()
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            runtime.spawn(async move {
                manager
                    .shutdown_all_threads_bounded(Duration::from_secs(/*secs*/ 5))
                    .await;
            });
        }
    }
}

pub(crate) struct CoreHost {
    pub store: SessionStore,
    pub project: Option<Arc<ScopedFiles>>,
    pub reviews: Reviews,
    pub events: mpsc::Sender<Value>,
    pub cancellation: CancellationToken,
}

pub(crate) async fn run_turn(
    mut session: Session,
    prompt: String,
    request: RequestConfig,
    host: CoreHost,
) -> anyhow::Result<Session> {
    let CoreHost {
        store,
        project,
        reviews,
        events,
        cancellation,
    } = host;
    if prompt.trim().is_empty() || prompt.len() > 8192 {
        bail!("prompt must contain 1–8192 UTF-8 bytes");
    }
    if cancellation.is_cancelled() {
        bail!("task cancelled");
    }
    let home = store.root.join("Core");
    std::fs::create_dir_all(&home)?;
    let home = home.canonicalize()?;
    let key = request
        .authorization
        .to_str()?
        .strip_prefix("Bearer ")
        .context("invalid credential")?;
    codex_login::login_with_api_key(
        &home,
        key,
        AuthCredentialsStoreMode::Ephemeral,
        AuthKeyringBackendKind::default(),
    )?;
    let _credential = EphemeralCredential(home.clone());
    let mut config = ConfigBuilder::default()
        .codex_home(home.clone())
        .harness_overrides(ConfigOverrides {
            model: Some(request.model),
            cwd: Some(home.clone()),
            ..Default::default()
        })
        .build()
        .await?;
    // Capability selection, not removal of upstream implementations. An app sandbox
    // has no shell, process host, desktop plugin service or privileged sandbox helper.
    for feature in [
        Feature::ShellTool,
        Feature::UnifiedExec,
        Feature::CodeMode,
        Feature::CodeModeHost,
        Feature::CodeModePrewarm,
        Feature::CodexHooks,
        Feature::ShellSnapshot,
        Feature::ShellSnapshotV2,
        Feature::NetworkProxy,
        Feature::MemoryTool,
        Feature::Apps,
        Feature::Worktrees,
        Feature::WebSearchRequest,
        Feature::WebSearchCached,
        Feature::StandaloneWebSearch,
        Feature::Collab,
        Feature::MultiAgentV2,
        Feature::ViewImage,
        Feature::RequestPermissionsTool,
        Feature::ApiKeyModelDiscovery,
    ] {
        config.features.disable(feature)?;
    }
    config.model_provider_id = "ios-native".into();
    config.model_provider = ModelProviderInfo {
        name: "iOS API connection".into(),
        base_url: Some(request.endpoint),
        requires_openai_auth: true,
        supports_websockets: false,
        request_max_retries: Some(2),
        stream_max_retries: Some(1),
        stream_idle_timeout_ms: Some(60_000),
        ..Default::default()
    };
    config.model_providers.insert(
        config.model_provider_id.clone(),
        config.model_provider.clone(),
    );
    let auth = Arc::new(
        AuthManager::new(
            home.clone(),
            /*enable_codex_api_key_env*/ false,
            AuthCredentialsStoreMode::Ephemeral,
            /*forced_chatgpt_workspace_id*/ None,
            /*chatgpt_base_url*/ None,
            AuthKeyringBackendKind::default(),
            config.auth_route_config(),
        )
        .await,
    );
    let environments = Arc::new(EnvironmentManager::without_environments(
        config.http_client_factory(),
    ));
    let manager = Arc::new(ThreadManager::new(
        &config,
        auth.clone(),
        codex_core::build_models_manager(&config, auth),
        CodexAppsToolsCache::default(),
        SessionSource::Custom("ios".into()),
        environments,
        empty_extension_registry(),
        Arc::new(HostInstructions),
        /*analytics_events_client*/ None,
        codex_core::passthrough_image_store(),
        codex_core::thread_store_from_config(&config, /*state_db*/ None),
        /*agent_graph_store*/ None,
        session.id.clone(),
        /*attestation_provider*/ None,
        /*external_time_provider*/ None,
    ));
    let mut shutdown = ShutdownGuard(Some(manager.clone()));
    let mut options = StartThreadOptions::new(config);
    options.allow_provider_model_fallback = true;
    options.environments = Some(Vec::new());
    if let Some(relative) = &session.core_rollout {
        let path = PathBuf::from(relative);
        if path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        {
            bail!("invalid core rollout path");
        }
        let path = home.join(path).canonicalize()?;
        if !path.starts_with(&home) || path.metadata()?.len() > 16 * 1024 * 1024 {
            bail!("core rollout is outside its container or exceeds the 16 MiB restore limit");
        }
        options.initial_history = RolloutRecorder::get_rollout_history(&path).await?;
    } else {
        options.reserved_thread_id = Some(ThreadId::from_string(&session.id)?);
    }
    if project.is_some() {
        options.dynamic_tools = agent::tools()
            .as_array()
            .context("tool definitions")?
            .iter()
            .map(|tool| {
                Ok(DynamicToolSpec::Function(DynamicToolFunctionSpec {
                    name: tool["name"].as_str().context("tool name")?.to_owned(),
                    description: tool["description"]
                        .as_str()
                        .context("tool description")?
                        .to_owned(),
                    input_schema: tool["parameters"].clone(),
                    defer_loading: false,
                }))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
    }
    let started = manager.start_thread(options).await?;
    let thread = started.thread;
    thread.ensure_rollout_materialized().await;
    let result = async {
        if cancellation.is_cancelled() { bail!("task cancelled"); }
        let path = match started.session_configured.rollout_path {
            Some(path) => path,
            None => codex_core::find_thread_path_by_id_str(&home, &session.id, /*state_db_ctx*/ None).await?.context("core rollout was not materialized")?,
        };
        session.core_rollout = Some(path.strip_prefix(&home)?.to_string_lossy().replace('\\', "/"));
        if session.messages.is_empty() { session.title = prompt.chars().take(/*n*/ 60).collect(); }
        session.messages.push(Message { role: "user".into(), text: prompt.clone() });
        store.save(&session)?;
        events.send(json!({"type":"session", "session":session})).await?;
        match thread.start_turn_if_idle(TurnInputRequest::user_input(vec![UserInput::Text { text: prompt, text_elements: Vec::new() }])).await? {
            StartIfIdleSubmission::Started { .. } => {}
            StartIfIdleSubmission::NotSubmitted { reason } => bail!("core declined the turn: {reason:?}"),
        }
        tokio::select! {
            result = drive(&thread, &mut session, &store, project.as_ref(), &reviews, &events) => result,
            _ = cancellation.cancelled() => Err(anyhow::anyhow!("task cancelled")),
        }
    }.await;
    // Shutdown runs on the host runtime and completes before FFI/lifecycle cancellation returns.
    let _ = thread.submit(Op::Interrupt).await;
    let flush = thread.flush_rollout().await;
    manager
        .shutdown_all_threads_bounded(Duration::from_secs(/*secs*/ 5))
        .await;
    shutdown.0 = None;
    result?;
    flush?;
    store.save(&session)?;
    Ok(session)
}

async fn drive(
    thread: &Arc<CodexThread>,
    session: &mut Session,
    store: &SessionStore,
    project: Option<&Arc<ScopedFiles>>,
    reviews: &Reviews,
    events: &mpsc::Sender<Value>,
) -> anyhow::Result<()> {
    let mut messages: HashMap<String, usize> = HashMap::new();
    let mut summary_bytes = 0;
    let mut tool_calls = 0;
    loop {
        let event = thread.next_event().await?;
        // Typed variants carry user-visible payloads. All remaining protocol variants
        // are reduced to a bounded type label; raw/private reasoning is never displayed.
        match event.msg {
            EventMsg::AgentMessageContentDelta(delta) => {
                let index = *messages.entry(delta.item_id).or_insert_with(|| {
                    session.messages.push(Message {
                        role: "assistant".into(),
                        text: String::new(),
                    });
                    session.messages.len() - 1
                });
                if session.messages[index].text.len() + delta.delta.len() > 32_768 {
                    bail!("response exceeds mobile display limit");
                }
                session.messages[index].text.push_str(&delta.delta);
                store.save(session)?;
                events
                    .send(json!({"type":"delta", "text":delta.delta}))
                    .await?;
            }
            EventMsg::ItemCompleted(completed) => {
                if let TurnItem::AgentMessage(message) = completed.item {
                    let text: String = message
                        .content
                        .into_iter()
                        .map(|content| match content {
                            AgentMessageContent::Text { text } => text,
                        })
                        .collect();
                    if text.len() > 32_768 {
                        bail!("response exceeds mobile display limit");
                    }
                    let index = *messages.entry(message.id).or_insert_with(|| {
                        session.messages.push(Message {
                            role: "assistant".into(),
                            text: String::new(),
                        });
                        session.messages.len() - 1
                    });
                    if session.messages[index].text != text {
                        session.messages[index].text = text;
                        store.save(session)?;
                        events
                            .send(json!({"type":"session", "session":session}))
                            .await?;
                    }
                }
            }
            EventMsg::ReasoningContentDelta(delta) => {
                summary_bytes += delta.delta.len();
                if summary_bytes > 8192 {
                    bail!("reasoning summary exceeds mobile display limit");
                }
                events
                    .send(json!({"type":"thinking", "text":delta.delta}))
                    .await?;
            }
            EventMsg::DynamicToolCallRequest(call) => {
                tool_calls += 1;
                if tool_calls > 12 {
                    bail!("mobile tool call limit reached (12)");
                }
                events
                    .send(json!({"type":"tool", "name":call.tool, "status":"running"}))
                    .await?;
                let result = agent::execute_tool(
                    &call.tool,
                    &serde_json::to_string(&call.arguments)?,
                    project,
                    reviews,
                    events,
                )
                .await;
                let success = result.is_ok();
                let output = result.unwrap_or_else(|error| format!("Tool error: {error}"));
                let truncated = output.chars().count() > 1800;
                let mut output: String = output.chars().take(/*n*/ 1800).collect();
                if truncated {
                    output.push_str("\n[Tool output truncated at 1800 characters]");
                }
                thread
                    .submit(Op::DynamicToolResponse {
                        id: call.call_id,
                        response: DynamicToolResponse {
                            success,
                            content_items: vec![DynamicToolCallOutputContentItem::InputText {
                                text: output,
                            }],
                        },
                    })
                    .await?;
                thread.flush_rollout().await?;
                events
                    .send(json!({"type":"tool", "name":call.tool, "status":"finished"}))
                    .await?;
            }
            EventMsg::TurnComplete(_) => {
                thread.flush_rollout().await?;
                return Ok(());
            }
            EventMsg::TurnAborted(_) => bail!("core turn interrupted; saved history is retained"),
            EventMsg::Error(error) => bail!("{}", error.message),
            EventMsg::ExecApprovalRequest(_)
            | EventMsg::ApplyPatchApprovalRequest(_)
            | EventMsg::RequestPermissions(_)
            | EventMsg::RequestUserInput(_)
            | EventMsg::ElicitationRequest(_) => {
                bail!(
                    "this core request requires a backend unavailable in the installed iOS capability set"
                );
            }
            EventMsg::ShutdownComplete => bail!("core stopped before turn completion"),
            other => {
                events
                    .send(json!({"type":"coreEvent", "name":other.to_string()}))
                    .await?;
            }
        }
    }
}

#[cfg(test)]
#[path = "upstream_tests.rs"]
mod tests;
