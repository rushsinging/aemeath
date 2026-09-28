//! 跨 BC 共享的工具执行词汇（shared kernel tool vocabulary）。
//!
//! `ToolName` / `ToolCapability` / `ToolCapabilities` / `AuthorizationContext`
//! 是 tools、policy、runtime 多个 BC 共用的领域词汇（同 `WorkspaceId` 判例），
//! 唯一定义在本模块；tools crate 根保留 re-export 兼容旧路径
//! `tools::ToolName` 等。决策 BC 的 domain 层依赖本模块而非工具 BC，
//! 以满足 Clean 依赖规则（内圈不依赖外圈）。
//!
//! 设计来源：`docs/design/02-modules/tools/01-domain-model.md`。
//!
//! # 不变量
//!
//! - `ToolName` 在同一 Registry Scope 内唯一，规范化为 ASCII 小写；
//! - `ToolCapabilities` 只能通过 baseline 或 `derive_restricted` 收缩，不可扩权。

use serde::{Deserialize, Serialize};
use std::fmt;

// ── ToolName ────────────────────────────────────────────────────────

/// 工具名称：保留 canonical 协议拼写，同时以 ASCII 小写键比较与哈希。
///
/// canonical 名称用于模型 schema、Runtime/SDK 事件与 TUI；normalized key
/// 用于 Registry Scope、Profile 与 Execution 查询。MCP 限定名的跨段语义不变。
#[derive(Debug, Clone)]
pub struct ToolName {
    canonical: String,
    normalized: String,
}

impl ToolName {
    pub fn new(name: impl Into<String>) -> Self {
        let canonical = name.into();
        let normalized = canonical.to_ascii_lowercase();
        Self {
            canonical,
            normalized,
        }
    }

    pub fn normalized(&self) -> &str {
        &self.normalized
    }

    pub fn as_str(&self) -> &str {
        &self.canonical
    }
}

impl PartialEq for ToolName {
    fn eq(&self, other: &Self) -> bool {
        self.normalized == other.normalized
    }
}

impl Eq for ToolName {}

impl PartialOrd for ToolName {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ToolName {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // 与 PartialEq/Hash 同源：按 normalized 键排序。
        self.normalized.cmp(&other.normalized)
    }
}

impl std::hash::Hash for ToolName {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.normalized.hash(state);
    }
}

impl fmt::Display for ToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.canonical)
    }
}

impl serde::Serialize for ToolName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.canonical)
    }
}

impl<'de> serde::Deserialize<'de> for ToolName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Self::new)
    }
}

impl From<&str> for ToolName {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for ToolName {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

// ── ToolCapability ──────────────────────────────────────────────────

/// 工具执行所需能力。Profile 声明允许能力。
///
/// Capability 表达安全权限，不表达 Tool 身份或装配位置。
/// 新增 Tool 未声明 required capabilities 时不得注册。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolCapability {
    /// 读取工作区（文件 / 目录）。
    Read,
    /// 写入工作区（创建 / 修改 / 删除文件）。
    Write,
    /// 执行外部进程（bash 等）。
    Execute,
    /// 网络访问（web fetch / search 等）。
    NetworkAccess,
    /// 用户交互（AskUserQuestion 等）。
    Interact,
    /// 派发子 agent。
    Dispatch,
    /// 读取 TaskData 列表或 TaskData 详情。
    TaskRead,
    /// 修改 TaskData 列表。
    TaskWrite,
    /// 控制 workspace（worktree 进入 / 退出）。
    WorkspaceControl,
    /// 控制 plan mode。
    Plan,
    /// 全量类：main 专属杂项工具。
    All,
}

impl ToolCapability {
    /// Parse a capability by its canonical variant name; `None` for unknown
    /// spellings. Consumed by role-policy compilation for config validation.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "Read" => Some(Self::Read),
            "Write" => Some(Self::Write),
            "Execute" => Some(Self::Execute),
            "NetworkAccess" => Some(Self::NetworkAccess),
            "Interact" => Some(Self::Interact),
            "Dispatch" => Some(Self::Dispatch),
            "TaskRead" => Some(Self::TaskRead),
            "TaskWrite" => Some(Self::TaskWrite),
            "WorkspaceControl" => Some(Self::WorkspaceControl),
            "Plan" => Some(Self::Plan),
            "All" => Some(Self::All),
            _ => None,
        }
    }
}

