use std::fs;
use std::path::Path;

fn write_source(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("parent dir")).expect("create parent");
    fs::write(path, contents).expect("write source");
}

fn fixture_root(temp: &Path) -> std::path::PathBuf {
    let root = temp.join("repo");
    fs::create_dir_all(root.join(".agents")).expect("create .agents");
    let registry = serde_json::json!({
        "version": 1,
        "entries": [],
        "rules": [
            {
                "id": "use.feature.no-internal-segments",
                "assertion": "forbidden_segments",
                "scope": { "kind": "path_prefix", "value": "agent" },
                "forbidden_segments": ["domain"],
                "reason": "内部层禁穿透",
                "profile": "fast"
            },
            {
                "id": "layout.task.top-level",
                "assertion": "layout",
                "scope": { "kind": "path_prefix", "value": "agent/features/task/src" },
                "allowed_entries": ["lib.rs", "domain"],
                "reason": "顶层布局",
                "profile": "full"
            }
        ],
        "retired_symbols": []
    });
    fs::write(
        root.join(".agents/architecture-guard-registry.json"),
        serde_json::to_string_pretty(&registry).expect("serialize"),
    )
    .expect("write registry");

    write_source(
        &root.join("agent/runtime/service.rs"),
        "use feature_x::domain::Entity;\n",
    );
    write_source(
        &root.join("agent/features/task/src/rogue.rs"),
        "pub fn f() {}\n",
    );
    root
}

#[test]
fn guard_run_reports_violations_with_rule_id_and_location() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = fixture_root(temp.path());

    let report = crate::guards::run(&root, crate::guards::Profile::Full, None).expect("run guard");

    assert_eq!(report.violations.len(), 2);
    let rendered = report.render();
    assert!(rendered.contains("use.feature.no-internal-segments"));
    assert!(rendered.contains("agent/runtime/service.rs:1"));
    assert!(rendered.contains("layout.task.top-level"));
    assert!(rendered.contains("rogue.rs"));
}

#[test]
fn guard_run_fast_profile_skips_full_only_rules() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = fixture_root(temp.path());

    let report = crate::guards::run(&root, crate::guards::Profile::Fast, None).expect("run guard");

    assert!(report
        .violations
        .iter()
        .all(|violation| violation.rule_id == "use.feature.no-internal-segments"));
}

#[test]
fn guard_run_single_rule_filter_runs_only_that_rule() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = fixture_root(temp.path());

    let report = crate::guards::run(
        &root,
        crate::guards::Profile::Full,
        Some("layout.task.top-level"),
    )
    .expect("run guard");

    assert_eq!(report.violations.len(), 1);
    assert_eq!(report.violations[0].rule_id, "layout.task.top-level");
}

#[test]
fn guard_run_clean_tree_has_no_violations() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = fixture_root(temp.path());
    fs::remove_file(root.join("agent/runtime/service.rs")).expect("remove violation source");
    fs::remove_file(root.join("agent/features/task/src/rogue.rs")).expect("remove rogue file");

    let report = crate::guards::run(&root, crate::guards::Profile::Full, None).expect("run guard");

    assert!(report.violations.is_empty());
    assert!(report.render().is_empty() || !report.render().trim().is_empty());
}
