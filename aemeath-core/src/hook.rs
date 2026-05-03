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
    /// SessionStart 事件数据
    Session(SessionHookData),
    /// PreCompact / PostCompact 事件数据
    Compact(CompactHookData),
    /// SubagentStart / SubagentStop 事件数据
    Subagent(SubagentHookData),
    // ========== P2 事件数据 ==========
    /// PermissionRequest / PermissionDenied 事件数据
    Permission(PermissionHookData),
    /// Notification 事件数据
    Notification(NotificationHookData),
    /// InstructionsLoaded 事件数据
    InstructionsLoaded(InstructionsLoadedHookData),
    /// ConfigChange 事件数据
    ConfigChange(ConfigChangeHookData),
    /// Elicitation / ElicitationResult 事件数据
    Elicitation(ElicitationHookData),
    // ========== P3 事件数据 ==========
    /// UserPromptExpansion 事件数据
    UserPromptExpansion(UserPromptExpansionHookData),
    /// CwdChanged 事件数据
    CwdChanged(CwdChangedHookData),
    /// FileChanged 事件数据
    FileChanged(FileChangedHookData),
    /// TeammateIdle 事件数据
    TeammateIdle(TeammateIdleHookData),
}

impl HookData {
    /// 将事件数据转换为环境变量（用于传递给 hook 命令）
    pub fn to_env_vars(&self) -> Vec<(&'static str, String)> {
        match self {
            HookData::Tool(d) => vec![
                ("AEMEATH_TOOL_NAME", d.tool_name.clone()),
                (
                    "AEMEATH_TOOL_INPUT",
                    serde_json::to_string(&d.tool_input).unwrap_or_default(),
                ),
            ],
            HookData::Prompt(d) => vec![("AEMEATH_PROMPT", d.prompt.clone())],
            HookData::Stop(d) => vec![("AEMEATH_STOP_TURNS", d.turns.to_string())],
            HookData::Session(_) => vec![],
            HookData::Compact(d) => vec![
                ("AEMEATH_COMPACT_TURNS", d.turns.to_string()),
                (
                    "AEMEATH_COMPACT_MESSAGES_BEFORE",
                    d.messages_before.to_string(),
                ),
                (
                    "AEMEATH_COMPACT_MESSAGES_AFTER",
                    d.messages_after.map(|n| n.to_string()).unwrap_or_default(),
                ),
            ],
            HookData::Subagent(d) => {
                let mut vars = vec![
                    ("AEMEATH_SUBAGENT_PROMPT", d.prompt.clone()),
                    ("AEMEATH_SUBAGENT_SYSTEM", d.system.clone()),
                ];
                if let Some(ref spec) = d.model_spec {
                    vars.push(("AEMEATH_SUBAGENT_MODEL_SPEC", spec.clone()));
                }
                if let Some(ref result) = d.result {
                    vars.push(("AEMEATH_SUBAGENT_RESULT", result.clone()));
                }
                if let Some(turns) = d.turns {
                    vars.push(("AEMEATH_SUBAGENT_TURNS", turns.to_string()));
                }
                if let Some(is_error) = d.is_error {
                    vars.push(("AEMEATH_SUBAGENT_IS_ERROR", is_error.to_string()));
                }
                vars
            }
            // P2 事件
            HookData::Permission(d) => vec![
                ("AEMEATH_PERMISSION_TOOL_NAME", d.tool_name.clone()),
                ("AEMEATH_PERMISSION_RULE", d.permission_rule.clone()),
            ],
            HookData::Notification(d) => vec![
                ("AEMEATH_NOTIFICATION_TEXT", d.notification_text.clone()),
                ("AEMEATH_NOTIFICATION_TYPE", d.notification_type.clone()),
            ],
            HookData::InstructionsLoaded(d) => vec![
                ("AEMEATH_INSTRUCTIONS_FILE_PATH", d.file_path.clone()),
                ("AEMEATH_INSTRUCTIONS_TYPE", d.instruction_type.clone()),
            ],
            HookData::ConfigChange(d) => {
                let mut vars = vec![("AEMEATH_CONFIG_FILE", d.config_file.clone())];
                if let Some(ref field) = d.changed_field {
                    vars.push(("AEMEATH_CONFIG_CHANGED_FIELD", field.clone()));
                }
                vars
            }
            HookData::Elicitation(d) => {
                let mut vars = vec![("AEMEATH_ELICITATION_SERVER", d.server_name.clone())];
                if let Some(ref text) = d.elicitation_text {
                    vars.push(("AEMEATH_ELICITATION_TEXT", text.clone()));
                }
                if let Some(ref response) = d.user_response {
                    vars.push(("AEMEATH_ELICITATION_RESPONSE", response.clone()));
                }
                vars
            }
            // P3 事件
            HookData::UserPromptExpansion(d) => vec![
                ("AEMEATH_PROMPT_ORIGINAL", d.original_input.clone()),
                ("AEMEATH_PROMPT_EXPANDED", d.expanded_input.clone()),
            ],
            HookData::CwdChanged(d) => vec![
                ("AEMEATH_CWD_OLD", d.old_cwd.clone()),
                ("AEMEATH_CWD_NEW", d.new_cwd.clone()),
            ],
            HookData::FileChanged(d) => vec![
                ("AEMEATH_FILE_PATH", d.file_path.clone()),
                ("AEMEATH_FILE_CHANGE_TYPE", d.change_type.clone()),
            ],
            HookData::TeammateIdle(d) => {
                let mut vars = vec![("AEMEATH_TEAMMATE_NAME", d.teammate_name.clone())];
                if let Some(ref reason) = d.idle_reason {
                    vars.push(("AEMEATH_TEAMMATE_IDLE_REASON", reason.clone()));
                }
                vars
            }
        }
    }
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

/// SessionStart 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHookData {}

/// PreCompact / PostCompact 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactHookData {
    /// agent 循环执行的轮次
    pub turns: usize,
    /// 压缩前消息数量
    pub messages_before: usize,
    /// 压缩后消息数量（仅 PostCompact）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages_after: Option<usize>,
    /// 是否实际执行了压缩
    pub was_compacted: bool,
}

