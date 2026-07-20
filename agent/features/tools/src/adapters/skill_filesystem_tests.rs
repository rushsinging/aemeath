//! Tests for the Tools-layer filesystem Skill adapter (`skill_filesystem`).
//!
//! These pin the discovery / materialization contract required by Issue #912:
//!   * discover from project `.claude/skills`, `.agents/skills`, global,
//!     extra dirs, and the built-in `commit` skill;
//!   * the adapter does NOT capture project roots — each call derives them
//!     from the query snapshot (`project_root`, `extra_dirs`);
//!   * stable priority (project `.claude` > project `.agents` > global >
//!     extra > builtin), first-seen name wins;
//!   * stable, sorted output (by stable key);
//!   * revision is a deterministic content revision;
//!   * **no** global single-slot cache — each call re-reads the filesystem;
//!   * read/parse failures of scanned files are typed (`SkillError`) and
//!     propagated by `materialize_available` (never silently skipped);
//!     non-existent directories are normally empty;
//!   * frontmatter `requires_tools` / `fallback_for` are honored.
//!
//! The adapter lives in the Tools crate (Issue #912). Context/Composition are
//! intentionally NOT modified here.

use std::collections::BTreeSet;
use std::io::Write;
use std::sync::Arc;

use super::skill_filesystem::FilesystemSkillAdapter;
use crate::domain::skill_pl::{
    CacheHint, SkillCatalogPort, SkillError, SkillMaterializationPort, SkillMaterializationQuery,
    SkillQuery, SkillSourceKind,
};

// ── helpers ────────────────────────────────────────────────────────────

/// Unique temp roots per test — required by `specs/rust-coding.md` 确定性.
static STAMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
fn unique_stamp(tag: &str) -> String {
    let n = STAMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{tag}-{n}")
}

/// A fresh, **non-existent** global skills dir, so global is normally empty.
/// (The dir is not created; `scan_dir` treats missing dirs as empty.)
fn fresh_global(tag: &str) -> std::path::PathBuf {
    let base = tempfile::tempdir().expect("tempdir").keep();
    base.join(unique_stamp(&format!("g-{tag}")))
        .join("global")
        .join("skills")
}

