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
