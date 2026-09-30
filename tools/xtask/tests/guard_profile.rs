//! fast/full 档位打标策略的数据完整性测试（读真实仓库 registry）。
//!
//! fast 档语义：Stop hook / pre-push 快档，抓结构性边界事实（依赖矩阵、
//! 跨 crate 内部段穿透、façade 导出、层序、目录布局）；文本扫描类断言器
//! （pattern_exclusion / line_budget / forbidden_file_names）留在 full 档。

use std::collections::HashSet;
use std::path::PathBuf;

use xtask::guards_rules::{self, Profile, RuleSpec};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn load_rules() -> Vec<guards_rules::Rule> {
    let registry_path = repo_root().join(".agents/architecture-guard-registry.json");
    let bytes = std::fs::read(&registry_path)
        .unwrap_or_else(|error| panic!("读取 {} 失败: {error}", registry_path.display()));
    guards_rules::parse_registry(&bytes)
        .unwrap_or_else(|error| panic!("解析 registry 失败: {error}"))
        .rules
}

fn assertion_name(rule: &guards_rules::Rule) -> &'static str {
    match &rule.spec {
        RuleSpec::ForbiddenSegments { .. } => "forbidden_segments",
        RuleSpec::FacadeWhitelist { .. } => "facade_whitelist",
        RuleSpec::LayerOrder { .. } => "layer_order",
        RuleSpec::PatternExclusion { .. } => "pattern_exclusion",
        RuleSpec::Layout { .. } => "layout",
        RuleSpec::ForbiddenFileNames { .. } => "forbidden_file_names",
        RuleSpec::DependencyMatrix { .. } => "dependency_matrix",
        RuleSpec::LineBudget { .. } => "line_budget",
        RuleSpec::ConstructionWhitelist { .. } => "construction_whitelist",
        RuleSpec::CountRatio { .. } => "count_ratio",
    }
}

#[test]
fn fast_profile_is_non_empty_and_strict_subset() {
    let rules = load_rules();
    let fast_count = rules
        .iter()
        .filter(|rule| rule.profile == Profile::Fast)
        .count();
    assert!(
        fast_count > 0,
        "fast 档至少包含 1 条规则，否则 Stop hook 快档引擎空跑"
    );
    assert!(
        fast_count < rules.len(),
        "fast 档必须是全量的真子集（fast={fast_count}, total={})",
        rules.len()
    );
}

#[test]
fn fast_profile_covers_structural_assertions() {
    let rules = load_rules();
    let fast_assertions: HashSet<&str> = rules
        .iter()
        .filter(|rule| rule.profile == Profile::Fast)
        .map(assertion_name)
        .collect();
    for expected in [
        "forbidden_segments",
        "facade_whitelist",
        "layer_order",
        "dependency_matrix",
        "layout",
    ] {
        assert!(
            fast_assertions.contains(expected),
            "fast 档缺少结构断言器 {expected}"
        );
    }
}

#[test]
fn text_scan_assertions_stay_in_full_profile() {
    let rules = load_rules();
    let text_scan_kinds = ["pattern_exclusion", "line_budget", "forbidden_file_names"];
    // 安全关键文本规则保留 fast 身份（原 fast 档独立脚本数据化而来，
    // Stop hook 必须持续覆盖进程隔离 / unsafe 切片 / 宽泛命名约束）；
    // 新增文本扫描规则 NEVER 进 fast（防快档膨胀）。
    let fast_text_scan_allowlist = [
        "pattern.all.no-broad-projection-naming",
        "pattern.all.no-unsafe-text-range-slicing",
        "pattern.all.lib-rs-no-const-definitions",
    ];
    let misplaced: Vec<&str> = rules
        .iter()
        .filter(|rule| rule.profile == Profile::Fast)
        .filter(|rule| text_scan_kinds.contains(&assertion_name(rule)))
        .filter(|rule| !fast_text_scan_allowlist.contains(&rule.id.as_str()))
        .map(|rule| rule.id.as_str())
        .collect();
    assert!(
        misplaced.is_empty(),
        "文本扫描类规则不应进入 fast 档（安全关键白名单除外）: {misplaced:?}"
    );
}
