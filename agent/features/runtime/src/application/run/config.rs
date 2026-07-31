//! Run 级固定配置。
//!
//! 每个 Main Run 或 Subagent Run 在创建时捕获一份 committed ConfigSnapshot；
//! 后续 Step 只能消费该快照，不能重新读取 ConfigReader。

use share::config::domain::snapshot::ConfigSnapshot;

#[derive(Debug, Clone)]
pub struct RunConfigSnapshot {
    config: ConfigSnapshot,
    tool_selection: share::config::ToolSelection,
}

impl RunConfigSnapshot {
    pub fn capture(config: ConfigSnapshot) -> Self {
        let tool_selection = config.tool_selection();
        Self {
            config,
            tool_selection,
        }
    }

    pub fn revision(&self) -> share::config::domain::snapshot::ConfigRevision {
        self.config.revision()
    }

    pub fn config(&self) -> &ConfigSnapshot {
        &self.config
    }

    pub fn tool_selection(&self) -> &share::config::ToolSelection {
        &self.tool_selection
    }

    pub fn allow_all(&self) -> bool {
        self.config.allow_all()
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
