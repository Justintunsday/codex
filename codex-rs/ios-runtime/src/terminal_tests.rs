use super::*;
use futures::SinkExt;
use futures::StreamExt;
use pretty_assertions::assert_eq;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

async fn next_event(events: &mut mpsc::Receiver<Value>) -> anyhow::Result<Value> {
    tokio::time::timeout(Duration::from_secs(/*secs*/ 10), events.recv())
        .await?
        .context("terminal event")
}

#[derive(Clone, Copy)]
enum Fixture {
    Interactive,
    CompletedProgram,
}

async fn fixture(
    kind: Fixture,
) -> anyhow::Result<(
    std::net::SocketAddr,
    mpsc::Receiver<Value>,
    JoinHandle<anyhow::Result<()>>,
)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (requests, captured) = mpsc::channel(/*buffer*/ 16);
    let peer = tokio::spawn(async move {
        let (socket, _) = listener.accept().await?;
        let mut socket = tokio_tungstenite::accept_hdr_async(
            socket,
            |request: &tokio_tungstenite::tungstenite::handshake::server::Request, response| {
                assert_eq!(
                    request
                        .headers()
                        .get("authorization")
                        .and_then(|value| value.to_str().ok()),
                    Some("Bearer fixture-terminal-token")
                );
                Ok(response)
            },
        )
        .await?;
        while let Some(message) = socket.next().await {
            let message = message?;
            if !message.is_text() {
                continue;
            }
            let request: Value = serde_json::from_str(message.to_text()?)?;
            if request["id"].is_null() {
                continue;
            }
            let method = request["method"].as_str().context("method")?;
            requests.send(request.clone()).await?;
            let result = match method {
                "initialize" => {
                    json!({"sessionId":uuid::Uuid::new_v4().to_string(), "environmentInfo":{
                        "shell":{"name":"fixture", "path":"fixture-shell"}, "cwd":"file:///fixture",
                        "platformOs":"linux", "prependPathDirs":[], "capabilities":{}
                    }})
                }
                "process/start" => json!({"processId":request["params"]["processId"]}),
                "process/write" => json!({"status":"accepted"}),
                "process/signal" => json!({}),
                "process/terminate" => json!({"running":false}),
                other => bail!("unexpected fixture method {other}"),
            };
            socket
                .send(Message::Text(
                    json!({"id":request["id"], "result":result})
                        .to_string()
                        .into(),
                ))
                .await?;
            if method == "process/start" {
                socket
                    .send(Message::Text(
                        json!({"method":"process/output", "params":{
                            "processId":request["params"]["processId"], "seq":1, "stream":"pty",
                            "chunk":ByteChunk(b"\x1b[31mHello\x1b[0m\r\n".to_vec())
                        }})
                        .to_string()
                        .into(),
                    ))
                    .await?;
                if matches!(kind, Fixture::CompletedProgram) {
                    for (method, params) in [
                        (
                            "process/exited",
                            json!({"processId":request["params"]["processId"], "seq":2, "exitCode":7}),
                        ),
                        (
                            "process/closed",
                            json!({"processId":request["params"]["processId"], "seq":3}),
                        ),
                    ] {
                        socket
                            .send(Message::Text(
                                json!({"method":method, "params":params}).to_string().into(),
                            ))
                            .await?;
                    }
                    break;
                }
            }
            if method == "process/terminate" {
                break;
            }
        }
        Ok::<_, anyhow::Error>(())
    });
    Ok((address, captured, peer))
}

