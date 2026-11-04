//! `aemeath cleanup` 子命令的落盘清理配置（`cleanup.*`）。
//!
//! 判据分两类（见 specs/3.10-storage.md）：
//! - 无引用清理（workflows 孤儿实体等）不涉及保留天数；
//! - 过期清理（session 三件套 / audit / tmp / legacy 旧布局）按
//!   `cleanup.retention_days` 判定，分项可覆盖全局默认。

use serde::{Deserialize, Serialize};

pub use super::constants::DEFAULT_CLEANUP_RETENTION_DAYS;

/// 过期清理的可配置面。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CleanupFace {
    /// session 三件套联动：`state/sessions/<id>*`、
    /// `state/session/<id>.dataset/`、`state/tool-results/<id>/`。
    Sessions,
    /// `state/audit/usage/*.jsonl` 等审计落盘。
    Audit,
    /// `state/tmp/` 运行期临时文件。
    Tmp,
    /// `~/.agents` 旧布局遗留 state 面（只读 fallback 目录）。
    Legacy,
}

impl CleanupFace {
    /// 配置键名（`cleanup.retention_days.<key>`）。
    pub const fn key(self) -> &'static str {
        match self {
            Self::Sessions => "sessions",
            Self::Audit => "audit",
            Self::Tmp => "tmp",
            Self::Legacy => "legacy",
        }
    }
}

/// `cleanup.retention_days`：全局默认 + per 面分项。
///
/// 分项缺省继承 `default`；显式 `0` 表示禁用该面（跳过清理）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupRetentionDaysConfig {
    /// 全局默认保留天数（默认 30；`0` 禁用所有未覆盖面）。
    #[serde(default = "default_cleanup_retention_days")]
    pub default: u64,
    /// session 三件套联动面。
    #[serde(default)]
    pub sessions: Option<u64>,
    /// 审计落盘面。
    #[serde(default)]
    pub audit: Option<u64>,
    /// 临时文件面。
    #[serde(default)]
    pub tmp: Option<u64>,
    /// 旧布局遗留面（`~/.agents` 旧 state 目录）。
    #[serde(default)]
    pub legacy: Option<u64>,
}

pub(crate) const fn default_cleanup_retention_days() -> u64 {
    DEFAULT_CLEANUP_RETENTION_DAYS
}

impl Default for CleanupRetentionDaysConfig {
    fn default() -> Self {
        Self {
            default: DEFAULT_CLEANUP_RETENTION_DAYS,
            sessions: None,
            audit: None,
            tmp: None,
            legacy: None,
        }
    }
}

impl CleanupRetentionDaysConfig {
    fn face_override(&self, face: CleanupFace) -> Option<u64> {
        match face {
            CleanupFace::Sessions => self.sessions,
            CleanupFace::Audit => self.audit,
            CleanupFace::Tmp => self.tmp,
            CleanupFace::Legacy => self.legacy,
        }
    }

    /// 解析某面的有效保留天数。
    ///
    /// 分项 `Some(0)` 显式禁用该面；否则分项覆盖全局 `default`
    ///（`default = 0` 同样视为禁用）。返回 `None` 表示该面跳过。
    pub fn effective_days(&self, face: CleanupFace) -> Option<u64> {
        match self.face_override(face) {
            Some(0) => None,
            Some(days) => Some(days),
            None if self.default == 0 => None,
            None => Some(self.default),
        }
    }
}

/// `cleanup.*` 节点。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CleanupConfig {
    /// 过期清理保留天数（全局默认 + per 面分项）。
    #[serde(default)]
    pub retention_days: CleanupRetentionDaysConfig,
}

#[cfg(test)]
#[path = "cleanup_tests.rs"]
mod tests;
