//! 组件热载来源分类。
//!
//! `SourceSnapshotRegistry` 检测到的文件变更 key（形如 `guidance:modified:x.md`）
//! 需要按来源分组生成用户可见回显；本枚举是 runtime 分组与 `share::i18n`
//! 文案共用的单一分类真相，NEVER 在消费方重复硬编码前缀字符串。
//!
//! config 文件不在本枚举内：配置热载由 `ConfigAppService` 的指纹刷新负责。

/// 组件热载来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentReloadSource {
    /// `~/.agents/guidance/*.md`。
    Guidance,
    /// `CLAUDE.md` / `AGENTS.md` 指令文件。
    Instructions,
    /// skills 目录。
    Skills,
}

impl ComponentReloadSource {
    /// 变更 key 前缀（与 runtime `classify_change_key` 输出一致）。
    pub const fn key_prefix(self) -> &'static str {
        match self {
            Self::Guidance => "guidance:",
            Self::Instructions => "instructions:",
            Self::Skills => "skills:",
        }
    }

    /// 回显标签（`[guidance]` 等）。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Guidance => "guidance",
            Self::Instructions => "instructions",
            Self::Skills => "skills",
        }
    }
}

#[cfg(test)]
#[path = "reload_tests.rs"]
mod tests;
