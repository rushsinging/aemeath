use std::fs;
use std::path::Path;

fn write_source(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("parent dir")).expect("create parent");
    fs::write(path, contents).expect("write source");
}

fn forbidden_segments_rule() -> crate::guards_rules::Rule {
    serde_json::from_value(serde_json::json!({
        "id": "use.feature.no-internal-segments",
        "assertion": "forbidden_segments",
        "scope": { "kind": "path_prefix", "value": "crates" },
        "forbidden_segments": ["domain", "adapters"],
        "allow_prefixes": ["crates/composition"],
        "reason": "内部层路径段禁止跨 crate 穿透",
        "profile": "full"
    }))
    .expect("deserialize rule")
}

#[test]
fn forbidden_segments_flags_use_of_internal_segment() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(
        &temp.path().join("crates/runtime/service.rs"),
        "use feature_x::domain::Entity;\n",
    );

    let violations = crate::guards_rules::enforce_rule(
        &forbidden_segments_rule(),
        temp.path(),
        "crates/runtime/service.rs",
    )
    .expect("enforce");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].rule_id, "use.feature.no-internal-segments");
    assert!(violations[0]
        .location
        .starts_with("crates/runtime/service.rs:1"));
    assert!(violations[0].message.contains("domain"));
}

#[test]
fn forbidden_segments_allows_files_under_allow_prefix() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(
        &temp.path().join("crates/composition/wire.rs"),
        "use feature_x::domain::Entity;\n",
    );

    let violations = crate::guards_rules::enforce_rule(
        &forbidden_segments_rule(),
        temp.path(),
        "crates/composition/wire.rs",
    )
    .expect("enforce");

    assert!(violations.is_empty());
}

#[test]
fn forbidden_segments_skips_test_module_uses() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(
        &temp.path().join("crates/runtime/service.rs"),
        "#[cfg(test)]\nmod tests {\n    use feature_x::domain::Entity;\n}\n",
    );

    let violations = crate::guards_rules::enforce_rule(
        &forbidden_segments_rule(),
        temp.path(),
        "crates/runtime/service.rs",
    )
    .expect("enforce");

    assert!(violations.is_empty());
}

#[test]
fn facade_whitelist_flags_unregistered_reexport() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(
        &temp.path().join("crates/task/src/lib.rs"),
        "pub use inner::{Registered, Rogue};\n",
    );

    let rule: crate::guards_rules::Rule = serde_json::from_value(serde_json::json!({
        "id": "facade.task.root-reexports",
        "assertion": "facade_whitelist",
        "scope": { "kind": "path_prefix", "value": "crates/task/src" },
        "allowed_symbols": ["Registered"],
        "reason": "crate 根窄 façade",
        "profile": "full"
    }))
    .expect("deserialize rule");

    let violations =
        crate::guards_rules::enforce_rule(&rule, temp.path(), "crates/task/src/lib.rs")
            .expect("enforce");

    assert_eq!(violations.len(), 1);
    assert!(violations[0].message.contains("Rogue"));
}

#[test]
fn layer_order_flags_inner_layer_depending_on_outer_layer() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(
        &temp.path().join("crates/task/src/domain/agg.rs"),
        "use crate::application::Service;\n",
    );
    write_source(
        &temp.path().join("crates/task/src/application/service.rs"),
        "use crate::domain::Entity;\n",
    );

    let rule: crate::guards_rules::Rule = serde_json::from_value(serde_json::json!({
        "id": "layer.task.hexagonal-order",
        "assertion": "layer_order",
        "scope": { "kind": "path_prefix", "value": "crates/task/src" },
        "layer_order": ["domain", "application", "ports", "adapters"],
        "reason": "domain 为最内层，依赖方向只能由外向内",
        "profile": "full"
    }))
    .expect("deserialize rule");

    let domain_violations =
        crate::guards_rules::enforce_rule(&rule, temp.path(), "crates/task/src/domain/agg.rs")
            .expect("enforce");
    assert_eq!(
        domain_violations.len(),
        1,
        "domain 依赖 application 必须违规"
    );

    let application_violations = crate::guards_rules::enforce_rule(
        &rule,
        temp.path(),
        "crates/task/src/application/service.rs",
    )
    .expect("enforce");
    assert!(
        application_violations.is_empty(),
        "application 依赖 domain 合法"
    );
}

#[test]
fn layout_flags_unregistered_directory_entry() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(
        &temp.path().join("crates/task/src/rogue_module.rs"),
        "pub fn f() {}\n",
    );

    let rule: crate::guards_rules::Rule = serde_json::from_value(serde_json::json!({
        "id": "layout.task.top-level",
        "assertion": "layout",
        "scope": { "kind": "path_prefix", "value": "crates/task/src" },
        "allowed_entries": ["lib.rs", "domain", "application"],
        "reason": "顶层布局白名单",
        "profile": "full"
    }))
    .expect("deserialize rule");

    let violations =
        crate::guards_rules::enforce_rule(&rule, temp.path(), "crates/task/src/rogue_module.rs")
            .expect("enforce");

    assert_eq!(violations.len(), 1);
    assert!(violations[0].message.contains("rogue_module.rs"));
}