/// SubagentStart / SubagentStop 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentHookData {
    /// sub-agent 的输入提示
    pub prompt: String,
    /// 系统消息
    pub system: String,
    /// 使用的模型规格（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_spec: Option<String>,
    /// 执行结果（仅 SubagentStop）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// 执行的轮次（仅 SubagentStop）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turns: Option<usize>,
    /// 是否为错误结果（仅 SubagentStop）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

// ========== P2 事件数据 ==========

/// PermissionRequest / PermissionDenied 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionHookData {
    /// 工具名
    pub tool_name: String,
    /// 权限规则
    pub permission_rule: String,
}

/// Notification 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationHookData {
    /// 通知文本
    pub notification_text: String,
    /// 通知类型
    pub notification_type: String,
}

/// InstructionsLoaded 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionsLoadedHookData {
    /// 文件路径
    pub file_path: String,
    /// 指令类型（"claude_md" / "guidance"）
    pub instruction_type: String,
}

/// ConfigChange 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigChangeHookData {
    /// 配置文件
    pub config_file: String,
    /// 变更的字段
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_field: Option<String>,
}

/// Elicitation / ElicitationResult 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationHookData {
    /// MCP 服务器名
    pub server_name: String,
    /// 请求文本（仅 Elicitation）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elicitation_text: Option<String>,
    /// 用户响应（仅 ElicitationResult）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_response: Option<String>,
}

// ========== P3 事件数据 ==========

/// UserPromptExpansion 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPromptExpansionHookData {
    /// 原始用户输入
    pub original_input: String,
    /// 展开后的输入
    pub expanded_input: String,
}

/// CwdChanged 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CwdChangedHookData {
    /// 旧工作目录
    pub old_cwd: String,
    /// 新工作目录
    pub new_cwd: String,
}

/// FileChanged 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChangedHookData {
    /// 文件路径
    pub file_path: String,
    /// 变更类型
    pub change_type: String,
}

/// TeammateIdle 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeammateIdleHookData {
    /// 队友名称
    pub teammate_name: String,
    /// 空闲原因
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_reason: Option<String>,
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
    /// 项目根目录
    project_dir: String,
}

