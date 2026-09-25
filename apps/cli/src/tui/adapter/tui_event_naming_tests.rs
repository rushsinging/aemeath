//! #1679 对账测试：承接 check-runtime-event-naming 守卫退役（TUI 层）。
//! `TuiRuntimeEvent` 变体集必须与 `.agents/runtime-event-naming-baseline.json` 对账。

use std::collections::BTreeSet;
use std::path::PathBuf;

fn baseline_variants(layer: &str) -> BTreeSet<String> {
    let raw = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.agents/runtime-event-naming-baseline.json"),
    )
    .expect("read runtime event naming baseline");
    let value: serde_json::Value = serde_json::from_str(&raw).expect("parse baseline JSON");
    value["layers"][layer]["variants"]
        .as_array()
        .expect("layer variants array")
        .iter()
        .map(|variant| variant.as_str().expect("variant name").to_owned())
        .collect()
}

fn source_variants(source: &str, enum_name: &str) -> BTreeSet<String> {
    let mut in_enum = false;
    let mut variants = BTreeSet::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        if !in_enum {
            if trimmed.contains(&format!("enum {enum_name}")) {
                in_enum = true;
            }
            continue;
        }
        if trimmed == "}" {
            break;
        }
        if trimmed.is_empty()
            || trimmed.starts_with("//")
            || trimmed.starts_with('#')
            || trimmed.starts_with("pub ")
        {
            continue;
        }
        if let Some(name) = trimmed
            .split(|ch: char| ch == ',' || ch == '(' || ch == '{' || ch.is_whitespace())
            .next()
            .filter(|name| name.starts_with(char::is_uppercase))
        {
            variants.insert(name.to_owned());
        }
    }
    variants
}

#[test]
fn tui_runtime_event_variants_match_baseline() {
    let source = include_str!("tui_runtime_event.rs");
    let actual = source_variants(source, "TuiRuntimeEvent");
    let baseline = baseline_variants("tui");

    let missing: Vec<&String> = baseline.difference(&actual).collect();
    let unexpected: Vec<&String> = actual.difference(&baseline).collect();
    assert!(
        missing.is_empty() && unexpected.is_empty(),
        "TuiRuntimeEvent 与 baseline 漂移：缺 {missing:?}；多 {unexpected:?}（同步 baseline 需评审）"
    );
}