impl fmt::Display for ToolCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 与 parse 对称：canonical 变体名。
        f.write_str(match self {
            Self::Read => "Read",
            Self::Write => "Write",
            Self::Execute => "Execute",
            Self::NetworkAccess => "NetworkAccess",
            Self::Interact => "Interact",
            Self::Dispatch => "Dispatch",
            Self::TaskRead => "TaskRead",
            Self::TaskWrite => "TaskWrite",
            Self::WorkspaceControl => "WorkspaceControl",
            Self::Plan => "Plan",
            Self::All => "All",
        })
    }
}

// 能力集合（bitflags）。
//
// 用于 `ToolDescriptor::required_capabilities` 和 `ToolProfile::allowed_capabilities`。
// 有效工具集 = Registry Scope ∩ Profile Allowed Capabilities。
bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct ToolCapabilities: u32 {
        const Read             = 1 << 0;
        const Write            = 1 << 1;
        const Execute          = 1 << 2;
        const NetworkAccess    = 1 << 3;
        const Interact         = 1 << 4;
        const Dispatch         = 1 << 5;
        const TaskWrite        = 1 << 6;
        const WorkspaceControl = 1 << 7;
        const Plan             = 1 << 8;
        const TaskRead         = 1 << 9;
        /// 全量类：main 专属。Brief / ToolSearch / Memory 等杂项工具归此位，
        /// 受限 profile 默认组装不出；config 显式声明 "All" 才放行。
        const All              = 1 << 10;
    }
}

impl ToolCapabilities {
    /// 从单个 capability 构造。
    pub fn single(cap: ToolCapability) -> Self {
        Self::from(cap)
    }

    /// 从多个 capability 构造。
    pub fn from_caps(caps: impl IntoIterator<Item = ToolCapability>) -> Self {
        caps.into_iter()
            .fold(Self::empty(), |acc, c| acc | Self::from(c))
    }

    /// 是否包含指定 capability。
    pub fn contains_cap(self, cap: ToolCapability) -> bool {
        self.contains(Self::from(cap))
    }

    /// `self` 是否是 `other` 的子集。
    pub fn is_subset_of(self, other: Self) -> bool {
        self.intersection(other) == self
    }
}

impl From<ToolCapability> for ToolCapabilities {
    fn from(cap: ToolCapability) -> Self {
        match cap {
            ToolCapability::Read => Self::Read,
            ToolCapability::Write => Self::Write,
            ToolCapability::Execute => Self::Execute,
            ToolCapability::NetworkAccess => Self::NetworkAccess,
            ToolCapability::Interact => Self::Interact,
            ToolCapability::Dispatch => Self::Dispatch,
            ToolCapability::TaskRead => Self::TaskRead,
            ToolCapability::TaskWrite => Self::TaskWrite,
            ToolCapability::WorkspaceControl => Self::WorkspaceControl,
            ToolCapability::Plan => Self::Plan,
            ToolCapability::All => Self::All,
        }
    }
}

// ── AuthorizationContext ────────────────────────────────────────────

/// 单次 Tool 调用的授权上下文（纯值对象）。
///
/// 唯一定义在本共享词汇模块；Policy 构造、Runtime 逐调用传递、
/// Tool/Project/Hook 只读消费。`STANDARD` 是默认严格约束，
/// `ALLOW_ALL` 对应 AllowAll permission mode（放行所有授权性限制）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthorizationContext {
    pub allow_outside_workspace: bool,
    pub require_read_before_write: bool,
    pub enforce_bash_safety: bool,
    pub enforce_tool_fuse: bool,
}

impl AuthorizationContext {
    pub const STANDARD: Self = Self {
        allow_outside_workspace: false,
        require_read_before_write: true,
        enforce_bash_safety: true,
        enforce_tool_fuse: true,
    };

    pub const ALLOW_ALL: Self = Self {
        allow_outside_workspace: true,
        require_read_before_write: false,
        enforce_bash_safety: false,
        enforce_tool_fuse: false,
    };
}
