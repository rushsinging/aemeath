//! 配置变更的应用边界。
//!
//! 此模块只比较两个有效 `Config`，不访问文件系统、环境变量或 Runtime。

use super::config::Config;

/// 配置变更可被应用的最早安全边界。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConfigApplicationScope {
    /// 当前 session 的基础设施必须重建后才能应用。
    SessionRestartRequired,
    /// 下一次 Main Run 或新建 Subagent Run 可以应用。
    Run,
}

impl ConfigApplicationScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SessionRestartRequired => "session_restart_required",
            Self::Run => "run",
        }
    }
}

/// 根据有效配置的差异返回稳定、去重的应用边界。
pub fn classify_application_scopes(before: &Config, after: &Config) -> Vec<ConfigApplicationScope> {
    let mut scopes = Vec::new();

    if session_restart_required_changed(before, after) {
        scopes.push(ConfigApplicationScope::SessionRestartRequired);
    }
    if run_scoped_changed(before, after) {
        scopes.push(ConfigApplicationScope::Run);
    }

    scopes
}

fn session_restart_required_changed(before: &Config, after: &Config) -> bool {
    value_changed(&before.ui.tui, &after.ui.tui)
        || value_changed(&before.logging.logs_dir, &after.logging.logs_dir)
        || value_changed(&before.logging.max_bytes, &after.logging.max_bytes)
        || value_changed(&before.logging.max_backups, &after.logging.max_backups)
        || value_changed(
            &before.logging.retention_days,
            &after.logging.retention_days,
        )
        || value_changed(&before.skills.dirs, &after.skills.dirs)
        || value_changed(&before.storage.sessions_dir, &after.storage.sessions_dir)
        || value_changed(&before.storage.history_file, &after.storage.history_file)
}

fn run_scoped_changed(before: &Config, after: &Config) -> bool {
    value_changed(&before.api, &after.api)
        || value_changed(&before.model, &after.model)
        || value_changed(&before.models, &after.models)
        || value_changed(&before.tools, &after.tools)
        || value_changed(&before.agents, &after.agents)
        || value_changed(&before.permissions, &after.permissions)
        || value_changed(&before.hooks, &after.hooks)
        || value_changed(&before.memory, &after.memory)
        || value_changed(&before.logging.level, &after.logging.level)
        || value_changed(
            &before.guidance.reload_policy,
            &after.guidance.reload_policy,
        )
        || value_changed(&before.language, &after.language)
}

fn value_changed<T: serde::Serialize>(before: &T, after: &T) -> bool {
    serde_json::to_value(before).ok() != serde_json::to_value(after).ok()
}

#[cfg(test)]
#[path = "scope_tests.rs"]
mod tests;
