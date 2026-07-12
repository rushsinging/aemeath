//! Hook 运行器 — 核心执行引擎

use crate::business::hook::data::{HookData, HookInput};
use crate::business::hook::result::{HookJsonOutput, HookResult};
use crate::LOG_TARGET;
use share::config::hooks::{HookEntry, HookEvent, HooksConfig};
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

    /// 从配置 HashMap 创建（兼容 CLI 层的 config_file 结构）
    pub fn from_config(config: &share::config::Config) -> Self {
        Self {
            config: config.hooks.clone(),
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
            target: LOG_TARGET,
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
            target: LOG_TARGET,
            "hook start: event={:?} matcher={} command={} workspace_root={}",
            input.event,
            hook.matcher,
            command,
            workspace_root_str
        );

        let mut command_builder = tokio::process::Command::new("sh");
        command_builder.kill_on_drop(true);
        let mut child = match command_builder
            .arg("-c")
            .arg(&command)
            .current_dir(workspace_root)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .env(
                "AEMEATH_HOOK_EVENT",
                serde_json::to_string(&input.event).unwrap_or_default(),
            )
            .env("AEMEATH_PROJECT_DIR", &workspace_root_str)
            .env("CLAUDE_PROJECT_DIR", &workspace_root_str)
            .envs(input.data.to_env_vars())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                log::warn!(
                    target: LOG_TARGET,
                    "hook spawn failed: event={:?} command={} error={}",
                    input.event,
                    command,
                    e
                );
                return HookResult::with_error(format!("启动 hook 命令失败: {e}"));
            }
        };

        // 写入 stdin 并关闭
        use tokio::io::AsyncWriteExt;
        if let Some(stdin) = child.stdin.take() {
            let mut writer = tokio::io::BufWriter::new(stdin);
            // stdin 写入或关闭失败不中断执行 — 快速 hook（如 echo）可能在
            // 我们写入前就退出关闭了读端（EPIPE），但 stdout 仍可读取。
            if let Err(e) = writer.write_all(input_json.as_bytes()).await {
                log::warn!(
                    target: LOG_TARGET,
                    "hook stdin write failed (process may have exited early): event={:?} command={} error={}",
                    input.event, command, e
                );
            }
            if let Err(e) = writer.shutdown().await {
                log::debug!(
                    target: LOG_TARGET,
                    "hook stdin shutdown: event={:?} command={} error={}",
                    input.event, command, e
                );
            }
            // stdin 在此处被关闭，进程看到 EOF
        }

        let result = tokio::select! {
            _ = cancel.cancelled() => {
                return HookResult::with_error(format!("hook '{}' 已取消", command));
            }
            result = tokio::time::timeout(timeout, child.wait_with_output()) => result,
        };

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let code = output.status.code().unwrap_or(-1);

                for line in hook_env_lines(&stdout) {
                    log::debug!(
                        target: LOG_TARGET,
                        "hook env: event={:?} command={} stream=stdout line={}",
                        input.event,
                        command,
                        line.trim()
                    );
                }
                for line in hook_env_lines(&stderr) {
                    log::debug!(
                        target: LOG_TARGET,
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
                        target: LOG_TARGET,
                        "hook '{}' exited with code {}: {}",
                        command,
                        code,
                        stderr.trim()
                    );
                }
                log::debug!(
                    target: LOG_TARGET,
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
                    error: if code != 0 {
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
            Ok(Err(e)) => {
                log::warn!(
                    target: LOG_TARGET,
                    "hook wait failed: event={:?} command={} error={}",
                    input.event,
                    command,
                    e
                );
                HookResult::with_error(format!("等待 hook 进程失败: {e}"))
            }
            Err(_) => {
                log::warn!(
                    target: LOG_TARGET,
                    "hook timeout: event={:?} command={} timeout={}s",
                    input.event,
                    command,
                    hook.timeout
                );
                HookResult::with_error(format!("hook '{}' 超时（{}秒）", command, hook.timeout))
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
                target: LOG_TARGET,
                "running hook: event={:?} matcher={} cmd={}",
                event,
                hook.matcher,
                hook.command
            );
            let result = self.execute_hook(hook, &input, workspace_root).await;
            log::debug!(
                target: LOG_TARGET,
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
                target: LOG_TARGET,
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
                target: LOG_TARGET,
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
