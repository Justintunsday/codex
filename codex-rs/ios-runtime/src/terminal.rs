//! Manual execution through the upstream remote backend. No iOS subprocess is started.
use anyhow::Context;
use anyhow::bail;
use codex_exec_server::ByteChunk;
use codex_exec_server::EnvironmentManager;
use codex_exec_server::ExecParams;
use codex_exec_server::ExecProcess;
use codex_exec_server::ExecProcessEvent;
use codex_exec_server::ProcessId;
use codex_exec_server::ProcessSignal;
use codex_exec_server::RemoteEnvironmentOptions;
use codex_exec_server::WriteStatus;
use codex_http_client::DestinationPolicy;
use codex_http_client::HttpClientFactory;
use codex_http_client::NetworkPolicyController;
use codex_http_client::OutboundProxyPolicy;
use codex_utils_path_uri::LegacyAppPathString;
use futures::FutureExt;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TerminalBackend {
    Remote,
    EnhancedHelper,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalConnection {
    pub endpoint: String,
    pub token: String,
    pub directory: LegacyAppPathString,
    pub backend: TerminalBackend,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TerminalProgram {
    Shell,
    Program { argv: Vec<String> },
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TerminalCommand {
    Start {
        connection: TerminalConnection,
        program: TerminalProgram,
    },
    Write {
        chunk: ByteChunk,
    },
    Interrupt,
    Stop,
}

type ProcessSlot = Arc<Mutex<Option<Arc<dyn ExecProcess>>>>;

#[derive(Default)]
pub(crate) struct TerminalHost {
    task: Option<JoinHandle<()>>,
    cancellation: CancellationToken,
    process: ProcessSlot,
}

impl TerminalHost {
    pub async fn handle(
        &mut self,
        command: TerminalCommand,
        events: &mpsc::Sender<Value>,
    ) -> anyhow::Result<()> {
        match command {
            TerminalCommand::Start {
                connection,
                program,
            } => {
                let factory = connection.factory()?;
                if let TerminalProgram::Program { argv } = &program
                    && (argv.is_empty()
                        || argv.len() > 64
                        || argv[0].is_empty()
                        || argv.iter().map(String::len).sum::<usize>() > 8192)
                {
                    bail!("enter a program and up to 64 arguments (8 KiB total)");
                }
                self.stop().await;
                self.cancellation = CancellationToken::new();
                let cancellation = self.cancellation.clone();
                let process = self.process.clone();
                let events = events.clone();
                events
                    .send(json!({"type":"terminalState", "status":"starting"}))
                    .await?;
                self.task = Some(tokio::spawn(async move {
                    let token = connection.token.clone();
                    let result = AssertUnwindSafe(run(
                        connection,
                        program,
                        factory,
                        &process,
                        &cancellation,
                        &events,
                    ))
                    .catch_unwind()
                    .await
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("terminal task panicked")));
                    if let Err(error) = result {
                        let message = error.to_string().replace(&token, "<redacted>");
                        let _ = events.send(json!({"type":"terminalState", "status":"stopped", "message":message.chars().take(2048).collect::<String>()})).await;
                    }
                    if let Ok(mut slot) = process.lock() {
                        slot.take();
                    }
                }));
            }
            TerminalCommand::Write { chunk } => {
                if chunk.0.len() > 4096 {
                    bail!("terminal input exceeds 4 KiB; send smaller chunks");
                }
                let process = self
                    .process
                    .lock()
                    .map_err(|_| anyhow::anyhow!("terminal state poisoned"))?
                    .clone()
                    .context("terminal is not running")?;
                let response =
                    tokio::time::timeout(Duration::from_secs(/*secs*/ 5), process.write(chunk.0))
                        .await??;
                match response.status {
                    WriteStatus::Accepted => {}
                    WriteStatus::UnknownProcess => bail!("terminal process no longer exists"),
                    WriteStatus::StdinClosed => bail!("terminal input is closed"),
                    WriteStatus::Starting => bail!("terminal is still starting"),
                }
            }
            TerminalCommand::Interrupt => {
                let process = self
                    .process
                    .lock()
                    .map_err(|_| anyhow::anyhow!("terminal state poisoned"))?
                    .clone()
                    .context("terminal is not running")?;
                tokio::time::timeout(
                    Duration::from_secs(/*secs*/ 5),
                    process.signal(ProcessSignal::Interrupt),
                )
                .await??;
            }
            TerminalCommand::Stop => {
                self.stop().await;
                events
                    .send(json!({"type":"terminalState", "status":"stopped"}))
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn stop(&mut self) {
        self.cancellation.cancel();
        if let Some(mut task) = self.task.take()
            && tokio::time::timeout(Duration::from_secs(/*secs*/ 6), &mut task)
                .await
                .is_err()
        {
            task.abort();
            let _ = task.await;
        }
        if let Ok(mut slot) = self.process.lock() {
            slot.take();
        }
    }
}

impl Drop for TerminalHost {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl TerminalConnection {
    fn factory(&self) -> anyhow::Result<HttpClientFactory> {
        let url = url::Url::parse(&self.endpoint)?;
        let host = url.host().context("terminal host")?;
        let loopback = match &host {
            url::Host::Ipv4(address) => address.is_loopback(),
            url::Host::Ipv6(address) => address.is_loopback(),
            url::Host::Domain(_) => false,
        };
        let secure = url.scheme() == "wss";
        #[cfg(test)]
        let secure = secure || (url.scheme() == "ws" && loopback);
        if !secure
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || self.endpoint.len() > 2048
            || self.token.is_empty()
            || self.token.len() > 8192
            || self.token.contains(['\r', '\n'])
        {
            bail!("use a WSS exec-server URL and a separate connection token");
        }
        if matches!(self.backend, TerminalBackend::EnhancedHelper) && !loopback {
            bail!(
                "enhanced mode requires a separately installed helper on this device's loopback address"
            );
        }
        if self.directory.as_str().len() > 8192
            || (!self.directory.as_str().is_empty()
                && self.directory.to_inferred_path_uri().is_none())
        {
            bail!("terminal directory must be an absolute path on the executor");
        }
        let network = NetworkPolicyController::default();
        let policy = DestinationPolicy::Restricted {
            allowed_hosts: [url
                .host_str()
                .context("terminal host")?
                .trim_end_matches('.')
                .to_owned()]
            .into(),
        };
        #[cfg(test)]
        let policy = if url.scheme() == "ws" && loopback {
            DestinationPolicy::Unrestricted
        } else {
            policy
        };
        if !network.publish(network.policy().revision(), policy) {
            bail!("terminal network policy changed");
        }
        Ok(HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault)
            .with_network_policy(network.policy()))
    }
}

struct TerminateOnDrop(Option<Arc<dyn ExecProcess>>);
impl Drop for TerminateOnDrop {
    fn drop(&mut self) {
        if let Some(process) = self.0.take()
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            runtime.spawn(async move {
                let _ = tokio::time::timeout(Duration::from_secs(/*secs*/ 3), process.terminate())
                    .await;
            });
        }
    }
}

async fn run(
    connection: TerminalConnection,
    program: TerminalProgram,
    factory: HttpClientFactory,
    slot: &ProcessSlot,
    cancellation: &CancellationToken,
    events: &mpsc::Sender<Value>,
) -> anyhow::Result<()> {
    let manager = EnvironmentManager::without_environments(factory);
    manager.upsert_environment_with_options(
        "ios-terminal".into(),
        RemoteEnvironmentOptions {
            exec_server_url: connection.endpoint,
            connect_timeout: Some(Duration::from_secs(/*secs*/ 10)),
            http_headers: HashMap::from([(
                "authorization".into(),
                format!("Bearer {}", connection.token),
            )]),
        },
    )?;
    let environment = manager
        .get_environment("ios-terminal")
        .context("terminal environment")?;
    let info = tokio::select! {
        _ = cancellation.cancelled() => bail!("terminal cancelled before connection"),
        result = tokio::time::timeout(Duration::from_secs(/*secs*/ 15), environment.info()) => result??,
    };
    if matches!(connection.backend, TerminalBackend::EnhancedHelper)
        && info.platform_os.as_deref() != Some("ios")
    {
        bail!("enhanced helper endpoint does not report an iOS executor");
    }
    let cwd = if connection.directory.as_str().is_empty() {
        info.cwd
            .context("executor did not report a working directory")?
    } else {
        connection
            .directory
            .to_inferred_path_uri()
            .context("executor directory")?
    };
    let (argv, tty) = match program {
        TerminalProgram::Shell => (vec![info.shell.path], true),
        TerminalProgram::Program { argv } => (argv, false),
    };
    if argv[0].is_empty()
        || argv[0].len() > 8192
        || argv.iter().any(|argument| argument.contains('\0'))
    {
        bail!("executor program is empty, invalid or exceeds mobile limits");
    }
    let backend = environment.get_exec_backend();
    let params = ExecParams {
        process_id: ProcessId::new(uuid::Uuid::new_v4().to_string()),
        metadata: None,
        argv,
        cwd,
        env_policy: None,
        shell_snapshot: None,
        env: HashMap::from([("TERM".into(), "xterm-256color".into())]),
        tty,
        pipe_stdin: true,
        arg0: None,
        sandbox: None,
        enforce_managed_network: false,
        managed_network: None,
        network_proxy: None,
    };
    let started = tokio::select! {
        _ = cancellation.cancelled() => bail!("terminal cancelled before launch"),
        result = tokio::time::timeout(Duration::from_secs(/*secs*/ 15), backend.start(params)) => result??,
    };
    let process = started.process;
    let mut guard = TerminateOnDrop(Some(process.clone()));
    *slot
        .lock()
        .map_err(|_| anyhow::anyhow!("terminal state poisoned"))? = Some(process.clone());
    let mut output = process.subscribe_events();
    events.send(json!({"type":"terminalState", "status":"running", "message":info.platform_os.unwrap_or_else(|| "remote executor".into())})).await?;
    let mut after = None;
    let mut bytes = 0;
    let mut sequence = 0;
    let mut exited = false;
    loop {
        let event = tokio::select! {
            _ = cancellation.cancelled() => {
                tokio::time::timeout(Duration::from_secs(/*secs*/ 3), process.terminate()).await??;
                guard.0 = None;
                bail!("terminal cancelled");
            }
            event = output.recv() => event,
        };
        match event {
            Ok(ExecProcessEvent::Output(chunk)) => {
                if after.is_some_and(|seq| chunk.seq <= seq) {
                    continue;
                }
                after = Some(chunk.seq);
                let data = chunk.chunk.0;
                bytes += data.len();
                if bytes > 1_048_576 {
                    bail!("terminal reached the 1 MiB output limit; start a new terminal");
                }
                for chunk in data.chunks(4096) {
                    sequence += 1;
                    events.send(json!({"type":"terminalOutput", "sequence":sequence, "chunk":ByteChunk(chunk.to_vec())})).await?;
                }
            }
            Ok(ExecProcessEvent::Exited { exit_code, .. }) => {
                exited = true;
                events.send(json!({"type":"terminalState", "status":"exited", "message":format!("Process exited with code {exit_code}")})).await?;
            }
            Ok(ExecProcessEvent::Closed { .. }) => {
                guard.0 = None;
                if !exited {
                    events.send(json!({"type":"terminalState", "status":"stopped", "message":"Executor closed the process stream"})).await?;
                }
                return Ok(());
            }
            Ok(ExecProcessEvent::Failed(message)) => bail!("terminal connection failed: {message}"),
            Err(broadcast::error::RecvError::Closed) => bail!("terminal output stream closed"),
            Err(broadcast::error::RecvError::Lagged(_)) => {
                bail!("terminal receiver exceeded its output capacity; start a new terminal")
            }
        }
    }
}

#[cfg(test)]
#[path = "terminal_tests.rs"]
mod tests;
