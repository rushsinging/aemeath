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
    assert_eq!(violations[0].location, "crates/task/src/lib.rs:1");
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

/// 多前缀 scope（`path_prefixes`）：同一规则覆盖多个并列目录，
/// 用于把「pattern 完全相同的重复规则」合并为一条。
fn multi_prefix_pattern_rule() -> crate::guards_rules::Rule {
    serde_json::from_value(serde_json::json!({
        "id": "pattern.tui.no-direct-effects",
        "assertion": "pattern_exclusion",
        "scope": {
            "kind": "path_prefixes",
            "values": ["apps/cli/src/tui/model", "apps/cli/src/tui/update"]
        },
        "forbidden_patterns": ["Command::new("],
        "reason": "多前缀副作用禁式",
        "profile": "full"
    }))
    .expect("deserialize multi-prefix rule")
}

#[test]
fn pattern_exclusion_multi_prefix_hits_every_listed_scope() {
    let temp = tempfile::tempdir().expect("create tempdir");
    for scope in ["apps/cli/src/tui/model", "apps/cli/src/tui/update"] {
        write_source(
            &temp.path().join(format!("{scope}/widget.rs")),
            "fn build() { let _ = Command::new(\"ls\"); }\n",
        );
    }

    for scope in ["apps/cli/src/tui/model", "apps/cli/src/tui/update"] {
        let relative = format!("{scope}/widget.rs");
        let violations =
            crate::guards_rules::enforce_rule(&multi_prefix_pattern_rule(), temp.path(), &relative)
                .expect("enforce");
        assert_eq!(violations.len(), 1, "{relative} 应命中多前缀规则");
        assert_eq!(violations[0].rule_id, "pattern.tui.no-direct-effects");
    }
}

#[test]
fn pattern_exclusion_multi_prefix_skips_unlisted_scope() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let relative = "apps/cli/src/tui/view/widget.rs";
    write_source(
        &temp.path().join(relative),
        "fn build() { let _ = Command::new(\"ls\"); }\n",
    );

    let violations =
        crate::guards_rules::enforce_rule(&multi_prefix_pattern_rule(), temp.path(), relative)
            .expect("enforce");
    assert!(violations.is_empty(), "未登记前缀不得被多前缀规则命中");
}

#[test]
fn pattern_exclusion_multi_prefix_keeps_file_exemptions() {
    let temp = tempfile::tempdir().expect("create tempdir");
    // app 编排层豁免（#59 S5-gap 裁定）：合并多前缀后豁免仍按路径生效
    write_source(
        &temp.path().join("apps/cli/src/tui/app/run_loop.rs"),
        "fn pump() { let _ = Command::new(\"ls\"); }\n",
    );
    write_source(
        &temp.path().join("apps/cli/src/tui/app/state.rs"),
        "fn reduce() { let _ = Command::new(\"ls\"); }\n",
    );

    let rule: crate::guards_rules::Rule = serde_json::from_value(serde_json::json!({
        "id": "pattern.tui.app-pure-model",
        "assertion": "pattern_exclusion",
        "scope": {
            "kind": "path_prefixes",
            "values": ["apps/cli/src/tui/app", "apps/cli/src/tui/view_model"]
        },
        "forbidden_patterns": ["Command::new("],
        "exclusions": [{ "path": "apps/cli/src/tui/app/run_loop.rs" }],
        "reason": "编排层豁免",
        "profile": "full"
    }))
    .expect("deserialize rule with exclusions");

    let exempt =
        crate::guards_rules::enforce_rule(&rule, temp.path(), "apps/cli/src/tui/app/run_loop.rs")
            .expect("enforce exempt");
    assert!(exempt.is_empty(), "登记豁免文件不得被命中");

    let flagged =
        crate::guards_rules::enforce_rule(&rule, temp.path(), "apps/cli/src/tui/app/state.rs")
            .expect("enforce flagged");
    assert_eq!(flagged.len(), 1, "同 scope 非豁免文件必须命中");
}

