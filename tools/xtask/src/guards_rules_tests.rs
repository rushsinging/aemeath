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
fn pattern_exclusion_skips_inline_cfg_test_region() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(
        &temp.path().join("crates/runtime/service.rs"),
        "#[cfg(test)]\nmod tests {\n    fn helper() {\n        hook::build_dispatcher(&snapshot);\n    }\n}\n",
    );

    let rule: crate::guards_rules::Rule = serde_json::from_value(serde_json::json!({
        "id": "pattern.runtime.no-hook-dispatcher-construction",
        "assertion": "pattern_exclusion",
        "scope": { "kind": "path_prefix", "value": "crates/runtime" },
        "forbidden_patterns": ["build_dispatcher("],
        "reason": "dispatcher 只由 composition 构造注入",
        "profile": "full"
    }))
    .expect("deserialize rule");

    let violations =
        crate::guards_rules::enforce_rule(&rule, temp.path(), "crates/runtime/service.rs")
            .expect("enforce");

    assert!(violations.is_empty(), "inline cfg(test) 区不得违规");
}

#[test]
fn pattern_exclusion_skips_plain_tests_module_file() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(
        &temp.path().join("crates/runtime/derived/tests.rs"),
        "use tools::composition::wire_skills;\nfn helper() { wire_skills(); }\n",
    );

    let rule: crate::guards_rules::Rule = serde_json::from_value(serde_json::json!({
        "id": "pattern.runtime.no-tool-self-assembly",
        "assertion": "pattern_exclusion",
        "scope": { "kind": "path_prefix", "value": "crates/runtime" },
        "forbidden_patterns": ["tools::composition::wire_"],
        "reason": "测试",
        "profile": "full"
    }))
    .expect("deserialize rule");

    let violations =
        crate::guards_rules::enforce_rule(&rule, temp.path(), "crates/runtime/derived/tests.rs")
            .expect("enforce");

    assert!(
        violations.is_empty(),
        "名为 tests.rs 的分离测试模块文件不得违规"
    );
}

#[test]
fn pattern_exclusion_skips_scenario_tests_directory() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(
        &temp
            .path()
            .join("crates/runtime/scenario_tests/derived_run.rs"),
        "fn harness() { wire_active_run_registry(); }\n",
    );

    let rule: crate::guards_rules::Rule = serde_json::from_value(serde_json::json!({
        "id": "pattern.runtime.no-tool-self-assembly",
        "assertion": "pattern_exclusion",
        "scope": { "kind": "path_prefix", "value": "crates/runtime" },
        "forbidden_patterns": ["wire_active_run_registry("],
        "reason": "测试",
        "profile": "full"
    }))
    .expect("deserialize rule");

    let violations = crate::guards_rules::enforce_rule(
        &rule,
        temp.path(),
        "crates/runtime/scenario_tests/derived_run.rs",
    )
    .expect("enforce");

    assert!(violations.is_empty(), "*_tests 目录下的测试源不得违规");
}

#[test]
fn forbidden_segments_supports_multi_segment_prefix() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(
        &temp.path().join("crates/runtime/service.rs"),
        "use share::adapter::widget;\nuse share::adapters::other;\n",
    );

    let rule: crate::guards_rules::Rule = serde_json::from_value(serde_json::json!({
        "id": "use.share.adapter-single-root",
        "assertion": "forbidden_segments",
        "scope": { "kind": "path_prefix", "value": "crates" },
        "forbidden_segments": ["share::adapter"],
        "reason": "adapter 只经 composition 引用",
        "profile": "full"
    }))
    .expect("deserialize rule");

    let violations =
        crate::guards_rules::enforce_rule(&rule, temp.path(), "crates/runtime/service.rs")
            .expect("enforce");

    assert_eq!(
        violations.len(),
        1,
        "share::adapter 前缀命中，share::adapters 不命中"
    );
    assert!(violations[0].message.contains("share::adapter"));
}

