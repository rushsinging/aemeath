use super::*;

#[test]
fn test_hooks_config_deserialize() {
    let json = r#"{
            "PreToolUse": [
                { "matcher": "Bash", "command": "echo bash-hook" }
            ],
            "PostToolUse": [
                { "matcher": "", "command": "notify-send done" }
            ],
            "Stop": [],
            "UserPromptSubmit": []
        }"#;
    let config: HooksConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.events.len(), 4);
    let pre = config.events.get(&HookEvent::PreToolUse).unwrap();
    assert_eq!(pre.len(), 1);
    assert_eq!(pre[0].matcher, "Bash");
    assert_eq!(pre[0].command, "echo bash-hook");
    assert_eq!(pre[0].timeout, 60);
}

#[test]
fn test_hooks_config_default() {
    let config = HooksConfig::default();
    assert!(config.events.is_empty());
}

#[test]
fn test_hooks_config_custom_timeout() {
    let json = r#"{
            "PreToolUse": [
                { "matcher": "", "command": "sleep 1", "timeout": 60 }
            ]
        }"#;
    let config: HooksConfig = serde_json::from_str(json).unwrap();
    let pre = config.events.get(&HookEvent::PreToolUse).unwrap();
    assert_eq!(pre[0].timeout, 60);
}

fn all_hook_events() -> Vec<(HookEvent, &'static str)> {
    vec![
        (HookEvent::PreToolUse, "PreToolUse"),
        (HookEvent::PostToolUse, "PostToolUse"),
        (HookEvent::PostToolUseFailure, "PostToolUseFailure"),
        (HookEvent::UserPromptSubmit, "UserPromptSubmit"),
        (HookEvent::Stop, "Stop"),
        (HookEvent::StopFailure, "StopFailure"),
        (HookEvent::SessionStart, "SessionStart"),
        (HookEvent::SessionEnd, "SessionEnd"),
        (HookEvent::PreCompact, "PreCompact"),
        (HookEvent::PostCompact, "PostCompact"),
        (HookEvent::PostToolBatch, "PostToolBatch"),
        (HookEvent::SubagentStart, "SubagentStart"),
        (HookEvent::SubagentStop, "SubagentStop"),
        (HookEvent::TaskCreated, "TaskCreated"),
        (HookEvent::TaskCompleted, "TaskCompleted"),
        (HookEvent::PermissionRequest, "PermissionRequest"),
        (HookEvent::PermissionDenied, "PermissionDenied"),
        (HookEvent::Notification, "Notification"),
        (HookEvent::InstructionsLoaded, "InstructionsLoaded"),
        (HookEvent::ConfigChange, "ConfigChange"),
        (HookEvent::Elicitation, "Elicitation"),
        (HookEvent::ElicitationResult, "ElicitationResult"),
        (HookEvent::UserPromptExpansion, "UserPromptExpansion"),
        (HookEvent::CwdChanged, "CwdChanged"),
        (HookEvent::FileChanged, "FileChanged"),
        (HookEvent::TeammateIdle, "TeammateIdle"),
    ]
}

#[test]
fn test_hook_event_serde_roundtrip_all_events() {
    for (event, expected_name) in all_hook_events() {
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(json, format!("\"{expected_name}\""));
        let back: HookEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, event);
    }
}

#[test]
fn test_hook_event_deserialize_all_config_keys() {
    let mut entries = Vec::new();
    for (_, name) in all_hook_events() {
        entries.push(format!(
            r#""{name}": [{{ "matcher": "", "command": "echo {name}" }}]"#
        ));
    }
    let json = format!("{{{}}}", entries.join(","));

    let config: HooksConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(config.events.len(), all_hook_events().len());
    for (event, name) in all_hook_events() {
        let hooks = config.events.get(&event).unwrap();
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].command, format!("echo {name}"));
    }
}

#[test]
fn test_hook_event_deserialize_rejects_unknown_event() {
    let err = serde_json::from_str::<HooksConfig>(
        r#"{
                "UnknownHookEvent": [
                    { "matcher": "", "command": "echo unknown" }
                ]
            }"#,
    )
    .unwrap_err();
    assert!(err.to_string().contains("unknown variant"));
}

#[test]
fn test_hook_event_deserialize_rejects_wrong_case() {
    let err = serde_json::from_str::<HooksConfig>(
        r#"{
                "preToolUse": [
                    { "matcher": "", "command": "echo wrong-case" }
                ]
            }"#,
    )
    .unwrap_err();
    assert!(err.to_string().contains("unknown variant"));
}

#[test]
fn test_hook_event_deserialize_user_prompt_submit() {
    let json = r#"{
            "UserPromptSubmit": [
                { "matcher": "", "command": "echo validate" }
            ]
        }"#;
    let config: HooksConfig = serde_json::from_str(json).unwrap();
    assert!(config.events.contains_key(&HookEvent::UserPromptSubmit));
}

#[test]
fn test_hook_event_deserialize_post_tool_use_failure() {
    let json = r#"{
            "PostToolUseFailure": [
                { "matcher": "Bash", "command": "echo failed" }
            ]
        }"#;
    let config: HooksConfig = serde_json::from_str(json).unwrap();
    assert!(config.events.contains_key(&HookEvent::PostToolUseFailure));
}