impl HookRunner {
    /// 从配置创建 runner
    pub fn new(config: HooksConfig, project_dir: String) -> Self {
        Self {
            config,
            project_dir,
        }
    }

    /// 创建空 runner（无 hook 配置时使用）
    pub fn empty(project_dir: String) -> Self {
        Self {
            config: HooksConfig::default(),
            project_dir,
        }
    }

    /// 从配置 HashMap 创建（兼容 CLI 层的 config_file 结构）
    pub fn from_config(config: &crate::config::Config, project_dir: String) -> Self {
        Self {
            config: config.hooks.clone(),
            project_dir,
        }
    }

    /// 返回配置的 hook 事件数量（用于调试日志）
    pub fn hook_count(&self) -> usize {
        self.config.events.len()
    }

    /// 获取匹配指定事件和工具名的 hook 列表
    pub fn matching_hooks(&self, event: HookEvent, tool_name: Option<&str>) -> Vec<&HookEntry> {
        self.config
            .events
            .get(&event)
            .map(|hooks| {
                hooks
                    .iter()
                    .filter(|h| {
                        // 空 matcher 匹配所有
                        h.matcher.is_empty() || tool_name.is_some_and(|name| name == h.matcher)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 执行单个 hook 命令
    pub async fn execute_hook(&self, hook: &HookEntry, input: &HookInput) -> HookResult {
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
        let command = self.expand_command_placeholders(&hook.command);
        log::info!(
            "hook start: event={:?} matcher={} command={} project_dir={}",
            input.event,
            hook.matcher,
            command,
            self.project_dir
        );

        let mut child = match tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&command)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .env(
                "AEMEATH_HOOK_EVENT",
                serde_json::to_string(&input.event).unwrap_or_default(),
            )
            .env("AEMEATH_PROJECT_DIR", &self.project_dir)
            .envs(input.data.to_env_vars())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                log::warn!(
                    "hook spawn failed: event={:?} command={} error={}",
                    input.event,
                    command,
                    e
                );
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
            let mut writer = tokio::io::BufWriter::new(stdin);
            if let Err(e) = writer.write_all(input_json.as_bytes()).await {
                return HookResult {
                    blocked: false,
                    output: String::new(),
                    error: Some(format!("写入 hook stdin 失败: {e}")),
                };
            }
            if let Err(e) = writer.shutdown().await {
                return HookResult {
                    blocked: false,
                    output: String::new(),
                    error: Some(format!("关闭 hook stdin 失败: {e}")),
                };
            }
            // stdin 在此处被关闭，进程看到 EOF
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
                        command,
                        code,
                        stderr.trim()
                    );
                }
                log::info!(
                    "hook end: event={:?} command={} code={} blocked={} stdout_bytes={} stderr_bytes={}",
                    input.event,
                    command,
                    code,
                    blocked,
                    stdout.len(),
                    stderr.len()
                );

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
            Ok(Err(e)) => {
                log::warn!(
                    "hook wait failed: event={:?} command={} error={}",
                    input.event,
                    command,
                    e
                );
                HookResult {
                    blocked: false,
                    output: String::new(),
                    error: Some(format!("等待 hook 进程失败: {e}")),
                }
            }
            Err(_) => {
                log::warn!(
                    "hook timeout: event={:?} command={} timeout={}s",
                    input.event,
                    command,
                    hook.timeout
                );
                HookResult {
                    blocked: false,
                    output: String::new(),
                    error: Some(format!("hook '{}' 超时（{}秒）", command, hook.timeout)),
                }
            }
        }
    }

    fn expand_command_placeholders(&self, command: &str) -> String {
        command.replace("{AEMEATH_PROJECT_DIR}", &self.project_dir)
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

        let input = HookInput { event, data };

        let mut results = Vec::with_capacity(hooks.len());
        for hook in hooks {
            log::debug!(
                "running hook: event={:?} matcher={} cmd={}",
                event,
                hook.matcher,
                hook.command
            );
            let result = self.execute_hook(hook, &input).await;
            log::debug!(
                "hook result: blocked={} error={:?}",
                result.blocked,
                result.error
            );
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
                HookEvent::UserPromptSubmit,
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

    /// 便捷方法：运行 SessionStart hooks，返回 JSON 输出
    pub async fn on_session_start(&self) -> Vec<(HookEntry, HookResult, Option<HookJsonOutput>)> {
        self.run_hooks_with_json(
            HookEvent::SessionStart,
            None,
            HookData::Session(SessionHookData {}),
        )
        .await
    }

    /// 便捷方法：运行 SessionEnd hooks，返回 JSON 输出
    pub async fn on_session_end(&self) -> Vec<(HookEntry, HookResult, Option<HookJsonOutput>)> {
        self.run_hooks_with_json(
            HookEvent::SessionEnd,
            None,
            HookData::Session(SessionHookData {}),
        )
        .await
    }

    /// 便捷方法：运行 PreCompact hooks，返回是否应阻止
    pub async fn pre_compact(
        &self,
        turns: usize,
        messages_count: usize,
    ) -> (bool, Vec<HookResult>) {
        let results = self
            .run_hooks(
                HookEvent::PreCompact,
                None,
                HookData::Compact(CompactHookData {
                    turns,
                    messages_before: messages_count,
                    messages_after: None,
                    was_compacted: false,
                }),
            )
            .await;
        let blocked = results.iter().any(|r| r.blocked);
        (blocked, results)
    }

    /// 便捷方法：运行 PostCompact hooks
    pub async fn post_compact(
        &self,
        turns: usize,
        messages_before: usize,
        messages_after: usize,
    ) -> Vec<HookResult> {
        self.run_hooks(
            HookEvent::PostCompact,
            None,
            HookData::Compact(CompactHookData {
                turns,
                messages_before,
                messages_after: Some(messages_after),
                was_compacted: true,
            }),
        )
        .await
    }

    /// 便捷方法：运行 SubagentStart hooks，返回 JSON 输出
    pub async fn on_subagent_start(
        &self,
        prompt: &str,
        system: &str,
        model_spec: Option<&str>,
    ) -> Vec<(HookEntry, HookResult, Option<HookJsonOutput>)> {
        self.run_hooks_with_json(
            HookEvent::SubagentStart,
            None,
            HookData::Subagent(SubagentHookData {
                prompt: prompt.to_string(),
                system: system.to_string(),
                model_spec: model_spec.map(String::from),
                result: None,
                turns: None,
                is_error: None,
            }),
        )
        .await
    }

    /// 便捷方法：运行 SubagentStop hooks，返回 JSON 输出
    pub async fn on_subagent_stop(
        &self,
        prompt: &str,
        system: &str,
        model_spec: Option<&str>,
        result: &str,
        turns: usize,
        is_error: bool,
    ) -> Vec<(HookEntry, HookResult, Option<HookJsonOutput>)> {
        self.run_hooks_with_json(
            HookEvent::SubagentStop,
            None,
            HookData::Subagent(SubagentHookData {
                prompt: prompt.to_string(),
                system: system.to_string(),
                model_spec: model_spec.map(String::from),
                result: Some(result.to_string()),
                turns: Some(turns),
                is_error: Some(is_error),
            }),
        )
        .await
    }

    /// 便捷方法：运行 TaskCreated hooks，返回 JSON 输出
    pub async fn on_task_created(
        &self,
        tool_input: serde_json::Value,
        tool_output: &str,
    ) -> Vec<(HookEntry, HookResult, Option<HookJsonOutput>)> {
        self.run_hooks_with_json(
            HookEvent::TaskCreated,
            None,
            HookData::Tool(ToolHookData {
                tool_name: "TaskCreate".to_string(),
                tool_input,
                tool_output: Some(tool_output.to_string()),
                is_error: Some(false),
            }),
        )
        .await
    }

    /// 便捷方法：运行 TaskCompleted hooks，返回 JSON 输出
    pub async fn on_task_completed(
        &self,
        tool_input: serde_json::Value,
        tool_output: &str,
    ) -> Vec<(HookEntry, HookResult, Option<HookJsonOutput>)> {
        self.run_hooks_with_json(
            HookEvent::TaskCompleted,
            None,
            HookData::Tool(ToolHookData {
                tool_name: "TaskUpdate".to_string(),
                tool_input,
                tool_output: Some(tool_output.to_string()),
                is_error: Some(false),
            }),
        )
        .await
    }

    // ========== P2 便捷方法 ==========

    /// 便捷方法：运行 PermissionRequest hooks，返回是否应阻止
    pub async fn on_permission_request(
        &self,
        tool_name: &str,
        permission_rule: &str,
    ) -> (bool, Vec<HookResult>) {
        let results = self
            .run_hooks(
                HookEvent::PermissionRequest,
                None,
                HookData::Permission(PermissionHookData {
                    tool_name: tool_name.to_string(),
                    permission_rule: permission_rule.to_string(),
                }),
            )
            .await;
        let blocked = results.iter().any(|r| r.blocked);
        (blocked, results)
    }

    /// 便捷方法：运行 PermissionDenied hooks
    pub async fn on_permission_denied(
        &self,
        tool_name: &str,
        permission_rule: &str,
    ) -> Vec<HookResult> {
        self.run_hooks(
            HookEvent::PermissionDenied,
            None,
            HookData::Permission(PermissionHookData {
                tool_name: tool_name.to_string(),
                permission_rule: permission_rule.to_string(),
            }),
        )
        .await
    }

    /// 便捷方法：运行 Notification hooks
    pub async fn on_notification(
        &self,
        notification_text: &str,
        notification_type: &str,
    ) -> Vec<HookResult> {
        self.run_hooks(
            HookEvent::Notification,
            None,
            HookData::Notification(NotificationHookData {
                notification_text: notification_text.to_string(),
                notification_type: notification_type.to_string(),
            }),
        )
        .await
    }

    /// 便捷方法：运行 InstructionsLoaded hooks
    pub async fn on_instructions_loaded(
        &self,
        file_path: &str,
        instruction_type: &str,
    ) -> Vec<HookResult> {
        self.run_hooks(
            HookEvent::InstructionsLoaded,
            None,
            HookData::InstructionsLoaded(InstructionsLoadedHookData {
                file_path: file_path.to_string(),
                instruction_type: instruction_type.to_string(),
            }),
        )
        .await
    }

    /// 便捷方法：运行 ConfigChange hooks
    pub async fn on_config_change(
        &self,
        config_file: &str,
        changed_field: Option<&str>,
    ) -> Vec<HookResult> {
        self.run_hooks(
            HookEvent::ConfigChange,
            None,
            HookData::ConfigChange(ConfigChangeHookData {
                config_file: config_file.to_string(),
                changed_field: changed_field.map(String::from),
            }),
        )
        .await
    }

    /// 便捷方法：运行 Elicitation hooks，返回是否应阻止
    pub async fn on_elicitation(
        &self,
        server_name: &str,
        elicitation_text: &str,
    ) -> (bool, Vec<HookResult>) {
        let results = self
            .run_hooks(
                HookEvent::Elicitation,
                None,
                HookData::Elicitation(ElicitationHookData {
                    server_name: server_name.to_string(),
                    elicitation_text: Some(elicitation_text.to_string()),
                    user_response: None,
                }),
            )
            .await;
        let blocked = results.iter().any(|r| r.blocked);
        (blocked, results)
    }

    /// 便捷方法：运行 ElicitationResult hooks
    pub async fn on_elicitation_result(
        &self,
        server_name: &str,
        user_response: &str,
    ) -> Vec<HookResult> {
        self.run_hooks(
            HookEvent::ElicitationResult,
            None,
            HookData::Elicitation(ElicitationHookData {
                server_name: server_name.to_string(),
                elicitation_text: None,
                user_response: Some(user_response.to_string()),
            }),
        )
        .await
    }

    // ========== P3 便捷方法 ==========

    /// 便捷方法：运行 UserPromptExpansion hooks，返回是否应拒绝
    pub async fn on_user_prompt_expansion(
        &self,
        original_input: &str,
        expanded_input: &str,
    ) -> (bool, Vec<HookResult>) {
        let results = self
            .run_hooks(
                HookEvent::UserPromptExpansion,
                None,
                HookData::UserPromptExpansion(UserPromptExpansionHookData {
                    original_input: original_input.to_string(),
                    expanded_input: expanded_input.to_string(),
                }),
            )
            .await;
        let blocked = results.iter().any(|r| r.blocked);
        (blocked, results)
    }

    /// 便捷方法：运行 CwdChanged hooks
    pub async fn on_cwd_changed(&self, old_cwd: &str, new_cwd: &str) -> Vec<HookResult> {
        self.run_hooks(
            HookEvent::CwdChanged,
            None,
            HookData::CwdChanged(CwdChangedHookData {
                old_cwd: old_cwd.to_string(),
                new_cwd: new_cwd.to_string(),
            }),
        )
        .await
    }

    /// 便捷方法：运行 FileChanged hooks
    pub async fn on_file_changed(&self, file_path: &str, change_type: &str) -> Vec<HookResult> {
        self.run_hooks(
            HookEvent::FileChanged,
            None,
            HookData::FileChanged(FileChangedHookData {
                file_path: file_path.to_string(),
                change_type: change_type.to_string(),
            }),
        )
        .await
    }

    /// 便捷方法：运行 TeammateIdle hooks
    pub async fn on_teammate_idle(
        &self,
        teammate_name: &str,
        idle_reason: Option<&str>,
    ) -> Vec<HookResult> {
        self.run_hooks(
            HookEvent::TeammateIdle,
            None,
            HookData::TeammateIdle(TeammateIdleHookData {
                teammate_name: teammate_name.to_string(),
                idle_reason: idle_reason.map(String::from),
            }),
        )
        .await
    }

    /// 运行匹配的 hook 并解析 JSON 输出
    ///
    /// 对每个 hook 结果尝试解析 JSON 输出，如果某个 hook 的 JSON 中
    /// `continue` 为 false 或 exit_code 为 2，则中断后续 hook。
    pub async fn run_hooks_with_json(
        &self,
        event: HookEvent,
        tool_name: Option<&str>,
        data: HookData,
    ) -> Vec<(HookEntry, HookResult, Option<HookJsonOutput>)> {
        let hooks = self.matching_hooks(event, tool_name);
        if hooks.is_empty() {
            return Vec::new();
        }

        let input = HookInput { event, data };

        let mut results = Vec::with_capacity(hooks.len());
        for hook in hooks {
            log::debug!(
                "running hook (with json): event={:?} matcher={} cmd={}",
                event,
                hook.matcher,
                hook.command
            );
            let result = self.execute_hook(hook, &input).await;
            let json_output = result.parse_json_output();
            let should_break =
                result.blocked || json_output.as_ref().is_some_and(|j| !j.r#continue);
            log::debug!(
                "hook result (json): blocked={} continue={:?} error={:?}",
                result.blocked,
                json_output.as_ref().map(|j| j.r#continue),
                result.error,
            );
            results.push((hook.clone(), result, json_output));
            if should_break {
                break;
            }
        }
        results
    }
}

/// Hook 的 JSON 输出（exit 0 时 stdout 可包含此 JSON）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookJsonOutput {
    /// 是否继续执行（false 时全局停止，需配合 stopReason）
    #[serde(default = "default_true")]
    pub r#continue: bool,
    /// 停止原因
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    /// 决策（"block" 表示阻止操作）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
    /// 阻止原因
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// 额外上下文（注入到 LLM 对话流）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
    /// 系统消息（警告等，显示在 TUI）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
    /// 事件特定输出（PreToolUse 用：permission/updatedInput 等）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<serde_json::Value>,
}

fn default_true() -> bool {
    true
}

impl HookResult {
    /// 从 output 字段解析 JSON 输出
    pub fn parse_json_output(&self) -> Option<HookJsonOutput> {
        if self.output.trim().is_empty() {
            return None;
        }
        serde_json::from_str::<HookJsonOutput>(&self.output).ok()
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
                            "printf '%s\\n' \"$AEMEATH_HOOK_EVENT|$AEMEATH_PROJECT_DIR\" > \"{}\"; cat >> \"{}\"",
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
            marker_content.contains(&format!("\"Stop\"|{project_dir_string}")),
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