#[test]
fn pattern_exclusion_flags_forbidden_pattern_with_file_exemption() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(
        &temp.path().join("crates/tui/model/update.rs"),
        "tokio::spawn(do_work);\n",
    );
    write_source(
        &temp.path().join("crates/tui/runtime/exec.rs"),
        "tokio::spawn(do_work);\n",
    );

    let rule: crate::guards_rules::Rule = serde_json::from_value(serde_json::json!({
        "id": "pattern.tui.model-no-side-effects",
        "assertion": "pattern_exclusion",
        "scope": { "kind": "path_prefix", "value": "crates/tui" },
        "forbidden_patterns": ["tokio::spawn"],
        "exclusions": [{ "path": "crates/tui/runtime" }],
        "reason": "model/update 目录禁止副作用",
        "profile": "full"
    }))
    .expect("deserialize rule");

    let model_violations =
        crate::guards_rules::enforce_rule(&rule, temp.path(), "crates/tui/model/update.rs")
            .expect("enforce");
    assert_eq!(model_violations.len(), 1);

    let runtime_violations =
        crate::guards_rules::enforce_rule(&rule, temp.path(), "crates/tui/runtime/exec.rs")
            .expect("enforce");
    assert!(runtime_violations.is_empty(), "豁免目录不得违规");
}

#[test]
fn pattern_exclusion_skips_test_sources() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(
        &temp.path().join("crates/tui/model/update_tests.rs"),
        "tokio::spawn(do_work);\n",
    );
    write_source(
        &temp.path().join("crates/tui/tests/support.rs"),
        "tokio::spawn(do_work);\n",
    );

    let rule: crate::guards_rules::Rule = serde_json::from_value(serde_json::json!({
        "id": "pattern.tui.model-no-side-effects",
        "assertion": "pattern_exclusion",
        "scope": { "kind": "path_prefix", "value": "crates/tui" },
        "forbidden_patterns": ["tokio::spawn"],
        "reason": "model/update 目录禁止副作用",
        "profile": "full"
    }))
    .expect("deserialize rule");

    let unit_test_violations =
        crate::guards_rules::enforce_rule(&rule, temp.path(), "crates/tui/model/update_tests.rs")
            .expect("enforce");
    assert!(unit_test_violations.is_empty(), "分离测试文件不得违规");

    let integration_violations =
        crate::guards_rules::enforce_rule(&rule, temp.path(), "crates/tui/tests/support.rs")
            .expect("enforce");
    assert!(integration_violations.is_empty(), "tests 目录不得违规");
}

#[test]
fn construction_whitelist_flags_symbol_outside_allowed_paths() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(
        &temp.path().join("crates/storage/src/lib.rs"),
        "pub fn wire() {}\n",
    );
    write_source(
        &temp.path().join("crates/runtime/src/assembly.rs"),
        "let blob = FileSystemBlobAdapter::new();\n",
    );

    let rule: crate::guards_rules::Rule = serde_json::from_value(serde_json::json!({
        "id": "construction.storage.filesystemblobadapter",
        "assertion": "construction_whitelist",
        "scope": { "kind": "path_prefix", "value": "crates" },
        "symbol": "FileSystemBlobAdapter",
        "allowed_paths": ["crates/storage/src/lib.rs"],
        "reason": "唯一构造点",
        "profile": "full"
    }))
    .expect("deserialize rule");

    let storage_violations =
        crate::guards_rules::enforce_rule(&rule, temp.path(), "crates/storage/src/lib.rs")
            .expect("enforce");
    assert!(storage_violations.is_empty());

    let runtime_violations =
        crate::guards_rules::enforce_rule(&rule, temp.path(), "crates/runtime/src/assembly.rs")
            .expect("enforce");
    assert_eq!(runtime_violations.len(), 1);
    assert!(runtime_violations[0].location.contains("assembly.rs:1"));
}

#[test]
fn rules_registry_parses_guard_section_and_retired_symbols() {
    let payload = serde_json::json!({
        "version": 1,
        "entries": [],
        "rules": [
            {
                "id": "use.feature.no-internal-segments",
                "assertion": "forbidden_segments",
                "scope": { "kind": "path_prefix", "value": "crates" },
                "forbidden_segments": ["domain"],
                "reason": "test",
                "profile": "fast"
            }
        ],
        "retired_symbols": [
            { "symbol": "CostTracker", "retired_by": "audit", "reason": "cost 退役" }
        ]
    });

    let registry =
        crate::guards_rules::parse_registry(payload.to_string().as_bytes()).expect("parse");

    assert_eq!(registry.rules.len(), 1);
    assert_eq!(
        registry.rules[0].profile,
        crate::guards_rules::Profile::Fast
    );
    assert_eq!(registry.retired_symbols.len(), 1);
    assert_eq!(registry.retired_symbols[0].symbol, "CostTracker");
}
