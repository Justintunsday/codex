use crate::FileSystemBackend;
use crate::PlatformError;
use serde::Serialize;
use similar::TextDiff;
use std::io::Read;
use std::io::Write;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

const MAX_FILE_BYTES: u64 = 65_536;
const MAX_ENTRIES: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub path: String,
    pub is_directory: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    pub path: String,
    pub before: String,
    pub after: String,
    pub diff: String,
    pub existed: bool,
}

pub struct ScopedFiles {
    root: PathBuf,
}

impl ScopedFiles {
    pub fn new(root: &Path) -> Result<Self, PlatformError> {
        let root = root.canonicalize()?;
        if !root.is_dir() {
            return Err(PlatformError::OutsideProject);
        }
        Ok(Self { root })
    }

    fn resolve(&self, relative: &str) -> Result<PathBuf, PlatformError> {
        let relative = Path::new(relative);
        if relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
        {
            return Err(PlatformError::OutsideProject);
        }
        // Refuse symlinks, including links that point back into the project: users must
        // review the real file, and a changed symlink must never redirect a pending write.
        let mut result = self.root.clone();
        for component in relative.components() {
            result.push(component);
            match result.symlink_metadata() {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(PlatformError::OutsideProject);
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        // The project root itself must still resolve to the root that was authorized.
        if self.root.canonicalize()? != self.root {
            return Err(PlatformError::OutsideProject);
        }
        Ok(result)
    }
}

impl FileSystemBackend for ScopedFiles {
    fn list(&self, relative: &str) -> Result<Vec<FileEntry>, PlatformError> {
        let folder = self.resolve(relative)?;
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(folder)?.take(MAX_ENTRIES) {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            let relative = path
                .strip_prefix(&self.root)
                .map_err(|_| PlatformError::OutsideProject)?;
            entries.push(FileEntry {
                path: relative.to_string_lossy().replace('\\', "/"),
                is_directory: file_type.is_dir(),
            });
        }
        entries.sort_by(|left, right| {
            right
                .is_directory
                .cmp(&left.is_directory)
                .then(left.path.cmp(&right.path))
        });
        Ok(entries)
    }

    fn read(&self, relative: &str) -> Result<String, PlatformError> {
        let file = std::fs::File::open(self.resolve(relative)?)?;
        if !file.metadata()?.is_file() {
            return Err(PlatformError::UnsupportedFile);
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(PlatformError::UnsupportedFile);
        }
        String::from_utf8(bytes).map_err(|_| PlatformError::UnsupportedFile)
    }

    fn prepare(&self, relative: &str, after: String) -> Result<Change, PlatformError> {
        if after.len() as u64 > MAX_FILE_BYTES {
            return Err(PlatformError::UnsupportedFile);
        }
        let path = self.resolve(relative)?;
        let existed = path.try_exists()?;
        let before = if existed {
            self.read(relative)?
        } else {
            String::new()
        };
        let diff = TextDiff::from_lines(&before, &after)
            .unified_diff()
            .header("before", "after")
            .to_string();
        Ok(Change {
            path: relative.to_owned(),
            before,
            after,
            diff,
            existed,
        })
    }

    fn apply(&self, change: &Change) -> Result<(), PlatformError> {
        let path = self.resolve(&change.path)?;
        if path.try_exists()? != change.existed
            || (change.existed && self.read(&change.path)? != change.before)
        {
            return Err(PlatformError::Conflict);
        }
        let parent = path.parent().ok_or(PlatformError::OutsideProject)?;
        // No implicit directory creation: every component must already be authorized.
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        if change.existed {
            temporary
                .as_file()
                .set_permissions(std::fs::metadata(&path)?.permissions())?;
        }
        temporary.write_all(change.after.as_bytes())?;
        temporary.as_file().sync_all()?;
        self.resolve(&change.path)?;
        temporary.persist(&path).map_err(|error| error.error)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "files_tests.rs"]
mod tests;
