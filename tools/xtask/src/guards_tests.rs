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
fn guard_run_enforces_construction_symbols_outside_allowed_paths() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = temp.path().join("repo");
    fs::create_dir_all(root.join(".agents")).expect("create .agents");
    let registry = serde_json::json!({
        "version": 1,
        "entries": [],
        "rules": [],
        "construction_symbols": [
            {
                "id": "construction.task.wire-task",
                "symbol": "wire_task",
                "owner_crate": "task",
                "kind": "wire",
                "allowed_paths": ["agent/features/task/src"],
                "reason": "test",
                "tracking_issue": 1
            }
        ]
    });
    fs::write(
        root.join(".agents/architecture-guard-registry.json"),
        serde_json::to_string_pretty(&registry).expect("serialize"),
    )
    .expect("write registry");
    write_source(
        &root.join("agent/features/task/src/lib.rs"),
        "pub fn wire_task() {}\n",
    );
    write_source(
        &root.join("agent/features/runtime/src/assembly.rs"),
        "let t = task::wire_task();\n",
    );

    let report = crate::guards::run(&root, crate::guards::Profile::Full, None).expect("run");

    let violations: Vec<&crate::guards_rules::Violation> = report
        .violations
        .iter()
        .filter(|violation| violation.location.contains("runtime/src/assembly.rs"))
        .collect();
    assert!(
        !violations.is_empty(),
        "owner crate 外引用登记 wire 必须违规：{:#?}",
        report.violations
    );
}

#[test]
fn guard_run_flags_unregistered_cross_bc_wire_calls_fail_closed() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = temp.path().join("repo");
    fs::create_dir_all(root.join(".agents")).expect("create .agents");
    let registry = serde_json::json!({
        "version": 1,
        "entries": [],
        "rules": [],
        "construction_symbols": [
            {
                "id": "construction.task.wire-task",
                "symbol": "wire_task",
                "owner_crate": "task",
                "kind": "wire",
                "allowed_paths": ["agent/features/task/src", "agent/composition/src"],
                "reason": "test",
                "tracking_issue": 1
            }
        ]
    });
    fs::write(
        root.join(".agents/architecture-guard-registry.json"),
        serde_json::to_string_pretty(&registry).expect("serialize"),
    )
    .expect("write registry");
    // 未登记的跨 BC wire 调用：storage::wire_storage 有真实 pub 定义但未登记。
    write_source(
        &root.join("agent/features/storage/src/lib.rs"),
        "pub fn wire_storage() {}\n",
    );
    write_source(
        &root.join("agent/composition/src/app.rs"),
        "let s = storage::wire_storage();\n",
    );
    // 同 crate 裸调用不拦截。
    write_source(
        &root.join("agent/features/task/src/lib.rs"),
        "pub fn wire_task() { wire_task_inner(); }\n",
    );

    let report = crate::guards::run(&root, crate::guards::Profile::Full, None).expect("run");

    assert!(
        report.violations.iter().any(|violation| violation.rule_id
            == "construction.cross-bc.fail-closed"
            && violation.location.contains("composition/src/app.rs")),
        "未登记跨 BC wire 调用必须 fail-closed：{:#?}",
        report.violations
    );
    assert!(
        !report
            .violations
            .iter()
            .any(|violation| violation.location.contains("task/src/lib.rs")),
        "owner crate 内裸调用不拦截"
    );
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