#[tokio::test]
async fn upstream_terminal_streams_raw_pty_bytes_accepts_input_and_terminates() -> anyhow::Result<()>
{
    let (address, mut captured, peer) = fixture(Fixture::Interactive).await?;
    let (events, mut output) = mpsc::channel(/*buffer*/ 16);
    let mut host = TerminalHost::default();
    host.handle(
        TerminalCommand::Start {
            connection: TerminalConnection {
                endpoint: format!("ws://{address}"),
                token: "fixture-terminal-token".into(),
                directory: LegacyAppPathString::from_string(""),
                backend: TerminalBackend::Remote,
            },
            program: TerminalProgram::Shell,
        },
        &events,
    )
    .await?;
    assert_eq!(
        next_event(&mut output).await?,
        json!({"type":"terminalState", "status":"starting"})
    );
    assert_eq!(
        next_event(&mut output).await?,
        json!({"type":"terminalState", "status":"running", "message":"linux"})
    );
    let frame = next_event(&mut output).await?;
    insta::assert_snapshot!(
        "native_terminal_frame",
        serde_json::to_string_pretty(&frame)?
    );
    let bytes: ByteChunk = serde_json::from_value(frame["chunk"].clone())?;
    assert_eq!(bytes.0, b"\x1b[31mHello\x1b[0m\r\n".to_vec());
    host.handle(
        TerminalCommand::Write {
            chunk: ByteChunk(b"pwd\n".to_vec()),
        },
        &events,
    )
    .await?;
    host.handle(TerminalCommand::Interrupt, &events).await?;
    host.handle(TerminalCommand::Stop, &events).await?;
    let mut methods = Vec::new();
    while let Some(request) =
        tokio::time::timeout(Duration::from_secs(/*secs*/ 10), captured.recv()).await?
    {
        methods.push(request["method"].as_str().context("method")?.to_owned());
        if request["method"] == "process/start" {
            assert_eq!(
                json!({"argv":request["params"]["argv"], "cwd":request["params"]["cwd"], "tty":request["params"]["tty"]}),
                json!({"argv":["fixture-shell"], "cwd":"file:///fixture", "tty":true})
            );
        }
        if request["method"] == "process/write" {
            let bytes: ByteChunk = serde_json::from_value(request["params"]["chunk"].clone())?;
            assert_eq!(bytes.0, b"pwd\n".to_vec());
        }
    }
    assert_eq!(
        methods,
        vec![
            "initialize",
            "process/start",
            "process/write",
            "process/signal",
            "process/terminate"
        ]
    );
    tokio::time::timeout(Duration::from_secs(/*secs*/ 10), peer).await???;
    Ok(())
}

#[tokio::test]
async fn terminal_program_preserves_arguments_and_reports_exit_before_closing() -> anyhow::Result<()>
{
    let (address, mut captured, peer) = fixture(Fixture::CompletedProgram).await?;
    let (events, mut output) = mpsc::channel(/*buffer*/ 16);
    let mut host = TerminalHost::default();
    host.handle(
        TerminalCommand::Start {
            connection: TerminalConnection {
                endpoint: format!("ws://{address}"),
                token: "fixture-terminal-token".into(),
                directory: LegacyAppPathString::from_string("/project"),
                backend: TerminalBackend::Remote,
            },
            program: TerminalProgram::Program {
                argv: vec!["fixture-program".into(), "argument with spaces".into()],
            },
        },
        &events,
    )
    .await?;
    assert_eq!(
        next_event(&mut output).await?,
        json!({"type":"terminalState", "status":"starting"})
    );
    assert_eq!(
        next_event(&mut output).await?,
        json!({"type":"terminalState", "status":"running", "message":"linux"})
    );
    assert_eq!(next_event(&mut output).await?["type"], "terminalOutput");
    assert_eq!(
        next_event(&mut output).await?,
        json!({"type":"terminalState", "status":"exited", "message":"Process exited with code 7"})
    );
    tokio::time::timeout(Duration::from_secs(/*secs*/ 10), peer).await???;
    if let Some(task) = host.task.take() {
        tokio::time::timeout(Duration::from_secs(/*secs*/ 10), task).await??;
    }
    let _ = captured.recv().await.context("initialize")?;
    let request = captured.recv().await.context("program launch")?;
    assert_eq!(
        json!({"argv":request["params"]["argv"], "cwd":request["params"]["cwd"], "tty":request["params"]["tty"]}),
        json!({"argv":["fixture-program", "argument with spaces"], "cwd":"file:///project", "tty":false})
    );
    assert_eq!(output.try_recv(), Err(mpsc::error::TryRecvError::Empty));
    Ok(())
}

#[test]
fn terminal_policy_confines_enhanced_helpers_and_rejects_credentials_in_urls() {
    for endpoint in [
        "ws://example.com/exec",
        "wss://user:password@example.com/exec",
        "wss://example.com/exec?token=secret",
    ] {
        let connection = TerminalConnection {
            endpoint: endpoint.into(),
            token: "fixture-token".into(),
            directory: LegacyAppPathString::from_string("/project"),
            backend: TerminalBackend::Remote,
        };
        assert!(connection.factory().is_err());
    }
    let connection = TerminalConnection {
        endpoint: "wss://example.com/exec".into(),
        token: "fixture-token".into(),
        directory: LegacyAppPathString::from_string("C:\\project"),
        backend: TerminalBackend::EnhancedHelper,
    };
    assert!(connection.factory().is_err());
    let connection = TerminalConnection {
        backend: TerminalBackend::Remote,
        ..connection
    };
    assert!(connection.factory().is_ok());
}
