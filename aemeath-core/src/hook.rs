//! Hook 执行引擎
//!
//! 在 aemeath 生命周期关键点执行用户自定义 shell 命令。
//! 通过 stdin 传入 JSON 数据，通过 exit code 控制行为。

use crate::config::hooks::{HookEntry, HookEvent, HooksConfig};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// hook 输入数据（通过 stdin 传递给 hook 命令）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookInput {
    /// 触发的事件类型
    pub event: HookEvent,
    /// 事件特定数据
    #[serde(flatten)]
    pub data: HookData,
}

/// 事件特定数据
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HookData {
    /// PreToolUse / PostToolUse 数据
    Tool(ToolHookData),
    /// UserPrompt 数据
    Prompt(PromptHookData),
    /// Stop 事件数据
    Stop(StopHookData),
}

/// 工具相关 hook 数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolHookData {
    /// 工具名
    pub tool_name: String,
    /// 工具输入参数（JSON）
    pub tool_input: serde_json::Value,
    /// 工具执行结果（仅 PostToolUse）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_output: Option<String>,
    /// 是否为错误结果（仅 PostToolUse）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

/// UserPrompt hook 数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptHookData {
    /// 用户输入文本
    pub prompt: String,
}

/// Stop 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopHookData {
    /// agent 循环执行的轮次
    pub turns: usize,
}

/// hook 执行结果
#[derive(Debug, Clone)]
pub struct HookResult {
    /// hook 是否阻止了操作（exit code 2）
    pub blocked: bool,
    /// hook 的 stdout 输出
    pub output: String,
    /// 如果 hook 执行失败，包含错误信息
    pub error: Option<String>,
}

/// Hook 运行器
#[derive(Debug, Clone)]
pub struct HookRunner {
    config: HooksConfig,
}

impl HookRunner {
    /// 从配置创建 runner
    pub fn new(config: HooksConfig) -> Self {
        Self { config }
    }

    /// 创建空 runner（无 hook 配置时使用）
    pub fn empty() -> Self {
        Self {
            config: HooksConfig::default(),
        }
    }

    /// 从配置 HashMap 创建（兼容 CLI 层的 config_file 结构）
    pub fn from_config(config: &crate::config::Config) -> Self {
        Self {
            config: config.hooks.clone(),
        }
    }

    /// 获取匹配指定事件和工具名的 hook 列表
    pub fn matching_hooks(
        &self,
        event: HookEvent,
        tool_name: Option<&str>,
    ) -> Vec<&HookEntry> {
        self.config
            .events
            .get(&event)
            .map(|hooks| {
                hooks
                    .iter()
                    .filter(|h| {
                        // 空 matcher 匹配所有
                        h.matcher.is_empty()
                            || tool_name.is_some_and(|name| name == h.matcher)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 执行单个 hook 命令
    pub async fn execute_hook(
        &self,
        hook: &HookEntry,
        input: &HookInput,
    ) -> HookResult {
        let input_json = match serde_json::to_string(input) {
            Ok(json) => json,
            Err(e) => {
                return HookResult {
                    blocked: false,
                    output: String::new(),
                    error: Some(format!("序列化 hook 输入失败: {e}")),
                };
            }
        };

        let timeout = Duration::from_secs(hook.timeout);

        let mut child = match tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&hook.command)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .env("AEMEATH_HOOK_EVENT", serde_json::to_string(&input.event).unwrap_or_default())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                return HookResult {
                    blocked: false,
                    output: String::new(),
                    error: Some(format!("启动 hook 命令失败: {e}")),
                };
            }
        };

        // 写入 stdin 并关闭
        use tokio::io::AsyncWriteExt;
        if let Some(stdin) = child.stdin.take() {
            if let Err(e) = tokio::io::BufWriter::new(stdin)
                .write_all(input_json.as_bytes())
                .await
            {
                return HookResult {
                    blocked: false,
                    output: String::new(),
                    error: Some(format!("写入 hook stdin 失败: {e}")),
                };
            }
            // stdin 在此处被 drop，进程看到 EOF
        }

        let result = tokio::time::timeout(timeout, child.wait_with_output()).await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let code = output.status.code().unwrap_or(-1);

                // Exit code 2 = 阻止操作（仅对 PreToolUse 有效）
                let blocked = code == 2;

                if code != 0 && code != 2 && !stderr.is_empty() {
                    log::warn!(
                        "hook '{}' exited with code {}: {}",
                        hook.command,
                        code,
                        stderr.trim()
                    );
                }

                HookResult {
                    blocked,
                    output: stdout,
                    error: if code != 0 && code != 2 {
                        Some(format!("exit code {code}: {stderr}"))
                    } else {
                        None
                    },
                }
            }
            Ok(Err(e)) => HookResult {
                blocked: false,
                output: String::new(),
                error: Some(format!("等待 hook 进程失败: {e}")),
            },
            Err(_) => HookResult {
                blocked: false,
                output: String::new(),
                error: Some(format!(
                    "hook '{}' 超时（{}秒）",
                    hook.command, hook.timeout
                )),
            },
        }
    }