#[test]
fn pattern_exclusion_strips_pub_crate_cfg_test_module() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(
        &temp.path().join("crates/x/src/domain/git.rs"),
        "pub fn production() {}\n\n#[cfg(test)]\npub(crate) mod tests {\n    fn fake() { std::fs::create_dir_all(\"/tmp/x\").unwrap(); }\n}\n",
    );
    let rule: crate::guards_rules::Rule = serde_json::from_value(serde_json::json!({
        "id": "pattern.test.no-io",
        "assertion": "pattern_exclusion",
        "scope": { "kind": "path_prefix", "value": "crates/x/src" },
        "forbidden_patterns": ["std::fs::"],
        "exclusions": [],
        "reason": "test",
        "profile": "full"
    }))
    .expect("deserialize rule");

    let violations =
        crate::guards_rules::enforce_rule(&rule, temp.path(), "crates/x/src/domain/git.rs")
            .expect("enforce");

    assert!(
        violations.is_empty(),
        "pub(crate) mod tests 内的命中必须被剥离：{violations:?}"
    );
}

#[test]
fn pattern_exclusion_regex_flags_matching_lines() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(
        &temp.path().join("crates/x/src/service.rs"),
        "struct DataProjection;\nstruct DataView;\n",
    );
    let rule: crate::guards_rules::Rule = serde_json::from_value(serde_json::json!({
        "id": "pattern.test.no-broad-name",
        "assertion": "pattern_exclusion",
        "scope": { "kind": "path_prefix", "value": "crates/x/src" },
        "forbidden_patterns": [],
        "forbidden_regex": ["\\b(?:struct|enum|trait)\\s+\\w*Projection\\w*"],
        "exclusions": [],
        "reason": "test",
        "profile": "full"
    }))
    .expect("deserialize rule");

    let violations =
        crate::guards_rules::enforce_rule(&rule, temp.path(), "crates/x/src/service.rs")
            .expect("enforce");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].location, "crates/x/src/service.rs:1");
}

#[test]
fn pattern_exclusion_regex_respects_exclusions_and_allow_marker() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(
        &temp.path().join("crates/x/src/allowed.rs"),
        "struct DataProjection;\n",
    );
    write_source(
        &temp.path().join("crates/x/src/marked.rs"),
        "struct DataProjection; // allow broad_name: legacy seam\n",
    );
    let rule: crate::guards_rules::Rule = serde_json::from_value(serde_json::json!({
        "id": "pattern.test.no-broad-name",
        "assertion": "pattern_exclusion",
        "scope": { "kind": "path_prefix", "value": "crates/x/src" },
        "forbidden_patterns": [],
        "forbidden_regex": ["\\bstruct\\s+\\w*Projection\\w*"],
        "exclusions": [{"path": "crates/x/src/allowed.rs", "reason": "legacy"}],
        "allow_marker": "allow broad_name",
        "reason": "test",
        "profile": "full"
    }))
    .expect("deserialize rule");

    for file in ["crates/x/src/allowed.rs", "crates/x/src/marked.rs"] {
        let violations =
            crate::guards_rules::enforce_rule(&rule, temp.path(), file).expect("enforce");
        assert!(violations.is_empty(), "{file} 必须被豁免：{violations:?}");
    }
}

#[test]
fn parse_registry_rejects_invalid_regex() {
    let bytes = br#"{
  "version": 1,
  "entries": [],
  "rules": [{
    "id": "pattern.test.bad-regex",
    "assertion": "pattern_exclusion",
    "scope": { "kind": "path_prefix", "value": "crates" },
    "forbidden_patterns": [],
    "forbidden_regex": ["[unclosed"],
    "exclusions": [],
    "reason": "test",
    "profile": "full"
  }],
  "retired_symbols": []
}"#;

    let error = crate::guards_rules::parse_registry(bytes).expect_err("非法 regex 必须 fail");
    assert!(
        format!("{error:#}").contains("[unclosed"),
        "错误应指出非法 regex: {error:#}"
    );
}

fn count_ratio_rule(exclusions: serde_json::Value) -> crate::guards_rules::Rule {
    serde_json::from_value(serde_json::json!({
        "id": "count.process.isolation",
        "assertion": "count_ratio",
        "scope": { "kind": "workspace" },
        "numerator_patterns": ["std::process::Command::new", "tokio::process::Command::new"],
        "denominator_patterns": ["utils::configure_std_noninteractive", "utils::configure_tokio_noninteractive"],
        "exclusions": exclusions,
        "reason": "每个外部进程构造必须配对 session 隔离",
        "profile": "fast"
    }))
    .expect("deserialize rule")
}

