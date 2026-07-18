use std::fs;
use std::path::Path;

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

fn registry(entries: &str, max_debt: usize) -> String {
    format!(
        r#"{{
  "version": 1,
  "budgets": {{"repository_migration_debt": {max_debt}, "modules": {{"runtime": {max_debt}}}}},
  "entries": [{entries}]
}}"#
    )
}

fn migration_entry(id: &str, path: &str) -> String {
    format!(
        r#"{{
  "id": "{id}",
  "guard": "check-example.sh",
  "module": "runtime",
  "scope": {{"kind": "path", "value": "{path}"}},
  "classification": "migration_exception",
  "owner": "runtime",
  "reason": "temporary migration seam",
  "tracking_issue": 1021,
  "introduced_baseline": "v0.1.0",
  "exit_condition": "remove after migration",
  "status": "active"
}}"#
    )
}

#[test]
fn registry_rejects_missing_migration_metadata() {
    let input = registry(
        r#"{
  "id": "migration.runtime.example",
  "guard": "check-example.sh",
  "module": "runtime",
  "scope": {"kind": "path", "value": "agent/runtime.rs"},
  "classification": "migration_exception",
  "owner": "",
  "reason": "",
  "tracking_issue": null,
  "introduced_baseline": "v0.1.0",
  "exit_condition": "",
  "status": "active"
}"#,
        1,
    );

    let error = xtask::guard_registry::validate_str(&input).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("owner"));
    assert!(message.contains("reason"));
    assert!(message.contains("tracking_issue"));
    assert!(message.contains("exit_condition"));
}

#[test]
fn registry_rejects_duplicate_stable_ids() {
    let entry = migration_entry("migration.runtime.example", "agent/runtime.rs");
    let input = registry(&format!("{entry},{entry}"), 2);

    let error = xtask::guard_registry::validate_str(&input).unwrap_err();
    assert!(error.to_string().contains("stable id 重复"));
}

#[test]
fn target_policy_is_not_counted_as_migration_debt() {
    let input = registry(
        r#"{
  "id": "policy.composition.root",
  "guard": "check-forbidden-imports.sh",
  "module": "composition",
  "scope": {"kind": "path_prefix", "value": "agent/composition/src/"},
  "classification": "target_capability_policy",
  "owner": "composition",
  "reason": "unique production composition root",
  "tracking_issue": 1002,
  "introduced_baseline": "v0.1.0",
  "exit_condition": "permanent while composition remains the deployable root",
  "status": "active"
}"#,
        0,
    );

    let report = xtask::guard_registry::validate_str(&input).unwrap();
    assert_eq!(report.migration_debt, 0);
    assert_eq!(report.by_classification["target_capability_policy"], 1);
}

#[test]
fn registry_rejects_repository_and_module_budget_overflow() {
    let entry = migration_entry("migration.runtime.example", "agent/runtime.rs");
    let input = registry(&entry, 0);

    let error = xtask::guard_registry::validate_str(&input).unwrap_err();
    assert!(error.to_string().contains("预算"));
}

#[test]
fn workspace_scan_rejects_unregistered_grep_exclusion() {
    let temp = tempfile::tempdir().unwrap();
    write(
        &temp.path().join(".agents/hooks/check-example.sh"),
        "#!/usr/bin/env bash\ngrep -R thing src | grep -v allowed.rs\n",
    );
    write(
        &temp.path().join(".agents/architecture-guard-registry.json"),
        &registry("", 0),
    );

    let error = xtask::guard_registry::check_workspace(temp.path(), None).unwrap_err();
    assert!(error.to_string().contains("未登记隐式排除"));
    assert!(error.to_string().contains("grep -v"));
}

#[test]
fn workspace_scan_accepts_registered_reference() {
    let temp = tempfile::tempdir().unwrap();
    write(
        &temp.path().join(".agents/hooks/check-example.sh"),
        "#!/usr/bin/env bash\ngrep -R thing src | grep -v allowed.rs # guard-registry:scope.runtime.example\n",
    );
    let entry = r#"{
  "id": "scope.runtime.example",
  "guard": "check-example.sh",
  "module": "runtime",
  "scope": {"kind": "pattern", "value": "allowed.rs"},
  "classification": "scope_exclusion",
  "owner": "runtime",
  "reason": "test fixture scope",
  "tracking_issue": 1021,
  "introduced_baseline": "v0.1.0",
  "exit_condition": "permanent test scope",
  "status": "active"
}"#;
    write(
        &temp.path().join(".agents/architecture-guard-registry.json"),
        &registry(entry, 0),
    );

    let report = xtask::guard_registry::check_workspace(temp.path(), None).unwrap();
    assert_eq!(report.migration_debt, 0);
}

#[test]
fn workspace_scan_rejects_stale_registered_path() {
    let temp = tempfile::tempdir().unwrap();
    write(
        &temp.path().join(".agents/hooks/check-example.sh"),
        "#!/usr/bin/env bash\n# guard-registry:migration.runtime.example\ntrue\n",
    );
    let entry = migration_entry("migration.runtime.example", "missing/path.rs");
    write(
        &temp.path().join(".agents/architecture-guard-registry.json"),
        &registry(&entry, 1),
    );

    let error = xtask::guard_registry::check_workspace(temp.path(), None).unwrap_err();
    assert!(error.to_string().contains("stale"));
    assert!(error.to_string().contains("missing/path.rs"));
}

#[test]
fn report_is_deterministic_and_sorted_by_stable_id() {
    let first = migration_entry("migration.runtime.zeta", "zeta.rs");
    let second = migration_entry("migration.runtime.alpha", "alpha.rs");
    let input = registry(&format!("{first},{second}"), 2);

    let report = xtask::guard_registry::validate_str(&input).unwrap();
    let rendered = report.render();
    assert!(
        rendered.find("migration.runtime.alpha").unwrap()
            < rendered.find("migration.runtime.zeta").unwrap()
    );
}
