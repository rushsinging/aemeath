//! Tests for the dynamic filesystem Skill adapter (Issue #1438).

use std::collections::BTreeSet;
use std::io::Write;
use std::sync::Arc;

use super::skill_filesystem::FilesystemSkillAdapter;
use crate::domain::skill_pl::{
    SkillCatalogPort, SkillError, SkillLoadPort, SkillLoadQuery, SkillQuery, SkillSourceKind,
};

// ── helpers ────────────────────────────────────────────────────────────

fn fresh_project() -> std::path::PathBuf {
    tempfile::tempdir().expect("tempdir").keep()
}

fn fresh_global() -> std::path::PathBuf {
    tempfile::tempdir().expect("tempdir").keep().join("skills")
}

fn skills_dir(project: &std::path::Path) -> std::path::PathBuf {
    project.join(".agents/skills")
}

fn write_skill(
    root: &std::path::Path,
    dir: &str,
    name: &str,
    extra_frontmatter: &str,
    body: &str,
) -> std::path::PathBuf {
    let dir = root.join(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("SKILL.md");
    let mut file = std::fs::File::create(&path).unwrap();
    write!(
        file,
        "---\nname: {name}\ndescription: {name} description\n{extra_frontmatter}---\n{body}"
    )
    .unwrap();
    path
}

fn catalog_query(project: std::path::PathBuf) -> SkillQuery {
    SkillQuery::new(project, Vec::new(), BTreeSet::new())
}

fn load_query(project: std::path::PathBuf, identity: &str) -> SkillLoadQuery {
    SkillLoadQuery::new(identity, project, Vec::new(), BTreeSet::new()).unwrap()
}

#[test]
fn catalog_lists_sorted_metadata_without_loading_body() {
    let project = fresh_project();
    write_skill(&skills_dir(&project), "z", "zeta", "", "Z_BODY");
    write_skill(
        &skills_dir(&project),
        "a",
        "alpha",
        "argument-hint: '[scope]'\n",
        "A_BODY",
    );
    let adapter = FilesystemSkillAdapter::new(fresh_global());

    let descriptors = adapter.list(catalog_query(project));
    let names: Vec<_> = descriptors.iter().map(|item| item.name()).collect();
    let mut expected = names.clone();
    expected.sort();
    assert_eq!(names, expected);
    let alpha = descriptors
        .iter()
        .find(|item| item.name() == "alpha")
        .unwrap();
    assert_eq!(alpha.argument_hint(), Some("[scope]"));
}

#[tokio::test]
async fn load_reads_one_skill_by_canonical_identity() {
    let project = fresh_project();
    write_skill(
        &skills_dir(&project),
        "release",
        "release",
        "",
        "release body",
    );
    write_skill(&skills_dir(&project), "review", "review", "", "review body");
    let adapter = FilesystemSkillAdapter::new(fresh_global());

    let loaded = adapter.load(load_query(project, "release")).await.unwrap();
    assert_eq!(loaded.name(), "release");
    assert_eq!(loaded.content(), "release body");
    assert_eq!(loaded.source().kind, SkillSourceKind::ProjectAgents);
    assert!(!loaded.revision().is_empty());
    assert!(!loaded.content().contains("review body"));
}

#[tokio::test]
async fn load_resolves_identity_alias() {
    let project = fresh_project();
    write_skill(
        &skills_dir(&project),
        "review",
        "review",
        "aliases:\n  - cr\n",
        "review body",
    );
    let adapter = FilesystemSkillAdapter::new(fresh_global());

    let loaded = adapter.load(load_query(project, "cr")).await.unwrap();
    assert_eq!(loaded.name(), "review");
}

#[tokio::test]
async fn load_after_catalog_entry_deleted_returns_not_found() {
    let project = fresh_project();
    let path = write_skill(
        &skills_dir(&project),
        "release",
        "release",
        "",
        "release body",
    );
    let adapter = FilesystemSkillAdapter::new(fresh_global());
    assert!(adapter
        .list(catalog_query(project.clone()))
        .iter()
        .any(|descriptor| descriptor.name() == "release"));
    std::fs::remove_file(path).unwrap();

    let error = adapter
        .load(load_query(project, "release"))
        .await
        .expect_err("deleted skill must be unavailable");
    assert!(matches!(error, SkillError::NotFound { ref identity } if identity == "release"));
}

#[tokio::test]
async fn load_returns_typed_parse_error_for_malformed_target() {
    let project = fresh_project();
    let path = skills_dir(&project).join("broken/SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "missing frontmatter").unwrap();
    let adapter = FilesystemSkillAdapter::new(fresh_global());

    let error = adapter
        .load(load_query(project, "broken"))
        .await
        .expect_err("malformed target must fail");
    assert!(
        matches!(error, SkillError::ParseFailed { ref path, .. } if path.ends_with("SKILL.md"))
    );
}

#[tokio::test]
async fn load_reapplies_requires_tools_filter() {
    let project = fresh_project();
    write_skill(
        &skills_dir(&project),
        "shell",
        "shell",
        "requires_tools:\n  - Bash\n",
        "shell body",
    );
    let adapter = FilesystemSkillAdapter::new(fresh_global());
    let error = adapter
        .load(load_query(project.clone(), "shell"))
        .await
        .expect_err("missing required tool hides skill");
    assert!(matches!(error, SkillError::NotFound { .. }));

    let query = SkillLoadQuery::new(
        "shell",
        project,
        Vec::new(),
        BTreeSet::from(["Bash".to_string()]),
    )
    .unwrap();
    assert_eq!(adapter.load(query).await.unwrap().content(), "shell body");
}

#[tokio::test]
async fn adapter_implements_both_ports() {
    let project = fresh_project();
    write_skill(&skills_dir(&project), "x", "x", "", "x body");
    let adapter = Arc::new(FilesystemSkillAdapter::new(fresh_global()));
    let catalog: Arc<dyn SkillCatalogPort> = adapter.clone();
    let loader: Arc<dyn SkillLoadPort> = adapter;

    assert!(catalog
        .list(catalog_query(project.clone()))
        .iter()
        .any(|d| d.name() == "x"));
    assert_eq!(
        loader
            .load(load_query(project, "x"))
            .await
            .unwrap()
            .content(),
        "x body"
    );
}

#[tokio::test]
async fn package_skills_publish_and_load_qualified_identities() {
    let project = fresh_project();
    let package_root = skills_dir(&project).join("superpowers/skills");
    write_skill(
        &package_root,
        "brainstorming",
        "brainstorming",
        "",
        "brainstorm body",
    );
    let adapter = FilesystemSkillAdapter::new(fresh_global());

    let descriptors = adapter.list(catalog_query(project.clone()));
    let descriptor = descriptors
        .iter()
        .find(|item| item.name() == "superpowers:brainstorming")
        .expect("qualified package Skill must be listed");
    assert_eq!(
        descriptor.slash_command(),
        Some("superpowers:brainstorming")
    );
    assert!(descriptor.slash_aliases().is_empty());

    let loaded = adapter
        .load(load_query(project, "superpowers:brainstorming"))
        .await
        .expect("qualified identity must load");
    assert_eq!(loaded.name(), "superpowers:brainstorming");
    assert_eq!(loaded.content(), "brainstorm body");
}

#[tokio::test]
async fn load_one_preserves_typed_io_error() {
    let error = FilesystemSkillAdapter::load_one(
        std::path::Path::new("/definitely/missing/SKILL.md"),
        SkillSourceKind::ProjectAgents,
    )
    .await
    .expect_err("missing file must fail");
    assert!(matches!(error, SkillError::ReadFailed { .. }));
}

// ── 无关入口错误不得阻断 load ──────────────────────────────────────────

/// 在 Skill 根目录创建一个指向不存在目标的符号链接，模拟已删除 Skill 的残留入口。
#[cfg(unix)]
fn write_dangling_symlink(root: &std::path::Path, name: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(root).unwrap();
    let link = root.join(name);
    std::os::unix::fs::symlink("definitely-missing-skill-target", &link).unwrap();
    link
}

#[cfg(unix)]
#[tokio::test]
async fn load_succeeds_when_unrelated_entry_is_dangling_symlink() {
    let project = fresh_project();
    write_skill(
        &skills_dir(&project),
        "wanaka-dev",
        "wanaka-dev",
        "",
        "dev body",
    );
    write_dangling_symlink(&skills_dir(&project), "stale-skill");
    let adapter = FilesystemSkillAdapter::new(fresh_global());

    let loaded = adapter
        .load(load_query(project, "wanaka-dev"))
        .await
        .expect("unrelated dangling entry must not block loading");
    assert_eq!(loaded.name(), "wanaka-dev");
    assert_eq!(loaded.content(), "dev body");
}

#[cfg(unix)]
#[tokio::test]
async fn load_package_skill_succeeds_despite_unrelated_dangling_entry() {
    let project = fresh_project();
    let package_root = skills_dir(&project).join("superpowers/skills");
    write_skill(
        &package_root,
        "brainstorming",
        "brainstorming",
        "",
        "brainstorm body",
    );
    write_dangling_symlink(&skills_dir(&project), "wanaka-template-engine-migrate");
    let adapter = FilesystemSkillAdapter::new(fresh_global());

    let loaded = adapter
        .load(load_query(project, "superpowers:brainstorming"))
        .await
        .expect("package Skill must load despite unrelated dangling entry");
    assert_eq!(loaded.name(), "superpowers:brainstorming");
}

#[cfg(unix)]
#[tokio::test]
async fn load_reports_read_error_when_requested_entry_is_dangling_symlink() {
    let project = fresh_project();
    write_dangling_symlink(&skills_dir(&project), "wanaka-dev");
    let adapter = FilesystemSkillAdapter::new(fresh_global());

    let error = adapter
        .load(load_query(project, "wanaka-dev"))
        .await
        .expect_err("requested dangling entry must surface its own read error");
    assert!(
        matches!(error, SkillError::ReadFailed { ref path, .. } if path.ends_with("wanaka-dev")),
        "unexpected error: {error:?}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn load_returns_not_found_when_identity_absent_amid_unrelated_errors() {
    let project = fresh_project();
    write_dangling_symlink(&skills_dir(&project), "stale-skill");
    let adapter = FilesystemSkillAdapter::new(fresh_global());

    let error = adapter
        .load(load_query(project, "missing-skill"))
        .await
        .expect_err("absent identity must stay not found");
    assert!(
        matches!(error, SkillError::NotFound { ref identity } if identity == "missing-skill"),
        "unexpected error: {error:?}"
    );
}
