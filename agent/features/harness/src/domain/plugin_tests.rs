//! plugin domain 契约测试（#2030 commit 1）。

use crate::domain::plugin::{validate_plugin_rule, PluginRule, PluginSeamEngine, SeamEffect};

fn trim_rule(tool: &str, max_chars: usize) -> PluginRule {
    PluginRule::TrimOutput {
        tool: tool.to_string(),
        max_chars,
    }
}

fn hint_rule(tool: &str, pattern: &str, hint: &str) -> PluginRule {
    PluginRule::ErrorHint {
        tool: tool.to_string(),
        pattern: pattern.to_string(),
        hint: hint.to_string(),
    }
}

/// trim 命中：超预算截断 + 标记 + outcome。
#[test]
fn trim_output_truncates_with_marker() {
    let mut engine = PluginSeamEngine::new();
    engine.register("output-guard", &[trim_rule("Bash", 10)]);
    let (text, outcomes) = engine.apply("Bash", &"x".repeat(50), false);
    assert!(text.contains("（plugin seam 截断）"));
    assert_eq!(outcomes.len(), 1);
    assert!(text.chars().count() < 50);
}

/// trim 未超预算：原样返回、无 outcome。
#[test]
fn trim_output_within_budget_is_noop() {
    let mut engine = PluginSeamEngine::new();
    engine.register("output-guard", &[trim_rule("Bash", 100)]);
    let (text, outcomes) = engine.apply("Bash", "short", false);
    assert_eq!(text, "short");
    assert!(outcomes.is_empty());
}

/// 工具名不匹配：规则不生效（空 tool = 通配）。
#[test]
fn rule_scoped_by_tool_name_with_wildcard() {
    let mut engine = PluginSeamEngine::new();
    engine.register("guard", &[trim_rule("Grep", 5), trim_rule("", 3)]);
    let (text, _) = engine.apply("Bash", &"y".repeat(20), false);
    // Grep 规则不匹配 Bash；空 tool 通配命中（预算 3）
    assert!(text.contains("（plugin seam 截断）"));
    let (unmatched, _) = engine.apply("Bash", "abc", false);
    assert_eq!(unmatched, "abc");
}

/// error_hint：is_error 且命中签名才追加。
#[test]
fn error_hint_appends_on_signature_match() {
    let mut engine = PluginSeamEngine::new();
    engine.register(
        "hint-plugin",
        &[hint_rule("Bash", "exit code 1", "试试加 set -e 排查")],
    );
    let (text, outcomes) = engine.apply("Bash", "command failed: exit code 1", true);
    assert!(text.ends_with("试试加 set -e 排查"));
    assert_eq!(outcomes.len(), 1);

    let (no_error, _) = engine.apply("Bash", "exit code 1", false);
    assert!(!no_error.contains("试试"));

    let (no_match, _) = engine.apply("Bash", "other error", true);
    assert!(!no_match.contains("试试"));
}

/// trim 与 hint 正交：同轮可叠加。
#[test]
fn trim_and_hint_compose_in_one_pass() {
    let mut engine = PluginSeamEngine::new();
    engine.register(
        "combo",
        &[
            trim_rule("Bash", 10),
            hint_rule("Bash", "截断", "hint-line"),
        ],
    );
    let (text, outcomes) = engine.apply("Bash", &format!("{} error", "z".repeat(50)), true);
    assert!(text.contains("（plugin seam 截断）"));
    assert!(text.ends_with("hint-line"));
    assert_eq!(outcomes.len(), 2);
}

/// 规则校验：max_chars=0 / 空 pattern / 空 hint 拒绝。
#[test]
fn rule_validation_rejects_empty_params() {
    assert!(validate_plugin_rule(&trim_rule("Bash", 0)).is_err());
    assert!(validate_plugin_rule(&hint_rule("Bash", "", "h")).is_err());
    assert!(validate_plugin_rule(&hint_rule("Bash", "p", " ")).is_err());
    assert!(validate_plugin_rule(&trim_rule("Bash", 10)).is_ok());
}

// ---- #2063 seam v2：限流与写后验证 ----

fn rate_rule(tool: &str, max_calls: usize) -> PluginRule {
    PluginRule::RateLimit {
        tool: tool.to_string(),
        max_calls,
    }
}

fn verify_rule(tool: &str, pattern: &str) -> PluginRule {
    PluginRule::VerifyAfterWrite {
        tool: tool.to_string(),
        pattern: pattern.to_string(),
    }
}

/// 限流：超限后结果替换为限流提示；未超限原样。
#[test]
fn rate_limit_replaces_after_threshold() {
    let mut engine = PluginSeamEngine::new();
    engine.register("limiter", &[rate_rule("Bash", 2)]);
    let (first, _) = engine.apply("Bash", "ok-1", false);
    assert_eq!(first, "ok-1");
    let (second, _) = engine.apply("Bash", "ok-2", false);
    assert_eq!(second, "ok-2");
    let (third, outcomes) = engine.apply("Bash", "would-be-3", false);
    assert!(third.contains("已达本 run 调用上限 2"));
    assert!(matches!(
        outcomes[0].applied,
        SeamEffect::RateLimited { call_index: 3 }
    ));
    // 其他工具不受限
    let (other, _) = engine.apply("Grep", "fine", false);
    assert_eq!(other, "fine");
}

/// 写后验证：命中 pattern 追加验证提示。
#[test]
fn verify_after_write_appends_prompt() {
    let mut engine = PluginSeamEngine::new();
    engine.register("verifier", &[verify_rule("Edit", "The file")]);
    let (text, outcomes) = engine.apply("Edit", "The file x.rs has been updated", false);
    assert!(text.ends_with("用 Read/Grep 复核产物后再继续"));
    assert_eq!(outcomes.len(), 1);
    let (no_hit, _) = engine.apply("Edit", "no match here", false);
    assert_eq!(no_hit, "no match here");
}

/// 规则校验：max_calls=0 / 空 pattern 拒绝。
#[test]
fn v2_rule_validation() {
    assert!(validate_plugin_rule(&rate_rule("Bash", 0)).is_err());
    assert!(validate_plugin_rule(&rate_rule("Bash", 3)).is_ok());
    assert!(validate_plugin_rule(&verify_rule("Edit", "")).is_err());
    assert!(validate_plugin_rule(&verify_rule("Edit", "x")).is_ok());
}

/// 解析：两新 seam 节。
#[test]
fn parse_v2_seam_sections() {
    let rules = crate::adapters::plugins_registry::parse_plugin_rules(
        "## seam: rate_limit
tool: Bash
max_calls: 5

## seam: verify_after_write
tool: Edit
pattern: The file
",
    )
    .expect("解析");
    assert!(rules.contains(&rate_rule("Bash", 5)));
    assert!(rules.contains(&verify_rule("Edit", "The file")));
}
