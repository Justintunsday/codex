use super::*;
use pretty_assertions::assert_eq;

#[test]
fn edits_are_reviewed_and_stale_baselines_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    std::fs::write(root.path().join("main.txt"), "before\n")?;
    let files = ScopedFiles::new(root.path())?;
    let change = files.prepare("main.txt", "after\n".to_owned())?;
    assert_eq!(
        change,
        Change {
            path: "main.txt".into(),
            before: "before\n".into(),
            after: "after\n".into(),
            diff: "--- before\n+++ after\n@@ -1 +1 @@\n-before\n+after\n".into(),
            existed: true
        }
    );
    files.apply(&change)?;
    assert_eq!(files.read("main.txt")?, "after\n");
    assert!(matches!(files.apply(&change), Err(PlatformError::Conflict)));
    Ok(())
}

#[test]
fn creation_cannot_overwrite_a_file_created_after_review() -> Result<(), Box<dyn std::error::Error>>
{
    let root = tempfile::tempdir()?;
    let files = ScopedFiles::new(root.path())?;
    let change = files.prepare("new.txt", "proposed".to_owned())?;
    std::fs::write(root.path().join("new.txt"), "external")?;
    assert!(matches!(files.apply(&change), Err(PlatformError::Conflict)));
    assert_eq!(files.read("new.txt")?, "external");
    Ok(())
}

#[test]
fn traversal_and_oversized_reads_fail_without_modifying_files()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let files = ScopedFiles::new(root.path())?;
    assert!(matches!(
        files.read("../outside"),
        Err(PlatformError::OutsideProject)
    ));
    std::fs::write(
        root.path().join("large.txt"),
        vec![b'x'; MAX_FILE_BYTES as usize + 1],
    )?;
    assert!(matches!(
        files.read("large.txt"),
        Err(PlatformError::UnsupportedFile)
    ));
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinks_cannot_redirect_a_read_or_pending_write() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    std::fs::write(outside.path().join("secret.txt"), "secret")?;
    std::os::unix::fs::symlink(outside.path(), root.path().join("link"))?;
    let files = ScopedFiles::new(root.path())?;
    assert!(matches!(
        files.read("link/secret.txt"),
        Err(PlatformError::OutsideProject)
    ));
    let change = files.prepare("pending.txt", "approved".into())?;
    std::os::unix::fs::symlink(
        outside.path().join("secret.txt"),
        root.path().join("pending.txt"),
    )?;
    assert!(matches!(
        files.apply(&change),
        Err(PlatformError::OutsideProject)
    ));
    assert_eq!(
        std::fs::read_to_string(outside.path().join("secret.txt"))?,
        "secret"
    );
    Ok(())
}
