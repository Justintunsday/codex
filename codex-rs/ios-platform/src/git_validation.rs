use super::Entries;
use super::Entry;
use anyhow::Context;
use anyhow::bail;
use gix::index::entry::Mode;
use gix::index::entry::Stage;
use std::path::Component;
use std::path::Path;

pub(super) fn validate_path(path: &str) -> anyhow::Result<()> {
    if path.is_empty()
        || path.len() > 4096
        || Path::new(path)
            .components()
            .any(|part| !matches!(part, Component::Normal(name) if !name.to_string_lossy().eq_ignore_ascii_case(".git")))
    {
        bail!("invalid Git file path");
    }
    Ok(())
}

pub(super) fn validate_mode(mode: Mode) -> anyhow::Result<()> {
    if mode != Mode::FILE && mode != Mode::FILE_EXECUTABLE {
        bail!("submodules, sparse indexes and symlinks require a remote Git backend");
    }
    Ok(())
}

pub(super) fn index_entries(index: &gix::index::State) -> anyhow::Result<Entries> {
    let mut result = Entries::new();
    for entry in index.entries() {
        if entry.stage() != Stage::Unconflicted {
            bail!("resolve Git merge conflicts using a remote backend");
        }
        let path = std::str::from_utf8(entry.path(index))?.to_owned();
        validate_path(&path)?;
        validate_mode(entry.mode)?;
        result.insert(
            path,
            Entry {
                id: entry.id,
                mode: entry.mode,
            },
        );
        if result.len() > 10_000 {
            bail!("Git index exceeds 10,000 files");
        }
    }
    for path in result.keys() {
        for parent in Path::new(path).ancestors().skip(/*n*/ 1) {
            if result.contains_key(parent.to_str().context("Git path")?) {
                bail!("Git index contains file/directory conflicts");
            }
        }
    }
    Ok(result)
}

pub(super) fn validate_metadata(
    root: &Path,
    visited: &mut usize,
    depth: usize,
) -> anyhow::Result<()> {
    if depth > 64 {
        bail!("Git metadata nesting exceeds limit");
    }
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        *visited += 1;
        if *visited > 10_000 || entry.file_type()?.is_symlink() {
            bail!("Git metadata exceeds limits or contains symlinks");
        }
        if entry.file_type()?.is_dir() {
            validate_metadata(&entry.path(), visited, depth + 1)?;
        }
    }
    Ok(())
}