#[test]
fn count_ratio_flags_unbalanced_construction() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(
        &temp.path().join("crates/x/src/run.rs"),
        "fn run() {\n    let mut a = std::process::Command::new(\"git\");\n    utils::configure_std_noninteractive(&mut a)?;\n    let _ = std::process::Command::new(\"curl\").output();\n}\n",
    );

    let violations = crate::guards_rules::enforce_rule(
        &count_ratio_rule(serde_json::json!([])),
        temp.path(),
        "crates/x/src/run.rs",
    )
    .expect("enforce");

    assert_eq!(violations.len(), 1);
    assert!(violations[0].location.contains("crates/x/src/run.rs"));
    assert!(
        violations[0].message.contains('2') && violations[0].message.contains('1'),
        "消息应含两侧计数: {}",
        violations[0].message
    );
}

#[test]
fn count_ratio_passes_when_isolation_covers_construction() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(
        &temp.path().join("crates/x/src/run.rs"),
        "fn run() {\n    let mut a = std::process::Command::new(\"git\");\n    utils::configure_std_noninteractive(&mut a)?;\n    let mut b = tokio::process::Command::new(\"curl\");\n    utils::configure_tokio_noninteractive(&mut b)?;\n}\n",
    );

    let violations = crate::guards_rules::enforce_rule(
        &count_ratio_rule(serde_json::json!([])),
        temp.path(),
        "crates/x/src/run.rs",
    )
    .expect("enforce");

    assert!(violations.is_empty(), "配比满足必须放行: {violations:?}");
}

#[test]
fn count_ratio_strips_cfg_test_module_and_exclusions() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(
        &temp.path().join("crates/x/src/run.rs"),
        "#[cfg(test)]\nmod tests {\n    fn t() { let _ = std::process::Command::new(\"git\"); }\n}\n",
    );
    write_source(
        &temp.path().join("crates/x/src/exempt.rs"),
        "fn f() { let _ = std::process::Command::new(\"git\"); }\n",
    );

    let rule = count_ratio_rule(serde_json::json!([
        {"path": "crates/x/src/exempt.rs", "reason": "owner boundary"}
    ]));
    for file in ["crates/x/src/run.rs", "crates/x/src/exempt.rs"] {
        let violations =
            crate::guards_rules::enforce_rule(&rule, temp.path(), file).expect("enforce");
        assert!(
            violations.is_empty(),
            "{file} 必须被豁免/剥离: {violations:?}"
        );
    }
}

#[test]
fn constant_placement_rejects_unplaced_module_constant() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(
        &temp.path().join("crates/x/src/service.rs"),
        "pub fn helper() {}\nconst NEW_LIMIT: usize = 10;\npub fn f() {}\n",
    );
    let rule: crate::guards_rules::Rule = serde_json::from_value(serde_json::json!({
        "id": "constant.test.placement",
        "assertion": "constant_placement",
        "scope": { "kind": "workspace" },
        "reason": "test",
        "profile": "full"
    }))
    .expect("deserialize rule");

    let violations =
        crate::guards_rules::enforce_rule(&rule, temp.path(), "crates/x/src/service.rs")
            .expect("enforce");

    assert_eq!(violations.len(), 1);
    assert!(
        violations[0].message.contains("NEW_LIMIT"),
        "{}",
        violations[0].message
    );
}

#[test]
fn constant_placement_cfg_gated_must_also_relocate() {
    let temp = tempfile::tempdir().expect("create tempdir");
    write_source(
        &temp.path().join("crates/x/src/service.rs"),
        "#[cfg(any(test, feature = \"fault\"))]\nconst FAULT_ENV: &str = \"X\";\n\nfn g() {\n    const LOCAL: usize = 2;\n}\n",
    );
    let rule: crate::guards_rules::Rule = serde_json::from_value(serde_json::json!({
        "id": "constant.test.placement",
        "assertion": "constant_placement",
        "scope": { "kind": "workspace" },
        "reason": "test",
        "profile": "full"
    }))
    .expect("deserialize rule");

    let violations =
        crate::guards_rules::enforce_rule(&rule, temp.path(), "crates/x/src/service.rs")
            .expect("enforce");
    assert!(
        violations.iter().any(|v| v.message.contains("FAULT_ENV")),
        "cfg 门控常量同样必须归位（cfg 属性随常量走）: {violations:?}"
    );
    assert!(
        violations.iter().all(|v| !v.message.contains("LOCAL")),
        "函数内缩进 const 仍不治理: {violations:?}"
    );
}
