//! Hook 运行器 — 核心执行引擎

use crate::adapters::legacy::data::{HookData, HookInput};
use crate::adapters::legacy::result::{HookJsonOutput, HookResult};
use crate::adapters::process::{
    ProcessDriver, ProcessFailureKind, ProcessRequest, DEFAULT_OUTPUT_LIMIT,
};
use share::config::hooks::{HookEntry, HookEvent, HooksConfig};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

/// Hook 运行器
#[derive(Debug, Clone)]
pub struct HookRunner {
    pub(crate) config: HooksConfig,
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

    /// 返回配置的 hook 事件数量（用于调试日志）
    pub fn hook_count(&self) -> usize {
        self.config.events.len()
    }

    /// 获取匹配指定事件和工具名的 hook 列表
    pub fn matching_hooks(&self, event: HookEvent, tool_name: Option<&str>) -> Vec<&HookEntry> {
        let hooks = self
            .config
            .events
            .get(&event)
            .map(|hooks| {
                hooks
                    .iter()
                    .filter(|h| {
                        // 空 matcher 匹配所有
                        h.matcher.is_empty() || tool_name.is_some_and(|name| name == h.matcher)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        log::debug!(
            target: crate::LOG_TARGET,
            "hook match: event={:?} tool_name={:?} matched={} configured_events={}",
            event,
            tool_name,
            hooks.len(),
            self.hook_count(),
        );
        hooks
    }

    /// 执行单个 hook 命令。
    ///
    /// `workspace_root` 用作 hook 进程的工作目录，并注入 `AEMEATH_PROJECT_DIR` /
    /// `CLAUDE_PROJECT_DIR` 环境变量。
    pub async fn execute_hook(
        &self,
        hook: &HookEntry,
        input: &HookInput,
        workspace_root: &Path,
    ) -> HookResult {
        self.execute_hook_with_cancel(
            hook,
            input,
            workspace_root,
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
    }

    pub async fn execute_hook_with_cancel(
        &self,
        hook: &HookEntry,
        input: &HookInput,
        workspace_root: &Path,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> HookResult {
        let input_json = match serde_json::to_string(input) {
            Ok(json) => json,
            Err(e) => {
                return HookResult::with_error(format!("序列化 hook 输入失败: {e}"));
            }
        };

        let timeout = Duration::from_secs(hook.timeout);
        let workspace_root_str = workspace_root.display().to_string();
        let command = Self::expand_command_placeholders_static(&hook.command, &workspace_root_str);
        log::debug!(
            target: crate::LOG_TARGET,
            "hook start: event={:?} matcher={} command={} workspace_root={}",
            input.event,
            hook.matcher,
            command,
            workspace_root_str
        );

        let mut env = HashMap::from([
            (
                "AEMEATH_HOOK_EVENT".to_string(),
                serde_json::to_string(&input.event).unwrap_or_default(),
            ),
            (
                "AEMEATH_PROJECT_DIR".to_string(),
                workspace_root_str.clone(),
            ),
            ("CLAUDE_PROJECT_DIR".to_string(), workspace_root_str.clone()),
        ]);
        env.extend(
            input
                .data
                .to_env_vars()
                .into_iter()
                .map(|(key, value)| (key.to_string(), value)),
        );
        let result = ProcessDriver
            .execute(
                ProcessRequest {
                    command: command.clone(),
                    cwd: workspace_root.to_path_buf(),
                    env,
                    stdin: input_json.into_bytes(),
                    timeout,
                    output_limit: DEFAULT_OUTPUT_LIMIT,
                },
                cancel,
            )
            .await;

        match result {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let code = output.exit_code.unwrap_or(-1);

                for line in hook_env_lines(&stdout) {
                    log::debug!(
                        target: crate::LOG_TARGET,
                        "hook env: event={:?} command={} stream=stdout line={}",
                        input.event,
                        command,
                        line.trim()
                    );
                }
                for line in hook_env_lines(&stderr) {
                    log::debug!(
                        target: crate::LOG_TARGET,
                        "hook env: event={:?} command={} stream=stderr line={}",
                        input.event,
                        command,
                        line.trim()
                    );
                }

                // 任意非零退出码都表示 hook 阻止当前流程继续。
                let blocked = code != 0;

                if code != 0 && !stderr.is_empty() {
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "hook '{}' exited with code {}: {}",
                        command,
                        code,
                        stderr.trim()
                    );
                }
                log::debug!(
                    target: crate::LOG_TARGET,
                    "hook end: event={:?} command={} code={} blocked={} stdout_bytes={} stderr_bytes={} stdout_truncated={} stderr_truncated={}",
                    input.event,
                    command,
                    code,
                    blocked,
                    stdout.len(),
                    stderr.len(),
                    output.stdout_truncated,
                    output.stderr_truncated
                );

                let output_truncated = output.stdout_truncated || output.stderr_truncated;
                HookResult {
                    blocked,
                    output: if output_truncated {
                        String::new()
                    } else {
                        stdout
                    },
                    error: if output_truncated {
                        Some(format!(
                            "hook 输出超过 {} 字节上限，结果已截断",
                            DEFAULT_OUTPUT_LIMIT
                        ))
                    } else if code != 0 {
                        Some(format!(
                            "exit code {code}: {}",
                            non_empty_text(&stderr).unwrap_or_else(|| "无错误输出".to_string())
                        ))
                    } else {
                        None
                    },
                    exit_code: Some(code),
                }
            }
            Err(failure) => {
                let reason = match failure.kind {
                    ProcessFailureKind::Timeout => "timeout",
                    ProcessFailureKind::Cancelled => "cancelled",
                    ProcessFailureKind::Spawn => "spawn_failed",
                    ProcessFailureKind::Io => "io_failed",
                    ProcessFailureKind::Wait => "wait_failed",
                    #[cfg(not(unix))]
                    ProcessFailureKind::Unsupported => "unsupported",
                };
                log::warn!(
                    target: crate::LOG_TARGET,
                    "hook execution failed: event={:?} command={} reason={} error={}",
                    input.event,
                    command,
                    reason,
                    failure.message
                );
                HookResult::with_error(failure.message)
            }
        }
    }

    pub(crate) fn expand_command_placeholders_static(
        command: &str,
        workspace_root: &str,
    ) -> String {
        command
            .replace("{AEMEATH_PROJECT_DIR}", workspace_root)
            .replace("{CLAUDE_PROJECT_DIR}", workspace_root)
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
        workspace_root: &Path,
    ) -> Vec<HookResult> {
        let hooks = self.matching_hooks(event, tool_name);
        if hooks.is_empty() {
            return Vec::new();
        }

        let input = HookInput { event, data };

        let mut results = Vec::with_capacity(hooks.len());
        for hook in hooks {
            log::debug!(
                target: crate::LOG_TARGET,
                "running hook: event={:?} matcher={} cmd={}",
                event,
                hook.matcher,
                hook.command
            );
            let result = self.execute_hook(hook, &input, workspace_root).await;
            log::debug!(
                target: crate::LOG_TARGET,
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

    /// 运行匹配的 hook 并解析 JSON 输出
    ///
    /// 对每个 hook 结果尝试解析 JSON 输出，如果某个 hook 的 JSON 中
    /// `continue` 为 false 或 exit_code 为 2，则中断后续 hook。
    pub async fn run_hooks_with_json(
        &self,
        event: HookEvent,
        tool_name: Option<&str>,
        data: HookData,
        workspace_root: &Path,
    ) -> Vec<(HookEntry, HookResult, Option<HookJsonOutput>)> {
        let hooks = self.matching_hooks(event, tool_name);
        if hooks.is_empty() {
            return Vec::new();
        }

        let input = HookInput { event, data };

        let mut results = Vec::with_capacity(hooks.len());
        for hook in hooks {
            log::debug!(
                target: crate::LOG_TARGET,
                "running hook (with json): event={:?} matcher={} cmd={}",
                event,
                hook.matcher,
                hook.command
            );
            let result = self.execute_hook(hook, &input, workspace_root).await;
            let json_output = result.parse_json_output();
            let should_break =
                result.blocked || json_output.as_ref().is_some_and(|j| !j.r#continue);
            log::debug!(
                target: crate::LOG_TARGET,
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

    /// 运行 blocking 类型的 hook（返回是否 blocked + 所有结果）
    pub(crate) async fn run_blocking_hooks(
        &self,
        event: HookEvent,
        tool_name: Option<&str>,
        data: HookData,
        workspace_root: &Path,
    ) -> (bool, Vec<HookResult>) {
        let results = self.run_hooks(event, tool_name, data, workspace_root).await;
        let blocked = results.iter().any(|r| r.blocked);
        (blocked, results)
    }
}

pub(crate) fn hook_env_lines(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| line.trim_start().starts_with("[hook-env]"))
        .collect()
}

fn non_empty_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;
