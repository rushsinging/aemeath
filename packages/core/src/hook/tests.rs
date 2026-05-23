#[cfg(test)]
mod hook_tests {
    use crate::config::hooks::{HookEntry, HookEvent, HooksConfig};
    use crate::hook::data::*;
    use crate::hook::runner::HookRunner;
    use std::collections::HashMap;

    #[test]
    fn test_matching_hooks_empty_matcher() {
        let config = HooksConfig {
            events: {
                let mut map = HashMap::new();
                map.insert(
                    HookEvent::PreToolUse,
                    vec![HookEntry {
                        matcher: String::new(),
                        command: "echo all".to_string(),
                        timeout: 30,
                    }],
                );
                map
            },
        };
        let runner = HookRunner::new(config, ".".to_string());
        let hooks = runner.matching_hooks(HookEvent::PreToolUse, Some("Bash"));
        assert_eq!(hooks.len(), 1);
    }

    #[test]
    fn test_matching_hooks_specific_matcher() {
        let config = HooksConfig {
            events: {
                let mut map = HashMap::new();
                map.insert(
                    HookEvent::PreToolUse,
                    vec![
                        HookEntry {
                            matcher: "Bash".to_string(),
                            command: "echo bash".to_string(),
                            timeout: 30,
                        },
                        HookEntry {
                            matcher: "Read".to_string(),
                            command: "echo read".to_string(),
                            timeout: 30,
                        },
                    ],
                );
                map
            },
        };
        let runner = HookRunner::new(config, ".".to_string());

        let hooks = runner.matching_hooks(HookEvent::PreToolUse, Some("Bash"));
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].matcher, "Bash");

        let hooks = runner.matching_hooks(HookEvent::PreToolUse, Some("Write"));
        assert_eq!(hooks.len(), 0);
    }

    #[test]
    fn test_matching_hooks_no_config() {
        let runner = HookRunner::empty(".".to_string());
        let hooks = runner.matching_hooks(HookEvent::PreToolUse, Some("Bash"));
        assert!(hooks.is_empty());
    }

    #[tokio::test]
    async fn test_execute_hook_success() {
        let hook = HookEntry {
            matcher: String::new(),
            command: "echo 'hello from hook'".to_string(),
            timeout: 5,
        };
        let runner = HookRunner::empty(".".to_string());
        let input = HookInput {
            event: HookEvent::PreToolUse,
            data: HookData::Tool(ToolHookData {
                tool_name: "Bash".to_string(),
                tool_input: serde_json::json!({"command": "ls"}),
                tool_output: None,
                is_error: None,
            }),
        };
        let result = runner.execute_hook(&hook, &input).await;
        assert!(!result.blocked);
        assert!(result.output.contains("hello from hook"));
        assert!(result.error.is_none());
    }

    #[tokio::test]
    async fn test_execute_hook_block() {
        let hook = HookEntry {
            matcher: String::new(),
            command: "exit 2".to_string(),
            timeout: 5,
        };
        let runner = HookRunner::empty(".".to_string());
        let input = HookInput {
            event: HookEvent::PreToolUse,
            data: HookData::Tool(ToolHookData {
                tool_name: "Bash".to_string(),
                tool_input: serde_json::json!({}),
                tool_output: None,
                is_error: None,
            }),
        };
        let result = runner.execute_hook(&hook, &input).await;
        assert!(result.blocked);
    }

    #[tokio::test]
    async fn test_execute_hook_blocks_on_any_nonzero_exit_code() {
        let hook = HookEntry {
            matcher: String::new(),
            command: "printf 'bad thing' >&2; exit 1".to_string(),
            timeout: 5,
        };
        let runner = HookRunner::empty(".".to_string());
        let input = HookInput {
            event: HookEvent::Stop,
            data: HookData::Stop(StopHookData { turns: 1 }),
        };

        let result = runner.execute_hook(&hook, &input).await;

        assert!(result.blocked);
        assert!(
            matches!(result.error.as_deref(), Some(error) if error.contains("exit code 1") && error.contains("bad thing"))
        );
    }

    #[tokio::test]
    async fn test_execute_hook_nonzero_without_stderr_still_reports_error() {
        let hook = HookEntry {
            matcher: String::new(),
            command: "exit 1".to_string(),
            timeout: 5,
        };
        let runner = HookRunner::empty(".".to_string());
        let input = HookInput {
            event: HookEvent::Stop,
            data: HookData::Stop(StopHookData { turns: 1 }),
        };

        let result = runner.execute_hook(&hook, &input).await;

        assert!(result.blocked);
        assert!(
            matches!(result.error.as_deref(), Some(error) if error.contains("exit code 1") && error.contains("无错误输出"))
        );
    }

    #[tokio::test]
    async fn test_execute_hook_timeout() {
        let hook = HookEntry {
            matcher: String::new(),
            command: "sleep 10".to_string(),
            timeout: 1,
        };
        let runner = HookRunner::empty(".".to_string());
        let input = HookInput {
            event: HookEvent::PreToolUse,
            data: HookData::Tool(ToolHookData {
                tool_name: "Bash".to_string(),
                tool_input: serde_json::json!({}),
                tool_output: None,
                is_error: None,
            }),
        };
        let result = runner.execute_hook(&hook, &input).await;
        assert!(result.error.is_some());
        assert!(result.error.as_ref().unwrap().contains("超时"));
    }

    #[tokio::test]
    async fn test_pre_tool_use_no_hooks() {
        let runner = HookRunner::empty(".".to_string());
        let (blocked, results) = runner
            .pre_tool_use("Bash", serde_json::json!({"command": "ls"}))
            .await;
        assert!(!blocked);
        assert!(results.is_empty());
    }

    #[test]
    fn test_expand_command_placeholders_project_dir() {
        let runner = HookRunner::empty("/tmp/aemeath-project".to_string());
        let command =
            runner.expand_command_placeholders("\"{AEMEATH_PROJECT_DIR}/build.sh\" --check");

        assert_eq!(command, "\"/tmp/aemeath-project/build.sh\" --check");
    }

    #[test]
    fn test_expand_command_placeholders_without_placeholder() {
        let runner = HookRunner::empty("/tmp/aemeath-project".to_string());
        let command = runner.expand_command_placeholders("cargo check");

        assert_eq!(command, "cargo check");
    }

    #[tokio::test]
    async fn test_execute_hook_sets_claude_project_dir_env() {
        let project_dir =
            std::env::temp_dir().join(format!("aemeath-claude-env-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(project_dir.join(".claude").join("hooks")).unwrap();
        let script = project_dir
            .join(".claude")
            .join("hooks")
            .join("print-dir.sh");
        std::fs::write(&script, "#!/bin/sh\nprintf '%s' \"$CLAUDE_PROJECT_DIR\"\n").unwrap();

        let hook = HookEntry {
            matcher: String::new(),
            command: format!("sh \"{}\"", script.display()),
            timeout: 5,
        };
        let project_dir_string = project_dir.display().to_string();
        let runner = HookRunner::empty(project_dir_string.clone());
        let input = HookInput {
            event: HookEvent::Stop,
            data: HookData::Stop(StopHookData { turns: 1 }),
        };

        let result = runner.execute_hook(&hook, &input).await;

        assert!(!result.blocked);
        assert!(result.error.is_none());
        assert_eq!(result.output, project_dir_string);

        let _ = std::fs::remove_dir_all(&project_dir);
    }

    #[tokio::test]
    async fn test_execute_hook_expands_project_dir_placeholder() {
        let project_dir = std::env::current_dir().unwrap().display().to_string();
        let hook = HookEntry {
            matcher: String::new(),
            command: "printf '%s' \"{AEMEATH_PROJECT_DIR}\"".to_string(),
            timeout: 5,
        };
        let runner = HookRunner::empty(project_dir.clone());
        let input = HookInput {
            event: HookEvent::Stop,
            data: HookData::Stop(StopHookData { turns: 1 }),
        };

        let result = runner.execute_hook(&hook, &input).await;

        assert!(!result.blocked);
        assert!(result.error.is_none());
        assert_eq!(result.output, project_dir);
    }

    #[tokio::test]
    async fn test_on_stop_runs_configured_hook_with_event_and_project_dir() {
        let project_dir =
            std::env::temp_dir().join(format!("aemeath-stop-hook-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&project_dir).unwrap();
        let marker = project_dir.join("stop-hook.marker");
        let marker_path = marker.display().to_string();
        let project_dir_string = project_dir.display().to_string();

        let config = HooksConfig {
            events: {
                let mut map = HashMap::new();
                map.insert(
                    HookEvent::Stop,
                    vec![HookEntry {
                        matcher: String::new(),
                        command: format!(
                            "printf '%s\\n' \"$AEMEATH_HOOK_EVENT|$AEMEATH_PROJECT_DIR|$CLAUDE_PROJECT_DIR\" > \"{}\"; cat >> \"{}\"",
                            marker_path, marker_path
                        ),
                        timeout: 5,
                    }],
                );
                map
            },
        };
        let runner = HookRunner::new(config, project_dir_string.clone());

        let results = runner.on_stop(7).await;

        assert_eq!(results.len(), 1);
        assert!(!results[0].blocked);
        assert!(results[0].error.is_none());
        assert!(marker.exists());
        let marker_content = std::fs::read_to_string(&marker).unwrap();
        assert!(
            marker_content.contains(&format!(
                "\"Stop\"|{project_dir_string}|{project_dir_string}"
            )),
            "marker content: {marker_content:?}"
        );
        let json_start = marker_content
            .find('{')
            .unwrap_or_else(|| panic!("marker content: {marker_content:?}"));
        let hook_input: HookInput = serde_json::from_str(&marker_content[json_start..]).unwrap();
        assert_eq!(hook_input.event, HookEvent::Stop);
        match hook_input.data {
            HookData::Stop(data) => assert_eq!(data.turns, 7),
            other => panic!("expected Stop hook data, got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&project_dir);
    }
}
