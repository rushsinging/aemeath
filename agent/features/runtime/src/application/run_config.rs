//! Run 级固定配置。
//!
//! 每个 Main Run 或 Subagent Run 在创建时捕获一份 committed ConfigSnapshot；
//! 后续 Step 只能消费该快照，不能重新读取 ConfigReader。

use share::config::domain::snapshot::ConfigSnapshot;

#[derive(Debug, Clone)]
pub struct RunConfigSnapshot {
    config: ConfigSnapshot,
}

impl RunConfigSnapshot {
    pub fn capture(config: ConfigSnapshot) -> Self {
        Self { config }
    }

    pub fn revision(&self) -> share::config::domain::snapshot::ConfigRevision {
        self.config.revision()
    }

    pub fn config(&self) -> &ConfigSnapshot {
        &self.config
    }

    pub fn allow_all(&self) -> bool {
        self.config.allow_all()
    }
}

#[cfg(test)]
#[path = "run_config_tests.rs"]
mod tests;