/// A fresh project root (created), under which `.claude/skills` and
/// `.agents/skills` will be derived by the adapter.
fn fresh_project(tag: &str) -> std::path::PathBuf {
    let base = tempfile::tempdir().expect("tempdir").keep();
    let root = base.join(unique_stamp(&format!("p-{tag}")));
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn agents_skills_dir(project: &std::path::Path) -> std::path::PathBuf {
    project.join(".agents").join("skills")
}

fn claude_skills_dir(project: &std::path::Path) -> std::path::PathBuf {
    project.join(".claude").join("skills")
}

fn catalog_query(project: std::path::PathBuf) -> SkillQuery {
    SkillQuery::new(project, Vec::new(), BTreeSet::new())
}

fn mat_query(project: std::path::PathBuf) -> SkillMaterializationQuery {
    SkillMaterializationQuery::new(project, Vec::new(), BTreeSet::new())
}

fn write_skill(dir: &std::path::Path, file: &str, name: &str, desc: &str, body: &str) {
    std::fs::create_dir_all(dir).unwrap();
    let mut f = std::fs::File::create(dir.join(file)).unwrap();
    write!(f, "---\nname: {name}\ndescription: {desc}\n---\n{body}").unwrap();
}

fn write_standard_skill(root: &std::path::Path, dir_name: &str, desc: &str, body: &str) {
    let dir = root.join(dir_name);
    write_skill(&dir, "SKILL.md", dir_name, desc, body);
}

/// Write a skill with arbitrary extra frontmatter lines (e.g.
/// `requires_tools`, `fallback_for`).
fn write_skill_fm(
    dir: &std::path::Path,
    file: &str,
    name: &str,
    desc: &str,
    extra_fm: &str,
    body: &str,
) {
    std::fs::create_dir_all(dir).unwrap();
    let mut f = std::fs::File::create(dir.join(file)).unwrap();
    write!(
        f,
        "---\nname: {name}\ndescription: {desc}\n{extra_fm}---\n{body}"
    )
    .unwrap();
}

// ── standard SKILL.md entry discovery ──────────────────────────────────

#[tokio::test]
async fn resource_markdown_beside_skill_entry_is_not_parsed_as_a_skill() {
    let project = fresh_project("resource_markdown");
    let root = agents_skills_dir(&project);
    write_standard_skill(&root, "promptfolio-summarize", "portrait", "skill body");
    std::fs::write(
        root.join("promptfolio-summarize")
            .join("analysis-prompt.md"),
        "# Conversation Analysis Guidelines\n\nResource content without frontmatter.",
    )
    .unwrap();

    let adapter = FilesystemSkillAdapter::new(fresh_global("resource_markdown"));
    let snapshot = adapter
        .materialize_available(mat_query(project))
        .await
        .expect("ordinary Markdown resources must not break Skill materialization");
    let keys: Vec<&str> = snapshot
        .fragments()
        .iter()
        .map(|fragment| fragment.stable_key())
        .collect();

    assert!(
        keys.contains(&"promptfolio-summarize"),
        "entry missing: {keys:?}"
    );
    assert!(
        !keys.contains(&"analysis-prompt"),
        "resource leaked: {keys:?}"
    );
}

#[tokio::test]
async fn catalog_ignores_resource_markdown_beside_skill_entry() {
    let project = fresh_project("catalog_resource_markdown");
    let root = agents_skills_dir(&project);
    write_standard_skill(&root, "demo", "demo", "skill body");
    write_skill(
        &root.join("demo"),
        "reference.md",
        "reference",
        "resource",
        "resource body",
    );

    let adapter = FilesystemSkillAdapter::new(fresh_global("catalog_resource_markdown"));
    let names: Vec<String> = adapter
        .list(catalog_query(project))
        .iter()
        .map(|descriptor| descriptor.name().to_owned())
        .collect();

    assert!(
        names.contains(&"demo".to_owned()),
        "entry missing: {names:?}"
    );
    assert!(
        !names.contains(&"reference".to_owned()),
        "resource leaked into Catalog: {names:?}"
    );
}

#[tokio::test]
async fn malformed_skill_entry_returns_typed_parse_error_for_skill_md() {
    let project = fresh_project("malformed_skill_entry");
    let entry = agents_skills_dir(&project).join("broken").join("SKILL.md");
    std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
    std::fs::write(&entry, "# missing frontmatter").unwrap();

    let adapter = FilesystemSkillAdapter::new(fresh_global("malformed_skill_entry"));
    let error = adapter
        .materialize_available(mat_query(project))
        .await
        .expect_err("a malformed SKILL.md must remain a typed error");
    assert!(
        matches!(error, SkillError::ParseFailed { ref path, .. } if path.ends_with("SKILL.md")),
        "unexpected error: {error:?}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn unreadable_skill_entry_returns_typed_read_error_for_skill_md() {
    use std::os::unix::fs::symlink;

    let project = fresh_project("unreadable_skill_entry");
    let skill_dir = agents_skills_dir(&project).join("broken");
    std::fs::create_dir_all(&skill_dir).unwrap();
    let entry = skill_dir.join("SKILL.md");
    symlink(skill_dir.join("missing-target.md"), &entry).unwrap();

    let adapter = FilesystemSkillAdapter::new(fresh_global("unreadable_skill_entry"));
    let error = adapter
        .materialize_available(mat_query(project))
        .await
        .expect_err("an unreadable SKILL.md must remain a typed error");
    assert!(
        matches!(error, SkillError::ReadFailed { ref path, .. } if path.ends_with("SKILL.md")),
        "unexpected error: {error:?}"
    );
}

#[tokio::test]
async fn markdown_named_directory_at_skill_root_is_not_a_flat_entry() {
    let project = fresh_project("markdown_named_directory");
    std::fs::create_dir_all(agents_skills_dir(&project).join("notes.md")).unwrap();

    let adapter = FilesystemSkillAdapter::new(fresh_global("markdown_named_directory"));
    let snapshot = adapter
        .materialize_available(mat_query(project))
        .await
        .expect("a directory named *.md is not a flat Skill entry");

    assert!(
        !snapshot
            .fragments()
            .iter()
            .any(|fragment| fragment.stable_key() == "notes"),
        "Markdown-named directory leaked into materialization"
    );
}

#[tokio::test]
async fn invalid_skill_root_returns_typed_read_error() {
    let project = fresh_project("invalid_skill_root");
    let global = fresh_global("invalid_skill_root");
    std::fs::create_dir_all(global.parent().unwrap()).unwrap();
    std::fs::write(&global, "not a directory").unwrap();

    let adapter = FilesystemSkillAdapter::new(global.clone());
    let error = adapter
        .materialize_available(mat_query(project))
        .await
        .expect_err("a non-directory Skill root must not be treated as empty");

    assert!(
        matches!(error, SkillError::ReadFailed { ref path, .. } if path == &global.to_string_lossy()),
        "unexpected error: {error:?}"
    );
}

#[tokio::test]
async fn package_skill_entry_keeps_namespace() {
    let project = fresh_project("package_skill_entry");
    let package_root = agents_skills_dir(&project)
        .join("superpowers")
        .join("skills");
    write_standard_skill(&package_root, "brainstorming", "brainstorm", "package body");
    std::fs::write(
        package_root.join("brainstorming").join("reference.md"),
        "# Package resource without frontmatter",
    )
    .unwrap();

    let adapter = FilesystemSkillAdapter::new(fresh_global("package_skill_entry"));
    let snapshot = adapter
        .materialize_available(mat_query(project))
        .await
        .expect("package SKILL.md must materialize");
    assert!(
        snapshot
            .fragments()
            .iter()
            .any(|fragment| fragment.stable_key() == "superpowers:brainstorming"),
        "namespaced package Skill missing"
    );
}

#[tokio::test]
async fn direct_skill_entry_takes_precedence_over_nested_skills_directory() {
    let project = fresh_project("direct_entry_precedence");
    let root = agents_skills_dir(&project);
    write_standard_skill(&root, "demo", "direct entry", "direct body");
    write_standard_skill(
        &root.join("demo").join("skills"),
        "nested",
        "nested resource",
        "nested body",
    );

    let adapter = FilesystemSkillAdapter::new(fresh_global("direct_entry_precedence"));
    let snapshot = adapter
        .materialize_available(mat_query(project))
        .await
        .expect("the direct SKILL.md must define a standard Skill");
    let keys: Vec<&str> = snapshot
        .fragments()
        .iter()
        .map(|fragment| fragment.stable_key())
        .collect();

    assert!(keys.contains(&"demo"), "direct entry missing: {keys:?}");
    assert!(
        !keys.contains(&"demo:nested"),
        "resource directory was misclassified as a package: {keys:?}"
    );
}

#[tokio::test]
async fn resource_markdown_does_not_change_materialization_revision() {
    let project = fresh_project("resource_revision");
    let root = agents_skills_dir(&project);
    write_standard_skill(&root, "demo", "demo", "stable body");
    let adapter = FilesystemSkillAdapter::new(fresh_global("resource_revision"));
    let before = adapter
        .materialize_available(mat_query(project.clone()))
        .await
        .unwrap();

    std::fs::write(root.join("demo").join("notes.md"), "ordinary resource").unwrap();
    let after = adapter
        .materialize_available(mat_query(project))
        .await
        .expect("resource Markdown must be ignored");

    assert_eq!(before.revision(), after.revision());
}

#[tokio::test]
async fn flat_markdown_at_skill_root_remains_compatible() {
    let project = fresh_project("flat_compatibility");
    write_skill(
        &agents_skills_dir(&project),
        "legacy.md",
        "legacy",
        "legacy flat entry",
        "legacy body",
    );

    let adapter = FilesystemSkillAdapter::new(fresh_global("flat_compatibility"));
    let snapshot = adapter
        .materialize_available(mat_query(project))
        .await
        .expect("root-level flat Markdown remains a compatibility entry");

    assert!(
        snapshot
            .fragments()
            .iter()
            .any(|fragment| fragment.stable_key() == "legacy"),
        "flat compatibility entry missing"
    );
}

// ── discovery & catalog ────────────────────────────────────────────────

#[tokio::test]
async fn catalog_lists_builtin_commit_when_no_project_skills() {
    let project = fresh_project("builtin_only");
    let adapter = FilesystemSkillAdapter::new(fresh_global("builtin_only"));
    let descs = adapter.list(catalog_query(project));
    let names: Vec<&str> = descs.iter().map(|d| d.name()).collect();
    assert!(
        names.contains(&"commit"),
        "built-in commit skill must always be discoverable: {names:?}"
    );
}

#[tokio::test]
async fn catalog_lists_project_and_extra_skills_plus_builtin() {
    let project = fresh_project("mixed");
    write_skill(
        &agents_skills_dir(&project),
        "review.md",
        "review",
        "review desc",
        "review body",
    );
    let extra = tempfile::tempdir().unwrap().keep().join("extra-skills");
    write_skill(&extra, "extra.md", "extra", "extra desc", "extra body");

    let adapter = FilesystemSkillAdapter::new(fresh_global("mixed"));
    let query = SkillQuery::new(project, vec![extra.clone()], BTreeSet::new());
    let descs = adapter.list(query);
    let by_name: std::collections::HashMap<&str, _> =
        descs.iter().map(|d| (d.name(), d.clone())).collect();

    assert!(by_name.contains_key("review"));
    assert!(by_name.contains_key("extra"));
    assert!(by_name.contains_key("commit"));
    assert_eq!(
        by_name["extra"].source().kind,
        SkillSourceKind::Extra,
        "extra-dir skill must be tagged Extra"
    );
    assert_eq!(
        by_name["review"].source().kind,
        SkillSourceKind::ProjectAgents
    );
    assert_eq!(by_name["commit"].source().kind, SkillSourceKind::Builtin);
}

#[tokio::test]
async fn catalog_output_is_sorted_by_name() {
    let project = fresh_project("sorted");
    // write out of lexical order
    for (file, name) in [("z.md", "zebra"), ("a.md", "alpha"), ("m.md", "mid")] {
        write_skill(&agents_skills_dir(&project), file, name, "d", "b");
    }
    let adapter = FilesystemSkillAdapter::new(fresh_global("sorted"));
    let descs = adapter.list(catalog_query(project));
    let names: Vec<&str> = descs.iter().map(|d| d.name()).collect();
    // builtin `commit` is also present; full set sorted.
    let mut expected = names.clone().to_vec();
    expected.sort();
    assert_eq!(names, expected, "catalog must be sorted by name");
}

// ── priority ───────────────────────────────────────────────────────────

#[tokio::test]
async fn project_claude_overrides_project_agents_for_same_name() {
    let project = fresh_project("prio_claude");
    write_skill(
        &claude_skills_dir(&project),
        "demo.md",
        "demo",
        "claude desc",
        "claude body",
    );
    write_skill(
        &agents_skills_dir(&project),
        "demo.md",
        "demo",
        "agents desc",
        "agents body",
    );
    let adapter = FilesystemSkillAdapter::new(fresh_global("prio_claude"));
    let descs = adapter.list(catalog_query(project));
    let demo = descs.iter().find(|d| d.name() == "demo").expect("demo");
    assert_eq!(demo.description(), "claude desc");
    assert_eq!(demo.source().kind, SkillSourceKind::ProjectClaude);
}

#[tokio::test]
async fn project_agents_overrides_global_for_same_name() {
    let project = fresh_project("prio_agents");
    write_skill(
        &agents_skills_dir(&project),
        "demo.md",
        "demo",
        "agents desc",
        "agents body",
    );
    let global = fresh_global("prio_agents");
    write_skill(&global, "demo.md", "demo", "global desc", "global body");
    let adapter = FilesystemSkillAdapter::new(global);
    let descs = adapter.list(catalog_query(project));
    let demo = descs.iter().find(|d| d.name() == "demo").expect("demo");
    assert_eq!(demo.description(), "agents desc");
    assert_eq!(demo.source().kind, SkillSourceKind::ProjectAgents);
}

#[tokio::test]
async fn global_overrides_extra_and_builtin_for_same_name() {
    let project = fresh_project("prio_global");
    let extra = tempfile::tempdir().unwrap().keep().join("e");
    write_skill(&extra, "demo.md", "demo", "extra desc", "extra body");
    let global = fresh_global("prio_global");
    write_skill(&global, "demo.md", "demo", "global desc", "global body");
    let adapter = FilesystemSkillAdapter::new(global);
    let query = SkillQuery::new(project, vec![extra], BTreeSet::new());
    let descs = adapter.list(query);
    let demo = descs.iter().find(|d| d.name() == "demo").expect("demo");
    assert_eq!(demo.description(), "global desc");
    assert_eq!(demo.source().kind, SkillSourceKind::Global);
}

#[tokio::test]
async fn project_skill_overrides_builtin_commit() {
    let project = fresh_project("prio_commit");
    write_skill(
        &agents_skills_dir(&project),
        "commit.md",
        "commit",
        "project commit",
        "project commit body",
    );
    let adapter = FilesystemSkillAdapter::new(fresh_global("prio_commit"));
    let snap = adapter
        .materialize_available(mat_query(project))
        .await
        .expect("materialize");
    let commit = snap
        .fragments()
        .iter()
        .find(|f| f.stable_key() == "commit")
        .expect("commit fragment");
    assert!(
        commit.content().contains("project commit body"),
        "project skill must override builtin commit"
    );
    assert_eq!(commit.source().kind, SkillSourceKind::ProjectAgents);
}

// ── materialization & revision ─────────────────────────────────────────

#[tokio::test]
async fn materialize_returns_fragments_with_content_and_stable_revision() {
    let project = fresh_project("mat_basic");
    write_skill(
        &agents_skills_dir(&project),
        "review.md",
        "review",
        "rd",
        "review body",
    );
    let adapter = FilesystemSkillAdapter::new(fresh_global("mat_basic"));

    let snap = adapter
        .materialize_available(mat_query(project))
        .await
        .expect("materialize");
    let review = snap
        .fragments()
        .iter()
        .find(|f| f.stable_key() == "review")
        .expect("review fragment");
    assert_eq!(review.content(), "review body");
    assert_eq!(review.cache_hint(), CacheHint::Stable);
    assert!(!snap.revision().as_str().is_empty());
}

#[tokio::test]
async fn revision_changes_when_a_skill_body_changes() {
    let project = fresh_project("mat_rev_change");
    write_skill(
        &agents_skills_dir(&project),
        "review.md",
        "review",
        "rd",
        "body v1",
    );
    let adapter = FilesystemSkillAdapter::new(fresh_global("mat_rev_change"));

    let before = adapter
        .materialize_available(mat_query(project.clone()))
        .await
        .expect("materialize before")
        .revision()
        .clone();

    // Mutate the skill body on disk.
    write_skill(
        &agents_skills_dir(&project),
        "review.md",
        "review",
        "rd",
        "body v2",
    );

    let after = adapter
        .materialize_available(mat_query(project))
        .await
        .expect("materialize after")
        .revision()
        .clone();

    assert_ne!(before, after, "revision must reflect content change");
}

#[tokio::test]
async fn revision_is_stable_across_repeated_reads_with_no_changes() {
    let project = fresh_project("mat_rev_stable");
    write_skill(
        &agents_skills_dir(&project),
        "review.md",
        "review",
        "rd",
        "stable body",
    );
    let adapter = FilesystemSkillAdapter::new(fresh_global("mat_rev_stable"));
    let a = adapter
        .materialize_available(mat_query(project.clone()))
        .await
        .unwrap()
        .revision()
        .clone();
    let b = adapter
        .materialize_available(mat_query(project))
        .await
        .unwrap()
        .revision()
        .clone();
    assert_eq!(a, b);
}

#[tokio::test]
async fn materialized_fragments_are_sorted_by_stable_key() {
    let project = fresh_project("mat_sorted");
    for (file, name) in [("z.md", "zebra"), ("a.md", "alpha")] {
        write_skill(&agents_skills_dir(&project), file, name, "d", "b");
    }
    let adapter = FilesystemSkillAdapter::new(fresh_global("mat_sorted"));
    let snap = adapter
        .materialize_available(mat_query(project))
        .await
        .expect("materialize");
    let keys: Vec<&str> = snap.fragments().iter().map(|f| f.stable_key()).collect();
    let mut sorted = keys.clone().to_vec();
    sorted.sort();
    assert_eq!(keys, sorted, "fragments must be sorted by stable_key");
}

#[tokio::test]
async fn materialize_includes_builtin_commit_fragment() {
    let project = fresh_project("mat_builtin");
    let adapter = FilesystemSkillAdapter::new(fresh_global("mat_builtin"));
    let snap = adapter
        .materialize_available(mat_query(project))
        .await
        .expect("materialize");
    let commit = snap
        .fragments()
        .iter()
        .find(|f| f.stable_key() == "commit")
        .expect("builtin commit fragment");
    assert!(commit.content().contains("git commit"));
    assert_eq!(commit.source().kind, SkillSourceKind::Builtin);
}

// ── adapter does NOT capture project roots (per-query derivation) ───────

#[tokio::test]
async fn no_global_single_slot_cache_new_file_is_visible_immediately() {
    // Two distinct adapter instances sharing the SAME global root must both
    // observe filesystem state at call time — proving there is no
    // process-global memoized slot shared between them.
    let global = fresh_global("no_global_cache");
    let project = fresh_project("no_global_cache");
    let adapter_a = FilesystemSkillAdapter::new(global.clone());
    let adapter_b = FilesystemSkillAdapter::new(global);

    let before = adapter_a
        .materialize_available(mat_query(project.clone()))
        .await
        .unwrap();
    assert!(
        !before.fragments().iter().any(|f| f.stable_key() == "fresh"),
        "precondition: fresh not present"
    );

    write_skill(
        &agents_skills_dir(&project),
        "fresh.md",
        "fresh",
        "fd",
        "fresh body",
    );

    // A *different* adapter instance observes the new file without any
    // invalidation call — there is no shared global cache to invalidate.
    let after = adapter_b
        .materialize_available(mat_query(project))
        .await
        .unwrap();
    assert!(
        after
            .fragments()
            .iter()
            .any(|f| f.stable_key() == "fresh" && f.content() == "fresh body"),
        "fresh skill must be visible to a second adapter instance"
    );
}

#[tokio::test]
async fn same_adapter_two_different_project_queries_do_not_cross_content() {
    // A single adapter instance must NOT bleed content between two consecutive
    // queries that target different project roots: project roots come from the
    // query snapshot, never from captured adapter state.
    let adapter = FilesystemSkillAdapter::new(fresh_global("two_proj"));

    let proj_a = fresh_project("two_proj_a");
    write_skill(
        &agents_skills_dir(&proj_a),
        "alpha.md",
        "alpha",
        "ad",
        "alpha body",
    );
    let proj_b = fresh_project("two_proj_b");
    write_skill(
        &agents_skills_dir(&proj_b),
        "beta.md",
        "beta",
        "bd",
        "beta body",
    );

    let snap_a = adapter
        .materialize_available(mat_query(proj_a))
        .await
        .expect("materialize a");
    let keys_a: Vec<&str> = snap_a.fragments().iter().map(|f| f.stable_key()).collect();
    assert!(
        keys_a.contains(&"alpha"),
        "project A must see alpha: {keys_a:?}"
    );
    assert!(
        !keys_a.contains(&"beta"),
        "project A must NOT see project B's beta: {keys_a:?}"
    );

    let snap_b = adapter
        .materialize_available(mat_query(proj_b))
        .await
        .expect("materialize b");
    let keys_b: Vec<&str> = snap_b.fragments().iter().map(|f| f.stable_key()).collect();
    assert!(
        keys_b.contains(&"beta"),
        "project B must see beta: {keys_b:?}"
    );
    assert!(
        !keys_b.contains(&"alpha"),
        "project B must NOT see project A's alpha: {keys_b:?}"
    );
}

// ── frontmatter requires_tools / fallback_for filtering ────────────────

#[tokio::test]
async fn requires_tools_and_fallback_for_are_honored() {
    let project = fresh_project("fm_filter");
    // primary: no constraints, always visible.
    write_skill(
        &agents_skills_dir(&project),
        "primary.md",
        "primary",
        "pd",
        "primary body",
    );
    // needs_bash: requires_tools: [Bash] -> hidden unless Bash is available.
    write_skill_fm(
        &agents_skills_dir(&project),
        "needs_bash.md",
        "needs_bash",
        "nbd",
        "requires_tools:\n  - Bash\n",
        "needs bash body",
    );
    // backup: fallback_for: [primary] -> hidden when primary is present.
    write_skill_fm(
        &agents_skills_dir(&project),
        "backup.md",
        "backup",
        "bud",
        "fallback_for:\n  - primary\n",
        "backup body",
    );

    let adapter = FilesystemSkillAdapter::new(fresh_global("fm_filter"));

    // Case 1: no tools available -> needs_bash hidden (Bash missing),
    // backup hidden (primary present).
    let snap = adapter
        .materialize_available(mat_query(project.clone()))
        .await
        .expect("materialize case1");
    let names: Vec<&str> = snap.fragments().iter().map(|f| f.stable_key()).collect();
    assert!(names.contains(&"primary"), "primary visible: {names:?}");
    assert!(
        !names.contains(&"needs_bash"),
        "needs_bash must be hidden when Bash is unavailable: {names:?}"
    );
    assert!(
        !names.contains(&"backup"),
        "backup must be hidden while primary exists: {names:?}"
    );

    // Case 2: Bash available -> needs_bash becomes visible; backup stays hidden.
    let mut tools = BTreeSet::new();
    tools.insert("Bash".to_string());
    let query = SkillMaterializationQuery::new(project, Vec::new(), tools);
    let snap2 = adapter
        .materialize_available(query)
        .await
        .expect("materialize case2");
    let names2: Vec<&str> = snap2.fragments().iter().map(|f| f.stable_key()).collect();
    assert!(
        names2.contains(&"needs_bash"),
        "needs_bash visible with Bash: {names2:?}"
    );
    assert!(
        !names2.contains(&"backup"),
        "backup still hidden while primary exists: {names2:?}"
    );
}

#[tokio::test]
async fn requires_tools_partial_match_is_hidden() {
    let project = fresh_project("fm_partial");
    // Requires both Bash and WebFetch; only Bash is available -> hidden.
    write_skill_fm(
        &agents_skills_dir(&project),
        "multi.md",
        "multi",
        "md",
        "requires_tools:\n  - Bash\n  - WebFetch\n",
        "multi body",
    );
    let adapter = FilesystemSkillAdapter::new(fresh_global("fm_partial"));
    let mut tools = BTreeSet::new();
    tools.insert("Bash".to_string());
    let query = SkillMaterializationQuery::new(project, Vec::new(), tools);
    let snap = adapter
        .materialize_available(query)
        .await
        .expect("materialize");
    let names: Vec<&str> = snap.fragments().iter().map(|f| f.stable_key()).collect();
    assert!(
        !names.contains(&"multi"),
        "multi must be hidden when not all required tools are available: {names:?}"
    );
}

// ── typed read / parse failures (strict in materialize_available) ───────

#[tokio::test]
async fn materialize_one_returns_typed_read_error_for_missing_file() {
    let err = FilesystemSkillAdapter::materialize_one(
        std::path::Path::new("/this/does/not/exist.md"),
        SkillSourceKind::ProjectAgents,
    )
    .await
    .expect_err("must be a typed read error");
    assert!(matches!(err, SkillError::ReadFailed { .. }), "got {err:?}");
}

#[tokio::test]
async fn materialize_one_returns_typed_parse_error_for_bad_frontmatter() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("bad.md");
    // Unclosed flow mapping is unambiguously invalid YAML, guaranteeing a
    // parser error independent of serde_yml's null-key tolerance.
    std::fs::write(&path, "---\nname: bad\ndescription: {unclosed\n---\nbody").unwrap();
    let err = FilesystemSkillAdapter::materialize_one(&path, SkillSourceKind::ProjectAgents)
        .await
        .expect_err("must be a typed parse error");
    assert!(matches!(err, SkillError::ParseFailed { .. }), "got {err:?}");
}

#[tokio::test]
async fn materialize_returns_first_typed_err_for_parse_errors_not_silently_skipped() {
    // A scanned file with a parse error must surface as a typed Err from
    // materialize_available (not be silently skipped). When multiple files
    // are bad, the first (in scan order) typed error is returned.
    let project = fresh_project("batch_bad");
    write_skill(
        &agents_skills_dir(&project),
        "good.md",
        "good",
        "gd",
        "good body",
    );
    // Two malformed files (unterminated frontmatter / bad YAML).
    std::fs::create_dir_all(agents_skills_dir(&project)).unwrap();
    std::fs::write(
        agents_skills_dir(&project).join("bad1.md"),
        "---\nname: bad1\ndescription: {unclosed\n---\nbody",
    )
    .unwrap();
    std::fs::write(
        agents_skills_dir(&project).join("bad2.md"),
        "---\nname: bad2\nthis: is: not: valid: yaml:\n",
    )
    .unwrap();

    let adapter = FilesystemSkillAdapter::new(fresh_global("batch_bad"));
    let err = adapter
        .materialize_available(mat_query(project))
        .await
        .expect_err("materialize must fail with the first typed parse error");
    assert!(
        matches!(err, SkillError::ParseFailed { .. }),
        "expected ParseFailed, got {err:?}"
    );
}

#[tokio::test]
async fn nonexistent_dirs_are_normally_empty_not_errors() {
    // A project root whose skills dirs do not exist yields only builtin
    // commit — no error.
    let project = fresh_project("missing_dirs");
    let adapter = FilesystemSkillAdapter::new(fresh_global("missing_dirs"));
    let snap = adapter
        .materialize_available(mat_query(project))
        .await
        .expect("missing dirs are normally empty");
    assert_eq!(snap.fragments().len(), 1, "only builtin commit expected");
    assert_eq!(snap.fragments()[0].stable_key(), "commit");
}

#[tokio::test]
async fn empty_roots_yields_only_builtin_commit() {
    let project = fresh_project("empty");
    let adapter = FilesystemSkillAdapter::new(fresh_global("empty"));
    let snap = adapter
        .materialize_available(mat_query(project))
        .await
        .expect("materialize");
    assert_eq!(snap.fragments().len(), 1, "only builtin commit expected");
    assert_eq!(snap.fragments()[0].stable_key(), "commit");
}

// ── ports are object-safe via Arc<dyn …> ────────────────────────────────

#[tokio::test]
async fn adapter_is_usable_through_trait_objects() {
    let project = fresh_project("trait_obj");
    write_skill(&agents_skills_dir(&project), "x.md", "x", "xd", "xb");
    let adapter = Arc::new(FilesystemSkillAdapter::new(fresh_global("trait_obj")));
    let catalog: Arc<dyn SkillCatalogPort> = adapter.clone();
    let materializer: Arc<dyn SkillMaterializationPort> = adapter.clone();
    assert!(catalog
        .list(catalog_query(project.clone()))
        .iter()
        .any(|d| d.name() == "x"));
    let snap = materializer
        .materialize_available(mat_query(project))
        .await
        .unwrap();
    assert!(snap.fragments().iter().any(|f| f.stable_key() == "x"));
}
