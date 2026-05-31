use crate::utils::bootstrap::claude_settings_adapter::ClaudeSettingsAdapter;
use share::config::hooks::{ClaudeSettingsConfig, HookEvent};

#[test]
fn test_claude_settings_config_converts_nested_hooks() {
    let json = r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Edit|Write|MultiEdit",
                        "hooks": [
                            {
                                "type": "command",
                                "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/guard-deps.sh",
                                "timeout": 10
                            },
                            {
                                "type": "command",
                                "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/guard-env-templates.sh"
                            }
                        ]
                    }
                ],
                "Stop": [
                    {
                        "hooks": [
                            {
                                "type": "command",
                                "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/stop-verify.sh",
                                "timeout": 600
                            }
                        ]
                    }
                ]
            }
        }"#;

    let settings: ClaudeSettingsConfig = serde_json::from_str(json).unwrap();
    let hooks = settings.into_hooks_config();

    let pre = hooks.events.get(&HookEvent::PreToolUse).unwrap();
    assert_eq!(pre.len(), 2);
    assert_eq!(pre[0].matcher, "Edit|Write|MultiEdit");
    assert_eq!(
        pre[0].command,
        "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/guard-deps.sh"
    );
    assert_eq!(pre[0].timeout, 10);
    assert_eq!(pre[1].timeout, 60);

    let stop = hooks.events.get(&HookEvent::Stop).unwrap();
    assert_eq!(stop.len(), 1);
    assert_eq!(stop[0].matcher, "");
    assert_eq!(stop[0].timeout, 600);
}

#[test]
fn test_claude_settings_config_ignores_empty_commands() {
    let json = r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            { "type": "command", "command": "" },
                            { "type": "command", "command": "echo ok" }
                        ]
                    }
                ]
            }
        }"#;

    let settings: ClaudeSettingsConfig = serde_json::from_str(json).unwrap();
    let hooks = settings.into_hooks_config();

    let pre = hooks.events.get(&HookEvent::PreToolUse).unwrap();
    assert_eq!(pre.len(), 1);
    assert_eq!(pre[0].command, "echo ok");
}
