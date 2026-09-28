fn base_registry_json() -> String {
    serde_json::json!({
        "version": 1,
        "budgets": { "repository_migration_debt": 0 },
        "construction_symbols": [],
        "entries": []
    })
    .to_string()
}

#[test]
fn registry_accepts_rules_and_retired_symbols_sections() {
    let mut payload = serde_json::from_str::<serde_json::Value>(&base_registry_json())
        .expect("parse base registry");
    payload["rules"] = serde_json::json!([
        {
            "id": "use.feature.no-internal-segments",
            "assertion": "forbidden_segments",
            "scope": { "kind": "path_prefix", "value": "agent" },
            "forbidden_segments": ["domain"],
            "reason": "内部层禁穿透",
            "profile": "fast"
        }
    ]);
    payload["retired_symbols"] = serde_json::json!([
        { "symbol": "CostTracker", "retired_by": "audit", "reason": "cost 退役" }
    ]);

    let report =
        crate::guard_registry::validate_str(&payload.to_string()).expect("validate registry");

    let rendered = report.render();
    assert!(rendered.contains("rules: 1"));
    assert!(rendered.contains("retired_symbols: 1"));
}

#[test]
fn registry_rejects_duplicate_rule_ids() {
    let mut payload = serde_json::from_str::<serde_json::Value>(&base_registry_json())
        .expect("parse base registry");
    let rule = serde_json::json!({
        "id": "use.feature.no-internal-segments",
        "assertion": "forbidden_segments",
        "scope": { "kind": "path_prefix", "value": "agent" },
        "forbidden_segments": ["domain"]
    });
    payload["rules"] = serde_json::json!([rule, rule]);

    let error = crate::guard_registry::validate_str(&payload.to_string())
        .expect_err("duplicate rule id must fail");

    assert!(error.to_string().contains("重复"));
}

#[test]
fn registry_rejects_unknown_assertion_kind() {
    let mut payload = serde_json::from_str::<serde_json::Value>(&base_registry_json())
        .expect("parse base registry");
    payload["rules"] = serde_json::json!([
        {
            "id": "use.feature.bogus",
            "assertion": "telepathy",
            "scope": { "kind": "path_prefix", "value": "agent" }
        }
    ]);

    let error = crate::guard_registry::validate_str(&payload.to_string())
        .expect_err("unknown assertion must fail");

    assert!(error.to_string().contains("解析架构 Guard 注册表失败"));
}

/// 豁免基线只降不升：`exclusion_baseline` 登记当前存量上限，
/// 新增内联测试（exclusions 增长）时 registry check 必须报错。
#[test]
fn registry_rejects_exclusion_count_above_baseline() {
    let payload = serde_json::json!({
        "version": 1,
        "budgets": { "repository_migration_debt": 0 },
        "construction_symbols": [],
        "entries": [],
        "rules": [
            {
                "id": "pattern.all.no-inline-test-modules",
                "assertion": "pattern_exclusion",
                "scope": { "kind": "workspace", "value": "" },
                "forbidden_patterns": ["mod tests {"],
                "exclusions": [{ "path": "a.rs" }, { "path": "b.rs" }],
                "exclusion_baseline": 1,
                "reason": "test",
                "profile": "full"
            }
        ],
        "retired_symbols": []
    });

    let error = crate::guard_registry::validate_str(payload.to_string().as_str())
        .expect_err("exclusions 超基线必须被拒绝");
    assert!(
        error.to_string().contains("exclusion_baseline"),
        "错误信息应指明基线约束，实际：{error}"
    );
}

#[test]
fn registry_accepts_exclusion_count_below_baseline() {
    let payload = serde_json::json!({
        "version": 1,
        "budgets": { "repository_migration_debt": 0 },
        "construction_symbols": [],
        "entries": [],
        "rules": [
            {
                "id": "pattern.all.no-inline-test-modules",
                "assertion": "pattern_exclusion",
                "scope": { "kind": "workspace", "value": "" },
                "forbidden_patterns": ["mod tests {"],
                "exclusions": [{ "path": "a.rs" }],
                "exclusion_baseline": 2,
                "reason": "test",
                "profile": "full"
            }
        ],
        "retired_symbols": []
    });

    let report =
        crate::guard_registry::validate_str(payload.to_string().as_str()).expect("低于基线应通过");
    assert_eq!(report.rules, 1);
}
