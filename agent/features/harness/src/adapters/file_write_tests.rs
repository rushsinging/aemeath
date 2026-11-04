#![cfg(unix)]

use super::*;
use std::os::unix::fs::symlink;

fn write_under_parent(path: &Path, text: &str) -> Result<(), HarnessError> {
    write_component_text(path, text, path.parent().expect("parent"))
}

#[test]
fn regular_existing_file_stays_regular_and_is_updated() {
    let root = tempfile::tempdir().expect("tempdir");
    let path = root.path().join("AGENTS.md");
    std::fs::write(&path, "old").expect("seed");

    write_under_parent(&path, "new").expect("write");

    let metadata = std::fs::symlink_metadata(&path).expect("metadata");
    assert!(!metadata.file_type().is_symlink());
    assert!(metadata.is_file());
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "new");
}

#[test]
fn missing_regular_file_is_created() {
    let root = tempfile::tempdir().expect("tempdir");
    let path = root.path().join("AGENTS.md");

    write_under_parent(&path, "new").expect("write");

    let metadata = std::fs::symlink_metadata(&path).expect("metadata");
    assert!(!metadata.file_type().is_symlink());
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "new");
}

#[test]
fn symlink_is_preserved_and_target_is_updated() {
    let root = tempfile::tempdir().expect("tempdir");
    let target = root.path().join("CLAUDE.md");
    let link = root.path().join("AGENTS.md");
    std::fs::write(&target, "old").expect("seed");
    symlink("CLAUDE.md", &link).expect("symlink");

    write_under_parent(&link, "new").expect("write");

    let metadata = std::fs::symlink_metadata(&link).expect("metadata");
    assert!(metadata.file_type().is_symlink());
    assert_eq!(
        std::fs::read_link(&link).expect("readlink"),
        Path::new("CLAUDE.md")
    );
    assert_eq!(std::fs::read_to_string(&target).expect("read"), "new");
}

#[test]
fn multi_level_symlink_chain_preserves_every_link() {
    let root = tempfile::tempdir().expect("tempdir");
    let target = root.path().join("CLAUDE.md");
    let middle = root.path().join("middle.md");
    let first = root.path().join("AGENTS.md");
    std::fs::write(&target, "old").expect("seed");
    symlink("CLAUDE.md", &middle).expect("middle symlink");
    symlink("middle.md", &first).expect("first symlink");

    write_under_parent(&first, "new").expect("write");

    assert!(std::fs::symlink_metadata(&first)
        .expect("first")
        .file_type()
        .is_symlink());
    assert!(std::fs::symlink_metadata(&middle)
        .expect("middle")
        .file_type()
        .is_symlink());
    assert_eq!(
        std::fs::read_link(&first).expect("readlink"),
        Path::new("middle.md")
    );
    assert_eq!(
        std::fs::read_link(&middle).expect("readlink"),
        Path::new("CLAUDE.md")
    );
    assert_eq!(std::fs::read_to_string(&target).expect("read"), "new");
}

#[test]
fn relative_symlink_into_subdirectory_is_allowed() {
    let root = tempfile::tempdir().expect("tempdir");
    let docs = root.path().join("docs");
    std::fs::create_dir_all(&docs).expect("docs");
    let target = docs.join("CLAUDE.md");
    let link = root.path().join("AGENTS.md");
    std::fs::write(&target, "old").expect("seed");
    symlink("docs/CLAUDE.md", &link).expect("symlink");

    write_under_parent(&link, "new").expect("write");

    assert!(std::fs::symlink_metadata(&link)
        .expect("link")
        .file_type()
        .is_symlink());
    assert_eq!(std::fs::read_to_string(&target).expect("read"), "new");
}

#[test]
fn absolute_symlink_target_is_allowed() {
    let root = tempfile::tempdir().expect("tempdir");
    let target = root.path().join("CLAUDE.md");
    let link = root.path().join("AGENTS.md");
    std::fs::write(&target, "old").expect("seed");
    symlink(&target, &link).expect("symlink");

    write_under_parent(&link, "new").expect("write");

    assert!(std::fs::symlink_metadata(&link)
        .expect("link")
        .file_type()
        .is_symlink());
    assert_eq!(std::fs::read_to_string(&target).expect("read"), "new");
}

#[test]
fn symlink_cycle_is_rejected_and_links_are_untouched() {
    let root = tempfile::tempdir().expect("tempdir");
    let first = root.path().join("a.md");
    let second = root.path().join("b.md");
    symlink("b.md", &first).expect("first");
    symlink("a.md", &second).expect("second");

    let error = write_under_parent(&first, "new").expect_err("cycle");

    assert!(error.to_string().contains("循环"), "{error}");
    assert!(std::fs::symlink_metadata(&first)
        .expect("first")
        .file_type()
        .is_symlink());
    assert!(std::fs::symlink_metadata(&second)
        .expect("second")
        .file_type()
        .is_symlink());
}

#[test]
fn dangling_symlink_is_rejected_without_creating_target() {
    let root = tempfile::tempdir().expect("tempdir");
    let link = root.path().join("AGENTS.md");
    symlink("missing.md", &link).expect("symlink");

    let error = write_under_parent(&link, "new").expect_err("dangling");

    assert!(error.to_string().contains("目标不存在"), "{error}");
    assert!(std::fs::symlink_metadata(&link)
        .expect("link")
        .file_type()
        .is_symlink());
    assert!(!root.path().join("missing.md").exists());
}

#[test]
fn symlink_to_directory_is_rejected() {
    let root = tempfile::tempdir().expect("tempdir");
    let directory = root.path().join("docs");
    std::fs::create_dir_all(&directory).expect("docs");
    let link = root.path().join("AGENTS.md");
    symlink("docs", &link).expect("symlink");

    let error = write_under_parent(&link, "new").expect_err("directory");

    assert!(error.to_string().contains("不是普通文件"), "{error}");
    assert!(std::fs::symlink_metadata(&link)
        .expect("link")
        .file_type()
        .is_symlink());
}

#[test]
fn symlink_escaping_allowed_root_is_rejected() {
    let root = tempfile::tempdir().expect("root");
    let outside = tempfile::tempdir().expect("outside");
    let outside_file = outside.path().join("secret.md");
    std::fs::write(&outside_file, "secret").expect("seed");
    let link = root.path().join("AGENTS.md");
    symlink(&outside_file, &link).expect("symlink");

    let error = write_under_parent(&link, "new").expect_err("escape");

    assert!(error.to_string().contains("逃出允许根目录"), "{error}");
    assert_eq!(
        std::fs::read_to_string(&outside_file).expect("read"),
        "secret"
    );
    assert!(std::fs::symlink_metadata(&link)
        .expect("link")
        .file_type()
        .is_symlink());
}

#[test]
fn too_deep_symlink_chain_is_rejected() {
    let root = tempfile::tempdir().expect("tempdir");
    let target = root.path().join("target.md");
    std::fs::write(&target, "old").expect("seed");
    let links: Vec<PathBuf> = (0..=MAX_COMPONENT_SYMLINK_DEPTH)
        .map(|index| root.path().join(format!("l{index}.md")))
        .collect();
    for index in 0..links.len() {
        let destination = if index + 1 == links.len() {
            "target.md".to_string()
        } else {
            format!("l{}.md", index + 1)
        };
        symlink(destination, &links[index]).expect("symlink");
    }

    let error = write_under_parent(&links[0], "new").expect_err("depth");

    assert!(error.to_string().contains("最大解析深度"), "{error}");
    assert_eq!(std::fs::read_to_string(&target).expect("read"), "old");
}
