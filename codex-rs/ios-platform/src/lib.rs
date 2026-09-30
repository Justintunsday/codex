//! Container-scoped iOS tools. No desktop executable or elevated privilege is assumed.
mod files;
mod git;

pub use files::Change;
pub use files::FileEntry;
pub use files::ScopedFiles;
pub use git::GitChange;
pub use git::GitDiff;
pub use git::GitLayer;
pub use git::GitReport;
pub use git::NativeGit;

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("path is outside the authorized project")]
    OutsideProject,
    #[error("file is not UTF-8 text or exceeds the 64 KiB editor limit")]
    UnsupportedFile,
    #[error("file changed since review; refresh the diff before saving")]
    Conflict,
    #[error("{0} is unavailable in this runtime; no executable will be launched")]
    Unsupported(&'static str),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Filesystem adapters must confine every operation to an explicitly authorized root.
/// Swift keeps document-provider access alive while this backend is in use.
pub trait FileSystemBackend: Send + Sync {
    fn list(&self, relative: &str) -> Result<Vec<FileEntry>, PlatformError>;
    fn read(&self, relative: &str) -> Result<String, PlatformError>;
    fn prepare(&self, relative: &str, after: String) -> Result<Change, PlatformError>;
    fn apply(&self, change: &Change) -> Result<(), PlatformError>;
}

/// An execution adapter must report unsupported operations instead of invoking a desktop shell.
/// Future remote or separately installed enhanced adapters implement this boundary.
pub trait ProcessBackend: Send + Sync {
    fn execute(
        &self,
        command: &str,
    ) -> impl std::future::Future<Output = Result<String, PlatformError>> + Send;
}

pub struct AppSandbox;

impl ProcessBackend for AppSandbox {
    async fn execute(&self, _command: &str) -> Result<String, PlatformError> {
        Err(PlatformError::Unsupported("process execution / PTY"))
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCapabilities {
    pub file_access: &'static str,
    pub process: &'static str,
    pub pty: &'static str,
    pub git_commit: &'static str,
    pub jailbreak: &'static str,
    pub agent_engine: &'static str,
}

impl Default for PlatformCapabilities {
    fn default() -> Self {
        Self {
            file_access: "authorizedProject",
            process: "unsupported",
            pty: "unsupported",
            git_commit: "unsupported",
            jailbreak: "adapterNotInstalled",
            agent_engine: "codexCore",
        }
    }
}