    /// 运行指定事件的所有匹配 hook
    ///
    /// 返回所有 hook 结果。如果有任何一个 PreToolUse hook 返回 blocked，
    /// 调用方应阻止工具执行。
    pub async fn run_hooks(
        &self,
        event: HookEvent,
        tool_name: Option<&str>,
        data: HookData,
    ) -> Vec<HookResult> {
        let hooks = self.matching_hooks(event, tool_name);
        if hooks.is_empty() {
            return Vec::new();
        }

        let input = HookInput {
            event,
            data,
        };

        let mut results = Vec::with_capacity(hooks.len());
        for hook in hooks {
            log::debug!("running hook: event={:?} matcher={} cmd={}", event, hook.matcher, hook.command);
            let result = self.execute_hook(hook, &input).await;
            log::debug!("hook result: blocked={} error={:?}", result.blocked, result.error);
            results.push(result);
            // 如果被阻止，跳过后续 hook
            if results.last().is_some_and(|r| r.blocked) {
                break;
            }
        }
        results
    }

    /// 便捷方法：运行 PreToolUse hooks，返回是否应阻止
    pub async fn pre_tool_use(
        &self,
        tool_name: &str,
        tool_input: serde_json::Value,
    ) -> (bool, Vec<HookResult>) {
        let results = self
            .run_hooks(
                HookEvent::PreToolUse,
                Some(tool_name),
                HookData::Tool(ToolHookData {
                    tool_name: tool_name.to_string(),
                    tool_input,
                    tool_output: None,
                    is_error: None,
                }),
            )
            .await;
        let blocked = results.iter().any(|r| r.blocked);
        (blocked, results)
    }

    /// 便捷方法：运行 PostToolUse hooks
    pub async fn post_tool_use(
        &self,
        tool_name: &str,
        tool_input: serde_json::Value,
        tool_output: &str,
        is_error: bool,
    ) -> Vec<HookResult> {
        self.run_hooks(
            HookEvent::PostToolUse,
            Some(tool_name),
            HookData::Tool(ToolHookData {
                tool_name: tool_name.to_string(),
                tool_input,
                tool_output: Some(tool_output.to_string()),
                is_error: Some(is_error),
            }),
        )
        .await
    }

    /// 便捷方法：运行 UserPrompt hooks，返回是否应拒绝
    pub async fn user_prompt(&self, prompt: &str) -> (bool, Vec<HookResult>) {
        let results = self
            .run_hooks(
                HookEvent::UserPrompt,
                None,
                HookData::Prompt(PromptHookData {
                    prompt: prompt.to_string(),
                }),
            )
            .await;
        let blocked = results.iter().any(|r| r.blocked);
        (blocked, results)
    }

    /// 便捷方法：运行 Stop hooks
    pub async fn on_stop(&self, turns: usize) -> Vec<HookResult> {
        self.run_hooks(
            HookEvent::Stop,
            None,
            HookData::Stop(StopHookData { turns }),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let runner = HookRunner::new(config);
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
        let runner = HookRunner::new(config);

        let hooks = runner.matching_hooks(HookEvent::PreToolUse, Some("Bash"));
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].matcher, "Bash");

        let hooks = runner.matching_hooks(HookEvent::PreToolUse, Some("Write"));
        assert_eq!(hooks.len(), 0);
    }

    #[test]
    fn test_matching_hooks_no_config() {
        let runner = HookRunner::empty();
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
        let runner = HookRunner::empty();
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
        let runner = HookRunner::empty();
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
    async fn test_execute_hook_timeout() {
        let hook = HookEntry {
            matcher: String::new(),
            command: "sleep 10".to_string(),
            timeout: 1,
        };
        let runner = HookRunner::empty();
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
        let runner = HookRunner::empty();
        let (blocked, results) = runner
            .pre_tool_use("Bash", serde_json::json!({"command": "ls"}))
            .await;
        assert!(!blocked);
        assert!(results.is_empty());
    }
}
