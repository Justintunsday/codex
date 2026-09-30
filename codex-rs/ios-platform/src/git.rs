//! Pure Rust Git file analysis. No executable, hook, filter or credential helper runs.
#[path = "git_validation.rs"]
mod validation;
use validation::index_entries;
use validation::validate_metadata;
use validation::validate_mode;
use validation::validate_path;

use crate::ScopedFiles;
use anyhow::Context;
use anyhow::bail;
use gix::hash::ObjectId;
use gix::index::entry::Mode;
use serde::Deserialize;
use serde::Serialize;
use similar::TextDiff;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::AtomicBool;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitChange {
    pub path: String,
    pub index: String,
    pub working: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitReport {
    pub head: String,
    pub head_id: String,
    pub index_id: String,
    pub changes: Vec<GitChange>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GitLayer {
    Index,
    Working,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiff {
    pub path: String,
    pub diff: String,
    pub current_id: String,
    pub layer: GitLayer,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Entry {
    id: ObjectId,
    mode: Mode,
}

type Entries = BTreeMap<String, Entry>;

fn current_id(entry: Option<Entry>) -> String {
    entry
        .map(|entry| format!("{}:{}", entry.id, entry.mode.bits()))
        .unwrap_or_else(|| "deleted".into())
}

/// Git operations are confined to the same imported container as the file editor.
/// Linked worktrees, object alternates, conflicts, symlinks and submodules report
/// an unsupported error. Diffs support UTF-8 blobs up to the editor's 64 KiB limit.
pub struct NativeGit<'a> {
    files: &'a ScopedFiles,
    repo: gix::Repository,
}

impl<'a> NativeGit<'a> {
    pub fn open(files: &'a ScopedFiles) -> anyhow::Result<Self> {
        let metadata = files.resolve(".git")?;
        if !metadata.is_dir() {
            bail!("import a repository containing its .git directory");
        }
        for relative in [".git/commondir", ".git/objects/info/alternates"] {
            if files.resolve(relative)?.try_exists()? {
                bail!("linked worktrees and object alternates require a remote Git backend");
            }
        }
        if files.resolve(".git/info/attributes")?.try_exists()? {
            bail!("repositories with Git attributes require a filter backend");
        }
        for (relative, limit) in [
            (".git/config", 1_048_576),
            (".git/index", 16_777_216),
            (".git/HEAD", 65_536),
            (".git/packed-refs", 4_194_304),
            (".git/info/exclude", 1_048_576),
        ] {
            let file = files.resolve(relative)?;
            if file.try_exists()? && file.metadata()?.len() > limit {
                bail!("Git metadata file exceeds mobile limits: {relative}");
            }
        }
        let mut visited = 0;
        validate_metadata(&metadata, &mut visited, /*depth*/ 0)?;
        let repo = gix::open_opts(
            &metadata,
            gix::open::Options::isolated().config_overrides(["core.excludesFile="]),
        )?;
        if repo
            .config_snapshot()
            .string("core.autocrlf")
            .is_some_and(|value| value.as_ref() != b"false".as_slice())
        {
            bail!("Git EOL conversion requires a filter backend");
        }
        if repo
            .workdir()
            .context("bare repositories are unsupported")?
            .canonicalize()?
            != files.root
        {
            bail!("Git worktree is outside the imported project");
        }
        Ok(Self { files, repo })
    }

    pub fn status(&self) -> anyhow::Result<GitReport> {
        let head = self.head_entries()?;
        let index = self.repo.index_or_empty()?;
        let staged = index_entries(&index)?;
        let mut paths: BTreeSet<String> = head.keys().chain(staged.keys()).cloned().collect();
        let mut visited = 0;
        self.worktree_paths("", &mut paths, &mut visited, /*depth*/ 0)?;
        if paths.len() > 10_000 {
            bail!("Git project exceeds 10,000 files");
        }
        let mut excludes = self.repo.excludes(
            &index,
            /*overrides*/ None,
            gix::worktree::stack::state::ignore::Source::WorktreeThenIdMappingIfNotSkipped,
        )?;
        let mut changes = Vec::new();
        for path in paths {
            let tracked = head.contains_key(&path) || staged.contains_key(&path);
            if !tracked
                && excludes
                    .at_entry(path.as_str(), /*mode*/ None)?
                    .is_excluded()
            {
                continue;
            }
            let working = self.working_entry(&path)?;
            let index_status = change_kind(head.get(&path).copied(), staged.get(&path).copied());
            let working_status = if !tracked {
                "?"
            } else {
                change_kind(staged.get(&path).copied(), working)
            };
            if !index_status.is_empty() || !working_status.is_empty() {
                changes.push(GitChange {
                    path,
                    index: index_status.into(),
                    working: working_status.into(),
                });
                if changes.len() > 500 {
                    bail!("Git status exceeds the 500-change display limit");
                }
            }
        }
        Ok(GitReport {
            head: self
                .repo
                .head_name()?
                .map(|name| name.as_bstr().to_string())
                .unwrap_or_else(|| "Detached HEAD".into()),
            head_id: self.head_id()?,
            index_id: index
                .checksum()
                .map(|id| id.to_string())
                .unwrap_or_default(),
            changes,
        })
    }

    pub fn diff(&self, path: &str, layer: GitLayer) -> anyhow::Result<GitDiff> {
        validate_path(path)?;
        let current = self.working_entry(path)?;
        let snapshot = self.repo.index_or_empty()?;
        let index = index_entries(&snapshot)?;
        let before = match layer {
            GitLayer::Index => self.blob_text(self.head_entries()?.get(path).copied())?,
            GitLayer::Working => self.blob_text(index.get(path).copied())?,
        };
        let after = match layer {
            GitLayer::Index => self.blob_text(index.get(path).copied())?,
            GitLayer::Working => {
                let file = self.files.resolve(path)?;
                if file.try_exists()? {
                    crate::FileSystemBackend::read(self.files, path)?
                } else {
                    String::new()
                }
            }
        };
        let diff = TextDiff::from_lines(&before, &after)
            .unified_diff()
            .header(&format!("a/{path}"), &format!("b/{path}"))
            .to_string();
        if matches!(layer, GitLayer::Working)
            && let Some(entry) = current
            && entry.id
                != gix::objs::compute_hash(
                    self.repo.object_hash(),
                    gix::objs::Kind::Blob,
                    after.as_bytes(),
                )?
        {
            bail!("file changed while reading Git diff; refresh before staging");
        }
        Ok(GitDiff {
            path: path.into(),
            diff,
            current_id: current_id(current),
            layer,
        })
    }

    fn head_id(&self) -> anyhow::Result<String> {
        if self.repo.head()?.is_unborn() {
            Ok("unborn".into())
        } else {
            Ok(self.repo.head_id()?.to_string())
        }
    }

    fn head_entries(&self) -> anyhow::Result<Entries> {
        if self.repo.head()?.is_unborn() {
            return Ok(Entries::new());
        }
        let mut entries = Entries::new();
        let commit = self.repo.head_id()?.detach();
        let header = self.repo.find_header(commit)?;
        if header.kind() != gix::objs::Kind::Commit || header.size() > 1_048_576 {
            bail!("Git HEAD must reference a commit within mobile limits");
        }
        self.collect_tree(
            self.repo.find_commit(commit)?.tree_id()?.detach(),
            "",
            &mut entries,
            /*depth*/ 0,
        )?;
        Ok(entries)
    }

    fn collect_tree(
        &self,
        id: ObjectId,
        prefix: &str,
        entries: &mut Entries,
        depth: usize,
    ) -> anyhow::Result<()> {
        if depth > 64 || self.repo.find_header(id)?.size() > 1_048_576 {
            bail!("Git tree exceeds mobile limits");
        }
        for entry in self.repo.find_tree(id)?.iter() {
            let entry = entry?;
            let name = std::str::from_utf8(entry.filename())?;
            let path = if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}/{name}")
            };
            validate_path(&path)?;
            if entry.mode().is_tree() {
                self.collect_tree(entry.object_id(), &path, entries, depth + 1)?;
            } else {
                let mode = Mode::from(entry.mode().kind());
                validate_mode(mode)?;
                entries.insert(
                    path,
                    Entry {
                        id: entry.object_id(),
                        mode,
                    },
                );
                if entries.len() > 10_000 {
                    bail!("Git tree exceeds 10,000 files");
                }
            }
        }
        Ok(())
    }

    fn worktree_paths(
        &self,
        folder: &str,
        paths: &mut BTreeSet<String>,
        visited: &mut usize,
        depth: usize,
    ) -> anyhow::Result<()> {
        if depth > 64 {
            bail!("project directory nesting exceeds limit");
        }
        for entry in std::fs::read_dir(self.files.resolve(folder)?)? {
            let entry = entry?;
            if entry.file_name() == ".git" {
                continue;
            }
            *visited += 1;
            if *visited > 10_000 {
                bail!("Git project exceeds 10,000 entries");
            }
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("Git paths must be UTF-8"))?;
            let path = if folder.is_empty() {
                name
            } else {
                format!("{folder}/{name}")
            };
            self.files.resolve(&path)?;
            if entry.file_type()?.is_dir() {
                self.worktree_paths(&path, paths, visited, depth + 1)?;
            } else if entry.file_type()?.is_file() {
                // Filtering and EOL conversion require an explicit separate adapter.
                if Path::new(&path)
                    .file_name()
                    .is_some_and(|name| name == ".gitattributes")
                {
                    bail!("repositories with .gitattributes require a Git filter backend");
                }
                paths.insert(path);
            }
        }
        Ok(())
    }

    fn working_entry(&self, path: &str) -> anyhow::Result<Option<Entry>> {
        validate_path(path)?;
        let file = self.files.resolve(path)?;
        if !file.try_exists()? {
            return Ok(None);
        }
        let mut file = std::fs::File::open(file)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > 256 * 1024 * 1024 {
            bail!("Git file exceeds mobile limits");
        }
        let id = gix::objs::compute_stream_hash(
            self.repo.object_hash(),
            gix::objs::Kind::Blob,
            &mut file,
            metadata.len(),
            &mut gix::progress::Discard,
            &AtomicBool::new(/*v*/ false),
        )?;
        #[cfg(unix)]
        let executable = {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o111 != 0
        };
        #[cfg(not(unix))]
        let executable = false;
        Ok(Some(Entry {
            id,
            mode: if executable {
                Mode::FILE_EXECUTABLE
            } else {
                Mode::FILE
            },
        }))
    }

    fn blob_text(&self, entry: Option<Entry>) -> anyhow::Result<String> {
        let Some(entry) = entry else {
            return Ok(String::new());
        };
        if self.repo.find_header(entry.id)?.size() > 65_536 {
            bail!("Git diff supports text blobs up to 64 KiB");
        }
        let blob = self.repo.find_blob(entry.id)?;
        if blob.data.contains(&0) {
            bail!("binary Git diff requires a separate viewer");
        }
        Ok(std::str::from_utf8(&blob.data)?.to_owned())
    }
}

fn change_kind(before: Option<Entry>, after: Option<Entry>) -> &'static str {
    match (before, after) {
        (None, None) => "",
        (None, Some(_)) => "A",
        (Some(_), None) => "D",
        (Some(before), Some(after)) if before == after => "",
        (Some(_), Some(_)) => "M",
    }
}

#[cfg(test)]
#[path = "git_tests.rs"]
mod tests;
