use anyhow::Context;
use anyhow::bail;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;
use std::io::Read;
use std::io::Write;
use std::path::PathBuf;
use uuid::Uuid;

pub(crate) const MAX_CONTEXT_BYTES: usize = 32_768;
const MAX_SESSION_BYTES: u64 = 262_144;
const MAX_SESSIONS: usize = 200;

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Message {
    pub role: String,
    pub text: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Session {
    pub id: String,
    pub title: String,
    pub messages: Vec<Message>,
    pub items: Vec<Value>,
}

impl Session {
    pub(crate) fn new() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            title: "New session".into(),
            messages: Vec::new(),
            items: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct SessionStore {
    root: PathBuf,
}

impl SessionStore {
    pub(crate) fn new(root: PathBuf) -> anyhow::Result<Self> {
        if !root.is_absolute() {
            bail!("Application Support path must be absolute");
        }
        std::fs::create_dir_all(&root)?;
        Ok(Self {
            root: root.canonicalize()?,
        })
    }

    pub(crate) fn save(&self, session: &Session) -> anyhow::Result<()> {
        Uuid::parse_str(&session.id)?;
        let path = self.root.join(format!("{}.json", session.id));
        if !path.try_exists()? && std::fs::read_dir(&self.root)?.count() >= MAX_SESSIONS {
            bail!("session limit reached (200); export or remove old sessions first");
        }
        if serde_json::to_vec(&session.items)?.len() > MAX_CONTEXT_BYTES {
            bail!("session context limit reached; start a new session");
        }
        for item in &session.items {
            if serde_json::to_vec(item)?.len() > 8192 {
                bail!("individual context item exceeds the 8 KiB mobile limit");
            }
        }
        let bytes = serde_json::to_vec(session)?;
        if bytes.len() as u64 > MAX_SESSION_BYTES {
            bail!("session storage limit reached");
        }
        let mut file = tempfile::NamedTempFile::new_in(&self.root)?;
        file.write_all(&bytes)?;
        file.as_file().sync_all()?;
        file.persist(path).map_err(|error| error.error)?;
        Ok(())
    }

    pub(crate) fn load(&self, id: &str) -> anyhow::Result<Session> {
        let mut session = self.read(id)?;
        // Append interrupted tool results so a killed app never replays an unapproved write.
        let pending: Vec<String> = session
            .items
            .iter()
            .filter(|item| item["type"] == "function_call")
            .filter_map(|item| item["call_id"].as_str())
            .filter(|id| {
                !session
                    .items
                    .iter()
                    .any(|item| item["type"] == "function_call_output" && item["call_id"] == *id)
            })
            .map(str::to_owned)
            .collect();
        for call_id in pending {
            session.items.push(json!({"type":"function_call_output", "call_id":call_id, "output":"Interrupted; no pending write was applied. Request review again."}));
        }
        self.save(&session)?;
        Ok(session)
    }

    fn read(&self, id: &str) -> anyhow::Result<Session> {
        let id = Uuid::parse_str(id)?.to_string();
        let mut bytes = Vec::new();
        std::fs::File::open(self.root.join(format!("{id}.json")))?
            .take(MAX_SESSION_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_SESSION_BYTES {
            bail!("session file exceeds storage limit");
        }
        let session: Session = serde_json::from_slice(&bytes).context("invalid session file")?;
        if session.id != id {
            bail!("session identifier mismatch");
        }
        Ok(session)
    }

    pub(crate) fn list(&self) -> anyhow::Result<Vec<Value>> {
        let mut sessions = Vec::new();
        for entry in std::fs::read_dir(&self.root)?.take(MAX_SESSIONS) {
            let path = entry?.path();
            if let Some(id) = path.file_stem().and_then(|name| name.to_str())
                && let Ok(uuid) = Uuid::parse_str(id)
                && let Ok(session) = self.read(&uuid.to_string())
            {
                sessions.push(json!({"id":session.id, "title":session.title}));
            }
        }
        sessions.sort_by(|left, right| left["title"].as_str().cmp(&right["title"].as_str()));
        Ok(sessions)
    }
}
