use super::*;
use pretty_assertions::assert_eq;

fn repository() -> anyhow::Result<tempfile::TempDir> {
    let root = tempfile::tempdir()?;
    let repo = gix::init(root.path())?;
    std::fs::write(root.path().join("hello.txt"), "Original\n")?;
    let blob = repo.write_blob("Original\n")?.detach();
    let tree = repo
        .write_object(&gix::objs::Tree {
            entries: vec![gix::objs::tree::Entry {
                mode: gix::objs::tree::EntryKind::Blob.into(),
                filename: "hello.txt".into(),
                oid: blob,
            }],
        })?
        .detach();
    let signature = gix::actor::Signature {
        name: "Fixture".into(),
        email: "fixture@example.com".into(),
        time: gix::date::Time {
            seconds: 0,
            offset: 0,
        },
    };
    let mut time = gix::date::parse::TimeBuf::default();
    let signature = signature.to_ref(&mut time);
    repo.commit_as(
        signature,
        signature,
        "HEAD",
        "Initial",
        tree,
        Vec::<ObjectId>::new(),
    )?;
    repo.index_from_tree(&tree)?.write(Default::default())?;
    Ok(root)
}

#[test]
fn native_git_separates_staged_working_and_ignored_files_without_mutating_the_index()
-> anyhow::Result<()> {
    let root = repository()?;
    let repo = gix::open(root.path())?;
    let staged = repo.write_blob("Staged\n")?.detach();
    let mut index = repo.open_index()?;
    index.entries_mut()[0].id = staged;
    index.write(Default::default())?;
    std::fs::write(root.path().join("hello.txt"), "Working\n")?;
    std::fs::write(root.path().join("new.txt"), "New\n")?;
    std::fs::write(root.path().join(".git/info/exclude"), "ignored.txt\n")?;
    std::fs::write(root.path().join("ignored.txt"), "Ignored\n")?;
    let original_index = std::fs::read(root.path().join(".git/index"))?;
    let files = ScopedFiles::new(root.path())?;
    let git = NativeGit::open(&files)?;
    assert_eq!(
        git.status()?.changes,
        vec![
            GitChange {
                path: "hello.txt".into(),
                index: "M".into(),
                working: "M".into()
            },
            GitChange {
                path: "new.txt".into(),
                index: "".into(),
                working: "?".into()
            },
        ]
    );
    let visible = git.diff("hello.txt", GitLayer::Working)?;
    insta::assert_snapshot!("native_git_working_diff", visible.diff);
    assert!(
        git.diff("hello.txt", GitLayer::Index)?
            .diff
            .contains("+Staged")
    );
    assert_eq!(
        std::fs::read(root.path().join(".git/index"))?,
        original_index
    );
    Ok(())
}

#[test]
fn deleted_file_is_reported_and_unsafe_metadata_is_rejected() -> anyhow::Result<()> {
    let root = repository()?;
    std::fs::remove_file(root.path().join("hello.txt"))?;
    let files = ScopedFiles::new(root.path())?;
    let git = NativeGit::open(&files)?;
    assert_eq!(
        git.status()?.changes,
        vec![GitChange {
            path: "hello.txt".into(),
            index: "".into(),
            working: "D".into()
        }]
    );
    assert!(
        git.diff("hello.txt", GitLayer::Working)?
            .diff
            .contains("-Original")
    );
    assert!(git.diff("../outside.txt", GitLayer::Working).is_err());
    assert!(git.diff(".GIT/HEAD", GitLayer::Working).is_err());
    std::fs::write(
        root.path().join(".git/objects/info/alternates"),
        "../outside\n",
    )?;
    assert!(NativeGit::open(&files).is_err());
    Ok(())
}
