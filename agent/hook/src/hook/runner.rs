//! Hook 运行器 — 核心执行引擎

use crate::hook::data::{HookData, HookInput};
use crate::hook::result::{HookJsonOutput, HookResult};
use aemeath_core::config::hooks::{HookEntry, HookEvent, HooksConfig};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Hook 运行器
#[derive(Debug, Clone)]
pub struct HookRunner {
    pub(crate) config: HooksConfig,
    /// 项目根目录
    pub(crate) project_dir: Arc<Mutex<String>>,
}

impl HookRunner {
    /// 从配置创建 runner
    pub fn new(config: HooksConfig, project_dir: String) -> Self {
        Self {
            config,
            project_dir: Arc::new(Mutex::new(project_dir)),
        }
    }

    /// 创建空 runner（无 hook 配置时使用）
    pub fn empty(project_dir: String) -> Self {
        Self {
            config: HooksConfig::default(),
            project_dir: Arc::new(Mutex::new(project_dir)),
        }
    }

    /// 从配置 HashMap 创建（兼容 CLI 层的 config_file 结构）
    pub fn from_config(config: &aemeath_core::config::Config, project_dir: String) -> Self {
        Self {
            config: config.hooks.clone(),
            project_dir: Arc::new(Mutex::new(project_dir)),
        }
    }

    /// 返回配置的 hook 事件数量（用于调试日志）
    pub fn hook_count(&self) -> usize {
        self.config.events.len()
    }

    /// 返回当前 hook 项目目录。
    pub fn project_dir(&self) -> String {
        self.project_dir
            .lock()
            .map(|p| p.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }

    /// 更新 hook 项目目录，用于 worktree/cwd 切换后同步内置环境变量。
    pub fn set_project_dir(&self, project_dir: String) {
        match self.project_dir.lock() {
            Ok(mut current) => *current = project_dir,
            Err(poisoned) => *poisoned.into_inner() = project_dir,
        }
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
                return HookResult::with_error(format!("序列化 hook 输入失败: {e}"));
            }
        };

        let timeout = Duration::from_secs(hook.timeout);
        let project_dir = self.project_dir();
        let command = self.expand_command_placeholders(&hook.command);
        log::info!(
            "hook start: event={:?} matcher={} command={} project_dir={}",
            input.event,
            hook.matcher,
            command,
            project_dir
        );

        let mut child = match tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&command)
            .current_dir(&project_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .env(
                "AEMEATH_HOOK_EVENT",
                serde_json::to_string(&input.event).unwrap_or_default(),
            )
            .env("AEMEATH_PROJECT_DIR", &project_dir)
            .env("CLAUDE_PROJECT_DIR", &project_dir)
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
                return HookResult::with_error(format!("启动 hook 命令失败: {e}"));
            }
        };

        // 写入 stdin 并关闭
        use tokio::io::AsyncWriteExt;
        if let Some(stdin) = child.stdin.take() {
            let mut writer = tokio::io::BufWriter::new(stdin);
            if let Err(e) = writer.write_all(input_json.as_bytes()).await {
                return HookResult::with_error(format!("写入 hook stdin 失败: {e}"));
            }
            if let Err(e) = writer.shutdown().await {
                return HookResult::with_error(format!("关闭 hook stdin 失败: {e}"));
            }
            // stdin 在此处被关闭，进程看到 EOF
        }

        let result = tokio::time::timeout(timeout, child.wait_with_output()).await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let code = output.status.code().unwrap_or(-1);

                // 任意非零退出码都表示 hook 阻止当前流程继续。
                let blocked = code != 0;

                if code != 0 && !stderr.is_empty() {
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
                    error: if code != 0 {
                        Some(format!(
                            "exit code {code}: {}",
                            non_empty_text(&stderr).unwrap_or_else(|| "无错误输出".to_string())
                        ))
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
                HookResult::with_error(format!("等待 hook 进程失败: {e}"))
            }
            Err(_) => {
                log::warn!(
                    "hook timeout: event={:?} command={} timeout={}s",
                    input.event,
                    command,
                    hook.timeout
                );
                HookResult::with_error(format!("hook '{}' 超时（{}秒）", command, hook.timeout))
            }
        }
    }

    pub(crate) fn expand_command_placeholders(&self, command: &str) -> String {
        let project_dir = self.project_dir();
        command
            .replace("{AEMEATH_PROJECT_DIR}", &project_dir)
            .replace("{CLAUDE_PROJECT_DIR}", &project_dir)
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

    /// 运行 blocking 类型的 hook（返回是否 blocked + 所有结果）
    pub(crate) async fn run_blocking_hooks(
        &self,
        event: HookEvent,
        tool_name: Option<&str>,
        data: HookData,
    ) -> (bool, Vec<HookResult>) {
        let results = self.run_hooks(event, tool_name, data).await;
        let blocked = results.iter().any(|r| r.blocked);
        (blocked, results)
    }
}

fn non_empty_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
