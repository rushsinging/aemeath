//! 组件标识、内容与 diff。

use std::fmt;

/// 组件类别（8 个可编辑槽位；声明序即 `list()` 的 kind 排序序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ComponentKind {
    Guardrails,
    MemoryView,
    Plugin,
    Script,
    Skill,
    SubAgents,
    SystemPromptOverlay,
    ToolGuidance,
}

impl ComponentKind {
    /// 稳定的日志/展示名（小写连字符）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Guardrails => "guardrails",
            Self::MemoryView => "memory-view",
            Self::Plugin => "plugin",
            Self::Script => "script",
            Self::Skill => "skill",
            Self::SubAgents => "sub-agents",
            Self::SystemPromptOverlay => "system-prompt-overlay",
            Self::ToolGuidance => "tool-guidance",
        }
    }
}

/// 多实例组件（skills/scripts/…）内的实例名；单例组件用固定名。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ComponentName(String);

impl ComponentName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ComponentName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// 组件唯一标识：类别 + 实例名。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ComponentId {
    pub kind: ComponentKind,
    pub name: ComponentName,
}

impl fmt::Display for ComponentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.kind.as_str(), self.name)
    }
}

/// 组件结构化正文：frontmatter 字段 + markdown 正文（非裸字节）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ComponentContent {
    /// YAML frontmatter 解析后的结构化字段（无 frontmatter 的组件为 None）。
    pub frontmatter: Option<serde_json::Value>,
    /// Markdown 正文。
    pub body: String,
}

/// 两版内容的 diff 结论（四态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentChange {
    /// before 不存在、after 存在。
    Added,
    /// 内容等价。
    Unchanged,
    /// 内容变化。
    Modified,
    /// before 存在、after 不存在。
    Removed,
}

/// 比较两版内容（None 表示该版本不存在）。
pub fn diff_contents(
    before: Option<&ComponentContent>,
    after: Option<&ComponentContent>,
) -> ContentChange {
    match (before, after) {
        (None, None) => ContentChange::Unchanged,
        (None, Some(_)) => ContentChange::Added,
        (Some(_), None) => ContentChange::Removed,
        (Some(before), Some(after)) => {
            if before == after {
                ContentChange::Unchanged
            } else {
                ContentChange::Modified
            }
        }
    }
}