#[test]
fn forbidden_file_names_flags_mod_rs_anywhere() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(
        &temp.path().join("crates/task/src/domain/mod.rs"),
        "pub struct X;\n",
    );

    let rule: crate::guards_rules::Rule = serde_json::from_value(serde_json::json!({
        "id": "layout.all.no-mod-rs",
        "assertion": "forbidden_file_names",
        "scope": { "kind": "workspace" },
        "forbidden_file_names": ["mod.rs"],
        "reason": "Rust 2018+ 同名文件模块约定",
        "profile": "full"
    }))
    .expect("deserialize rule");

    let violations =
        crate::guards_rules::enforce_rule(&rule, temp.path(), "crates/task/src/domain/mod.rs")
            .expect("enforce");

    assert_eq!(violations.len(), 1);
    assert!(violations[0].message.contains("mod.rs"));
}

#[test]
fn dependency_matrix_flags_edge_outside_allow_list() {
    let matrix = std::collections::BTreeMap::from([
        ("task".to_owned(), vec![]),
        ("storage".to_owned(), vec!["share".to_owned()]),
    ]);
    let edges = std::collections::BTreeMap::from([
        ("task".to_owned(), vec!["tools".to_owned()]),
        ("storage".to_owned(), vec!["share".to_owned()]),
        ("tools".to_owned(), vec![]),
        ("share".to_owned(), vec![]),
    ]);

    let violations = crate::guards_rules::check_dependency_edges(&matrix, &edges);

    assert_eq!(violations.len(), 1);
    assert!(violations[0].message.contains("task"));
    assert!(violations[0].message.contains("tools"));
}

#[test]
fn line_budget_flags_overrun_and_missing_required_files() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(&temp.path().join("crates/r/engine.rs"), &"a\n".repeat(5));
    // required 文件 deliberate 不创建。

    let rule: crate::guards_rules::Rule = serde_json::from_value(serde_json::json!({
        "id": "budget.r.responsibility",
        "assertion": "line_budget",
        "scope": { "kind": "workspace" },
        "budgets": [{ "path": "crates/r/engine.rs", "max_lines": 4 }],
        "required_files": ["crates/r/contracts.rs"],
        "reason": "#1400 职责预算",
        "profile": "full"
    }))
    .expect("deserialize rule");

    let violations = crate::guards_rules::collect_line_budget_violations(
        &rule,
        temp.path(),
        &["crates/r/engine.rs".to_owned()],
    );
    assert!(
        violations
            .iter()
            .any(|v| v.message.contains("超出职责预算")),
        "超预算必须违规：{:#?}",
        violations
    );

    // required_files 缺失：对任意被扫描文件报告（引擎在 run 级别聚合一次）。
    let missing = crate::guards_rules::collect_line_budget_violations(
        &rule,
        temp.path(),
        &["crates/r/engine.rs".to_owned()],
    );
    assert!(
        missing.iter().any(|v| v.location.contains("contracts.rs")),
        "缺失必需文件必须违规：{:#?}",
        missing
    );
}

#[test]
fn pattern_exclusion_skips_comments_and_inline_allow_marker() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(
        &temp.path().join("crates/x/s.rs"),
        "//! docs mention .split_at( demo\nlet s = a.split_at(8); // allow unsafe_text_op\nlet t = b.split_at(4);\n",
    );

    let rule: crate::guards_rules::Rule = serde_json::from_value(serde_json::json!({
        "id": "pattern.all.no-unsafe-text-slicing",
        "assertion": "pattern_exclusion",
        "scope": { "kind": "workspace" },
        "forbidden_patterns": [".split_at("],
        "allow_marker": "allow unsafe_text_op",
        "reason": "测试",
        "profile": "full"
    }))
    .expect("deserialize rule");

    let violations =
        crate::guards_rules::enforce_rule(&rule, temp.path(), "crates/x/s.rs").expect("enforce");

    assert_eq!(violations.len(), 1, "仅无标记的第三行违规：{violations:#?}");
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
