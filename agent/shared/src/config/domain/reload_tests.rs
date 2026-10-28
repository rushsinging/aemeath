use super::*;

#[test]
fn key_prefix_matches_classify_change_key_output() {
    // 前缀 MUST 与 runtime `classify_change_key` 的输出一致，
    // 否则分组统计会静默漏算（回显数量错误）。
    let keys = [
        "guidance:modified:_default.md",
        "instructions:deleted:AGENTS.md",
        "skills:added:skills",
    ];
    assert!(keys[0].starts_with(ComponentReloadSource::Guidance.key_prefix()));
    assert!(keys[1].starts_with(ComponentReloadSource::Instructions.key_prefix()));
    assert!(keys[2].starts_with(ComponentReloadSource::Skills.key_prefix()));
}

#[test]
fn labels_are_lowercase_source_names() {
    assert_eq!(ComponentReloadSource::Guidance.label(), "guidance");
    assert_eq!(ComponentReloadSource::Instructions.label(), "instructions");
    assert_eq!(ComponentReloadSource::Skills.label(), "skills");
}
