//! Host the upstream Codex agent and a legacy Responses adapter through a bounded
//! event bridge. iOS capabilities are selected without changing desktop core.
mod agent;
mod session;
mod upstream;

use anyhow::Context;
use anyhow::bail;
use codex_ios_platform::Change;
use codex_ios_platform::FileSystemBackend;
use codex_ios_platform::PlatformCapabilities;
use codex_ios_platform::ScopedFiles;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;
use session::Session;
use session::SessionStore;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentEngine {
    #[default]
    CodexCore,
    ResponsesAdapter,
}

pub const MAX_COMMAND_BYTES: usize = 131_072;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Init {
    pub home: PathBuf,
}

#[derive(Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Command {
    CreateSession,
    RestoreSession {
        id: String,
    },
    ListSessions,
    OpenProject {
        path: PathBuf,
    },
    ListFiles {
        path: String,
    },
    ReadFile {
        path: String,
    },
    PreviewChange {
        path: String,
        after: String,
    },
    Review {
        id: String,
        decision: Decision,
    },
    SendPrompt {
        prompt: String,
        model: String,
        endpoint: String,
        api_key: String,
        #[serde(default)]
        engine: AgentEngine,
    },
    Cancel,
    Lifecycle {
        state: LifeCycle,
    },
    Diagnostics,
    Shutdown,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Decision {
    Approve,
    Reject,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LifeCycle {
    Foreground,
    Background,
    MemoryPressure,
}

pub(crate) struct Review {
    change: Change,
    project: Arc<ScopedFiles>,
    answer: Option<oneshot::Sender<Result<(), String>>>,
}

pub(crate) type Reviews = Arc<Mutex<HashMap<String, Review>>>;

pub async fn run(
    init: Init,
    mut commands: mpsc::Receiver<Command>,
    events: mpsc::Sender<Value>,
) -> anyhow::Result<()> {
    let store = SessionStore::new(init.home)?;
    let mut session: Option<Session> = None;
    let mut project: Option<Arc<ScopedFiles>> = None;
    let reviews: Reviews = Arc::new(Mutex::new(HashMap::new()));
    let mut turn: Option<JoinHandle<anyhow::Result<Session>>> = None;
    let mut cancellation = CancellationToken::new();
    events
        .send(json!({"type":"ready", "abi":1, "capabilities":PlatformCapabilities::default()}))
        .await?;
    loop {
        tokio::select! {
            result = async {
                match turn.as_mut() {
                    Some(turn) => turn.await,
                    None => std::future::pending().await,
                }
            } => {
                turn = None;
                match result {
                    Ok(Ok(updated)) => session = Some(updated),
                    Ok(Err(error)) => {
                        events.send(json!({"type":"error", "message":error.to_string()})).await?;
                        if let Some(current) = &session { session = Some(store.load(&current.id)?); }
                    }
                    Err(error) => {
                        events.send(json!({"type":"error", "message":format!("runtime task failed: {error}")})).await?;
                        if let Some(current) = &session { session = Some(store.load(&current.id)?); }
                    }
                }
                reviews.lock().map_err(|_| anyhow::anyhow!("review state poisoned"))?.clear();
                if let Some(current) = &session { events.send(json!({"type":"session", "session":current})).await?; }
                events.send(json!({"type":"status", "status":"idle"})).await?;
            }
            command = commands.recv() => {
                let Some(command) = command else { break; };
                if matches!(command, Command::Shutdown) { break; }
                let result: anyhow::Result<()> = async {
                    match command {
                        Command::CreateSession | Command::RestoreSession { .. } | Command::OpenProject { .. }
                            if turn.is_some() => bail!("cancel the active task before changing sessions or projects"),
                        Command::CreateSession => {
                            let created = Session::new();
                            store.save(&created)?;
                            events.send(json!({"type":"session", "session":created})).await?;
                            session = Some(created);
                        }
                        Command::RestoreSession { id } => {
                            let restored = store.load(&id)?;
                            events.send(json!({"type":"session", "session":restored})).await?;
                            session = Some(restored);
                        }
                        Command::ListSessions => {
                            events.send(json!({"type":"sessions", "sessions":store.list()?})).await?;
                        }
                        Command::OpenProject { path } => {
                            project = Some(Arc::new(ScopedFiles::new(&path)?));
                            events.send(json!({"type":"project", "path":path})).await?;
                        }
                        Command::ListFiles { path } => {
                            let files = project.as_ref().context("select a project first")?.list(&path)?;
                            events.send(json!({"type":"files", "path":path, "files":files})).await?;
                        }
                        Command::ReadFile { path } => {
                            let text = project.as_ref().context("select a project first")?.read(&path)?;
                            events.send(json!({"type":"file", "path":path, "text":text})).await?;
                        }
                        Command::PreviewChange { path, after } => {
                            let files = project.as_ref().context("select a project first")?.clone();
                            let change = files.prepare(&path, after)?;
                            agent::request_review(change, files, &reviews, &events, /*answer*/ None).await?;
                        }
                        Command::Review { id, decision } => {
                            let review = reviews.lock().map_err(|_| anyhow::anyhow!("review state poisoned"))?.remove(&id).context("review is no longer active")?;
                            let result = match decision {
                                Decision::Approve => review.project.apply(&review.change).map_err(|error| error.to_string()),
                                Decision::Reject => Err("change rejected by user".to_owned()),
                            };
                            let message = match &result { Ok(()) => "change saved".to_owned(), Err(message) => message.clone() };
                            if let Some(answer) = review.answer { let _ = answer.send(result); }
                            events.send(json!({"type":"reviewResolved", "id":id, "message":message})).await?;
                        }
                        Command::SendPrompt { prompt, model, endpoint, api_key, engine } => {
                            if turn.is_some() { bail!("a task is already running"); }
                            let mut current = session.as_ref().context("create a session first")?.clone();
                            if current.engine.is_some_and(|previous| previous != engine) {
                                bail!("start a new session before changing engines");
                            }
                            if current.engine.is_none() && !current.items.is_empty() && engine == AgentEngine::CodexCore {
                                bail!("this legacy session uses the Responses adapter; choose that engine or create a new session");
                            }
                            current.engine = Some(engine);
                            let request = agent::RequestConfig::new(model, endpoint, api_key)?;
                            store.save(&current)?;
                            cancellation = CancellationToken::new();
                            let task_store = store.clone();
                            let task_project = project.clone();
                            let task_reviews = reviews.clone();
                            let task_events = events.clone();
                            let task_cancellation = cancellation.clone();
                            let task = async move {
                                match engine {
                                    AgentEngine::CodexCore => upstream::run_turn(current, prompt, request, upstream::CoreHost {
                                        store: task_store, project: task_project, reviews: task_reviews, events: task_events, cancellation: task_cancellation,
                                    }).await,
                                    AgentEngine::ResponsesAdapter => {
                                        tokio::select! {
                                            result = agent::run_turn(current, prompt, request, task_store, task_project, task_reviews, task_events) => result,
                                            _ = task_cancellation.cancelled() => Err(anyhow::anyhow!("task cancelled")),
                                        }
                                    }
                                }
                            };
                            // The spawned future owns every state value; no actor borrow crosses tasks.
                            turn = Some(tokio::spawn(task));
                            events.send(json!({"type":"status", "status":"working"})).await?;
                        }
                        Command::Cancel | Command::Lifecycle { state: LifeCycle::Background | LifeCycle::MemoryPressure } => {
                            if let Some(active) = turn.take() {
                                cancellation.cancel();
                                reviews.lock().map_err(|_| anyhow::anyhow!("review state poisoned"))?.clear();
                                let mut active = active;
                                if tokio::time::timeout(std::time::Duration::from_secs(/*secs*/ 10), &mut active).await.is_err() {
                                    active.abort();
                                    let _ = active.await;
                                }
                                if let Some(current) = &session {
                                    let recovered = store.load(&current.id)?;
                                    events.send(json!({"type":"session", "session":recovered})).await?;
                                    session = Some(recovered);
                                }
                            }
                            reviews.lock().map_err(|_| anyhow::anyhow!("review state poisoned"))?.clear();
                            events.send(json!({"type":"status", "status":"idle"})).await?;
                        }
                        Command::Diagnostics => {
                            events.send(json!({"type":"diagnostics", "capabilities":PlatformCapabilities::default(), "arch":std::env::consts::ARCH, "os":std::env::consts::OS})).await?;
                        }
                        Command::Lifecycle { state: LifeCycle::Foreground } => {}
                        Command::Shutdown => {}
                    }
                    Ok(())
                }.await;
                if let Err(error) = result { events.send(json!({"type":"error", "message":error.to_string()})).await?; }
            }
        }
    }
    if let Some(active) = turn {
        cancellation.cancel();
        reviews
            .lock()
            .map_err(|_| anyhow::anyhow!("review state poisoned"))?
            .clear();
        let mut active = active;
        if tokio::time::timeout(std::time::Duration::from_secs(/*secs*/ 10), &mut active)
            .await
            .is_err()
        {
            active.abort();
            let _ = active.await;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
