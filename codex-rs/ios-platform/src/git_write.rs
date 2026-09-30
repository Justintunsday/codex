use super::Entries;
use super::NativeGit;
use super::current_id;
use super::index_entries;
use crate::ScopedFiles;
use anyhow::bail;
use gix::hash::ObjectId;
use gix::index::entry::Mode;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommit {
    pub expected_head: String,
    pub expected_index: String,
    pub name: String,
    pub email: String,
    pub message: String,
}

impl NativeGit<'_> {
    /// Create metadata only when the imported project has no existing repository.
    pub fn initialize(files: &ScopedFiles) -> anyhow::Result<()> {
        if files.resolve(".git")?.try_exists()? {
            bail!("existing Git metadata will not be replaced");
        }
        gix::ThreadSafeRepository::init_opts(
            &files.root,
            gix::create::Kind::WithWorktree,
            gix::create::Options::default(),
            gix::open::Options::isolated(),
        )?;
        Ok(())
    }
    /// Stage exactly the reviewed working file. Stale content never updates the index.
    pub fn stage(&self, path: &str, expected_current: &str) -> anyhow::Result<()> {
        self.status()?;
        let working = self.working_entry(path)?;
        let current = current_id(working);
        if current != expected_current {
            bail!("file changed after Git diff; refresh before staging");
        }
        let snapshot = self.repo.index_or_empty()?;
        let mut index = gix::index::File::clone(&snapshot);
        index_entries(&index)?;
        index.remove_entries(|_, entry_path, _| entry_path == path.as_bytes());
        if let Some(entry) = working {
            let file = std::fs::File::open(self.files.resolve(path)?)?;
            if file.metadata()?.len() > 8 * 1024 * 1024 {
                bail!("native staging supports files up to 8 MiB");
            }
            let blob = self.repo.write_blob_stream(file)?.detach();
            if blob != entry.id {
                bail!("file changed during Git staging");
            }
            index.dangerously_push_entry(
                Default::default(),
                blob,
                gix::index::entry::Flags::empty(),
                entry.mode,
                path.as_bytes().into(),
            );
        }
        index.sort_entries();
        index_entries(&index)?;
        index.write(Default::default())?;
        Ok(())
    }

    /// Commit only staged content, with HEAD/index consistency checks and explicit identity.
    /// Native commits are unsigned and do not execute repository hooks or filters.
    pub fn commit(&self, request: GitCommit) -> anyhow::Result<String> {
        if request.name.trim().is_empty()
            || request.name.len() > 128
            || !request.email.contains('@')
            || request.email.len() > 256
            || request.name.contains(['\n', '\r', '\0', '<', '>'])
            || request.email.contains(['\n', '\r', '\0', '<', '>'])
            || request.message.trim().is_empty()
            || request.message.len() > 8192
        {
            bail!("enter a valid Git name, email and commit message");
        }
        self.status()?;
        let index = self.repo.index_or_empty()?;
        if self.head_id()? != request.expected_head
            || index
                .checksum()
                .map(|id| id.to_string())
                .unwrap_or_default()
                != request.expected_index
        {
            bail!("Git HEAD or index changed; refresh before committing");
        }
        let staged = index_entries(&index)?;
        if staged == self.head_entries()? {
            bail!("no staged changes to commit");
        }
        // Keep the index locked for the HEAD update. App-owned imports do not support
        // concurrent mutation by other processes; reference updates also compare parent IDs.
        let lock_path = self.files.resolve(".git/index.lock")?;
        let lock = std::fs::OpenOptions::new()
            .write(/*write*/ true)
            .create_new(/*create_new*/ true)
            .open(&lock_path)?;
        let result = (|| {
            let tree = write_tree(&self.repo, &staged, "", /*depth*/ 0)?;
            let parent = if request.expected_head == "unborn" {
                Vec::new()
            } else {
                vec![ObjectId::from_hex(request.expected_head.as_bytes())?]
            };
            let signature = gix::actor::Signature {
                name: request.name.into(),
                email: request.email.into(),
                time: gix::date::Time {
                    seconds: SystemTime::now()
                        .duration_since(UNIX_EPOCH)?
                        .as_secs()
                        .try_into()?,
                    offset: 0,
                },
            };
            let mut time = gix::date::parse::TimeBuf::default();
            let signature = signature.to_ref(&mut time);
            Ok(self
                .repo
                .commit_as(signature, signature, "HEAD", request.message, tree, parent)?
                .to_string())
        })();
        drop(lock);
        std::fs::remove_file(lock_path)?;
        result
    }
}

fn write_tree(
    repo: &gix::Repository,
    files: &Entries,
    prefix: &str,
    depth: usize,
) -> anyhow::Result<ObjectId> {
    if depth > 64 {
        bail!("Git tree nesting exceeds limit");
    }
    let mut entries = BTreeMap::new();
    for (path, file) in files {
        let Some(relative) = path.strip_prefix(prefix) else {
            continue;
        };
        match relative.split_once('/') {
            Some((directory, _)) => {
                entries
                    .entry(directory.to_owned())
                    .or_insert_with(|| (gix::objs::tree::EntryKind::Tree, None));
            }
            None => {
                entries.insert(
                    relative.to_owned(),
                    (
                        if file.mode == Mode::FILE_EXECUTABLE {
                            gix::objs::tree::EntryKind::BlobExecutable
                        } else {
                            gix::objs::tree::EntryKind::Blob
                        },
                        Some(file.id),
                    ),
                );
            }
        }
    }
    let mut tree = Vec::new();
    for (name, (kind, id)) in entries {
        let id = match id {
            Some(id) => id,
            None => write_tree(repo, files, &format!("{prefix}{name}/"), depth + 1)?,
        };
        tree.push(gix::objs::tree::Entry {
            mode: kind.into(),
            filename: name.into(),
            oid: id,
        });
    }
    tree.sort();
    Ok(repo
        .write_object(&gix::objs::Tree { entries: tree })?
        .detach())
}
