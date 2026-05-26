use super::*;
use crate::config::hooks::{HookEntry, HookEvent, HooksConfig};
use std::collections::HashMap;

/// Helper: build a `Config` with the given hooks (everything else default).
fn config_with_hooks(events: HashMap<HookEvent, Vec<HookEntry>>) -> Config {
    let mut cfg = Config::default();
    cfg.hooks = HooksConfig { events };
    cfg
}

/// Helper: build a single `HookEntry`.
fn hook_entry(matcher: &str, command: &str) -> HookEntry {
    HookEntry {
        matcher: matcher.to_string(),
        command: command.to_string(),
        timeout: 30,
    }
}

// ---- Test 1: hooks 空合并（base 和 overlay 都没有 hooks） ----

#[test]
fn test_merge_hooks_both_empty() {
    let base = Config::default();
    let overlay = Config::default();

    let merged = ConfigManager::merge_config(base, overlay);

    assert!(merged.hooks.events.is_empty());
}

// ---- Test 2: overlay 覆盖 base hooks（同事件类型） ----

#[test]
fn test_merge_hooks_overlay_overrides_same_event() {
    let base = config_with_hooks(HashMap::from([(
        HookEvent::PreToolUse,
        vec![hook_entry("Bash", "base-hook")],
    )]));

    let overlay = config_with_hooks(HashMap::from([(
        HookEvent::PreToolUse,
        vec![
            hook_entry("Bash", "overlay-hook"),
            hook_entry("Read", "overlay-read"),
        ],
    )]));

    let merged = ConfigManager::merge_config(base, overlay);

    let pre = merged.hooks.events.get(&HookEvent::PreToolUse).unwrap();
    assert_eq!(pre.len(), 2);
    assert_eq!(pre[0].command, "overlay-hook");
    assert_eq!(pre[1].command, "overlay-read");
    // base hook should NOT appear
    assert!(!pre.iter().any(|h| h.command == "base-hook"));
}

// ---- Test 3: base 有 hooks, overlay 没有 → 保留 base ----

#[test]
fn test_merge_hooks_base_only_preserved() {
    let base = config_with_hooks(HashMap::from([(
        HookEvent::PostToolUse,
        vec![hook_entry("", "post-hook")],
    )]));
    let overlay = Config::default();

    let merged = ConfigManager::merge_config(base, overlay);

    let post = merged.hooks.events.get(&HookEvent::PostToolUse).unwrap();
    assert_eq!(post.len(), 1);
    assert_eq!(post[0].command, "post-hook");
}

// ---- Test 4: overlay 新增不同事件类型 → 两者都保留 ----

#[test]
fn test_merge_hooks_overlay_adds_new_event() {
    let base = config_with_hooks(HashMap::from([(
        HookEvent::PreToolUse,
        vec![hook_entry("Bash", "pre-hook")],
    )]));

    let overlay = config_with_hooks(HashMap::from([(
        HookEvent::Stop,
        vec![hook_entry("", "stop-hook")],
    )]));

    let merged = ConfigManager::merge_config(base, overlay);

    assert_eq!(merged.hooks.events.len(), 2);

    let pre = merged.hooks.events.get(&HookEvent::PreToolUse).unwrap();
    assert_eq!(pre.len(), 1);
    assert_eq!(pre[0].command, "pre-hook");

    let stop = merged.hooks.events.get(&HookEvent::Stop).unwrap();
    assert_eq!(stop.len(), 1);
    assert_eq!(stop[0].command, "stop-hook");
}

// ---- Test 5: Config 整体 hooks 字段 JSON 反序列化 ----

#[test]
fn test_config_hooks_deserialize() {
    let json = r#"{
            "hooks": {
                "PreToolUse": [
                    { "matcher": "Bash", "command": "echo before-bash" }
                ],
                "Stop": [
                    { "matcher": "", "command": "echo stopped", "timeout": 60 }
                ]
            }
        }"#;

    let config: Config = serde_json::from_str(json).unwrap();

    assert_eq!(config.hooks.events.len(), 2);

    let pre = config.hooks.events.get(&HookEvent::PreToolUse).unwrap();
    assert_eq!(pre.len(), 1);
    assert_eq!(pre[0].matcher, "Bash");
    assert_eq!(pre[0].command, "echo before-bash");
    assert_eq!(pre[0].timeout, 60); // default

    let stop = config.hooks.events.get(&HookEvent::Stop).unwrap();
    assert_eq!(stop.len(), 1);
    assert_eq!(stop[0].command, "echo stopped");
    assert_eq!(stop[0].timeout, 60); // explicit
}

#[tokio::test]
async fn test_load_reads_claude_settings_hooks_when_project_config_missing() {
    let base = std::env::temp_dir().join(format!(
        "aemeath_claude_settings_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let claude_dir = base.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(
        claude_dir.join("settings.json"),
        r#"{
                "hooks": {
                    "PreToolUse": [
                        {
                            "matcher": "Edit|Write",
                            "hooks": [
                                {
                                    "type": "command",
                                    "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/guard.sh",
                                    "timeout": 10
                                }
                            ]
                        }
                    ]
                }
            }"#,
    )
    .unwrap();

    let manager = ConfigManager::new(Some(&base));
    let config = manager.load().await.unwrap();

    let pre = config.hooks.events.get(&HookEvent::PreToolUse).unwrap();
    assert_eq!(pre.len(), 1);
    assert_eq!(pre[0].matcher, "Edit|Write");
    assert_eq!(
        pre[0].command,
        "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/guard.sh"
    );
    assert_eq!(pre[0].timeout, 10);

    std::fs::remove_dir_all(base).unwrap();
}

#[tokio::test]
async fn test_load_project_aemeath_config_overrides_claude_settings_hooks() {
    let base = std::env::temp_dir().join(format!(
        "aemeath_claude_settings_override_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let claude_dir = base.join(".claude");
    let agents_dir = base.join(".agents");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::create_dir_all(&agents_dir).unwrap();
    std::fs::write(
        claude_dir.join("settings.json"),
        r#"{
                "hooks": {
                    "PreToolUse": [
                        {
                            "matcher": "Edit",
                            "hooks": [
                                { "type": "command", "command": "claude-hook", "timeout": 10 }
                            ]
                        }
                    ]
                }
            }"#,
    )
    .unwrap();
    std::fs::write(
        agents_dir.join("aemeath.json"),
        r#"{
                "hooks": {
                    "PreToolUse": [
                        { "matcher": "Bash", "command": "agents-hook", "timeout": 30 }
                    ]
                }
            }"#,
    )
    .unwrap();

    let manager = ConfigManager::new(Some(&base));
    let config = manager.load().await.unwrap();

    let pre = config.hooks.events.get(&HookEvent::PreToolUse).unwrap();
    assert_eq!(pre.len(), 1);
    assert_eq!(pre[0].matcher, "Bash");
    assert_eq!(pre[0].command, "agents-hook");
    assert_eq!(pre[0].timeout, 30);

    std::fs::remove_dir_all(base).unwrap();
}
