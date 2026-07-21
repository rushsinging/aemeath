//! Skill Published Language（独立 PL，Issue #912）。
//!
//! 本模块定义 Skill BC 的值对象、查询、快照与 typed 错误，供
//! [`super::skill_ports`] 双端口消费。它是与 Tool Published Language
//! （`published_language.rs`）独立的另一套 Published Language：Skill
//! 不走 `ToolExecutionPort`，其产物 `PromptFragment` 交给 Context
//! Management 决定注入位置、预算与缓存分段。
//!
//! 设计来源：`docs/design/02-modules/tools/02-ports-and-lifecycle.md` §6。
//!
//! # 不变量
//!
//! - `PromptFragment::stable_key` 是去重 / 身份键，在同一物化快照内唯一；
//! - `SkillMaterializationRevision` 是确定性内容 revision：由物化结果内容
//!   决定（见 [`SkillMaterializationRevision::from_fragments`]），同一内容集 →
//!   同一 revision，与输入顺序无关；
//! - `SkillError` 是封闭的 typed 错误，调用方按变体分类处理，不泄漏
//!   协议私有信息。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt;
use std::path::PathBuf;

// 双端口 trait 与本 PL 同属 Skill Published Language 外观，按 `skill_pl`
// 路径统一消费（实现见 `super::skill_ports`）。
pub use super::skill_ports::{SkillCatalogPort, SkillMaterializationPort};

// ── 来源 ───────────────────────────────────────────────────────────────

/// Skill 文件的发现来源，用于溯源与缓存分段提示。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillSourceKind {
    /// 项目级 `{cwd}/.claude/skills`（最高优先级）。
    ProjectClaude,
    /// 项目级 `{cwd}/.agents/skills`。
    ProjectAgents,
    /// 全局 `~/.agents/skills`。
    Global,
    /// 配置提供的额外目录。
    Extra,
    /// 内置 Skill（如 `commit`）。
    Builtin,
}

/// Skill 的来源标识：kind + 文件路径或内置 URI。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSource {
    pub kind: SkillSourceKind,
    /// 文件路径（绝对或相对）或内置 URI（如 `aemeath-builtin://commit`）。
    pub path: String,
}

impl SkillSource {
    pub fn file(kind: SkillSourceKind, path: impl Into<String>) -> Self {
        Self {
            kind,
            path: path.into(),
        }
    }

    pub fn builtin(uri: impl Into<String>) -> Self {
        Self {
            kind: SkillSourceKind::Builtin,
            path: uri.into(),
        }
    }
}

// ── CacheHint ──────────────────────────────────────────────────────────

/// 给 Context Management 的缓存分段提示。
///
/// 当前所有文件型 Skill 内容在 revision 内稳定，故默认 `Stable`；预留
/// `Volatile` 供未来动态来源使用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CacheHint {
    /// 内容在给定 revision 内稳定，可参与缓存分段。
    Stable,
    /// 每个 Context Window 重新计算。
    Volatile,
}

// ── PromptFragment ─────────────────────────────────────────────────────

/// 已物化、不可变的 Prompt 片段，由 Skill 产出。
///
/// Context Management 接收 `PromptFragment` 后决定注入位置、预算、去重、
/// 顺序与缓存分段。本类型只携带值，不含文件句柄、adapter 或 Tokio 类型。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptFragment {
    stable_key: String,
    content: String,
    source: SkillSource,
    cache_hint: CacheHint,
}

impl PromptFragment {
    pub fn new(
        stable_key: impl Into<String>,
        content: impl Into<String>,
        source: SkillSource,
        cache_hint: CacheHint,
    ) -> Self {
        Self {
            stable_key: stable_key.into(),
            content: content.into(),
            source,
            cache_hint,
        }
    }

    pub fn stable_key(&self) -> &str {
        &self.stable_key
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    /// 测试与 revision 计算用的可变访问（crate 内部）。
    #[cfg(test)]
    pub(crate) fn mut_content(&mut self) -> &mut String {
        &mut self.content
    }

    pub fn source(&self) -> &SkillSource {
        &self.source
    }

    pub fn cache_hint(&self) -> CacheHint {
        self.cache_hint
    }
}

// ── SkillDescriptor（Catalog 投影，廉价） ───────────────────────────────

/// Skill Catalog 的 Published Language。
///
/// 只暴露 catalog 所需的廉价元数据，不携带物化后的 content，也不含
/// adapter、文件句柄或运行时资源。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillDescriptor {
    name: String,
    description: String,
    source: SkillSource,
    aliases: Vec<String>,
    /// 用户可输入的 Slash Command 名；`None` 表示该 Skill 仅供 agent 物化，
    /// 不得自动注册进 Command Catalog。
    slash_command: Option<String>,
    /// 与 `slash_command` 同一投影的合法别名；不复用 Skill identity aliases。
    slash_aliases: Vec<String>,
}

impl SkillDescriptor {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        source: SkillSource,
        aliases: Vec<String>,
        slash_command: Option<String>,
        slash_aliases: Vec<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            source,
            aliases,
            slash_command,
            slash_aliases,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn source(&self) -> &SkillSource {
        &self.source
    }

    pub fn aliases(&self) -> &[String] {
        &self.aliases
    }

    pub fn slash_command(&self) -> Option<&str> {
        self.slash_command.as_deref()
    }

    pub fn slash_aliases(&self) -> &[String] {
        &self.slash_aliases
    }
}

// ── 查询 ───────────────────────────────────────────────────────────────

/// Catalog 列表查询：携带 project root、额外目录与可用工具集合。
///
/// adapter（如 [`FilesystemSkillAdapter`](crate::adapters::skill_filesystem::FilesystemSkillAdapter)）
/// **不**捕获 project roots，每次 `list` 都从本快照推导
/// `{project_root}/.claude/skills`、`{project_root}/.agents/skills` 与
/// `extra_dirs`，并依据 `available_tools` 过滤 frontmatter 的
/// `requires_tools` / `fallback_for`。
///
/// `Default` 产生空 project root / 空集合：adapter 把不存在的目录视为空
/// （非错误），可用于只关心内置 Skill 的场景。
#[derive(Debug, Clone, Default)]
pub struct SkillQuery {
    /// 项目根目录：adapter 推导项目级 `.claude/skills` 与 `.agents/skills`。
    pub project_root: PathBuf,
    /// 额外发现目录（最低优先级，按提供顺序扫描）；adapter 会展开 `~` 前缀。
    pub extra_dirs: Vec<PathBuf>,
    /// 当前 Context Window 可用工具名集合，用于过滤 `requires_tools`。
    pub available_tools: BTreeSet<String>,
}

impl SkillQuery {
    pub fn new(
        project_root: PathBuf,
        extra_dirs: Vec<PathBuf>,
        available_tools: BTreeSet<String>,
    ) -> Self {
        Self {
            project_root,
            extra_dirs,
            available_tools,
        }
    }
}

/// 物化查询：携带 project root、额外目录与可用工具集合。
///
/// 语义同 [`SkillQuery`]，但用于异步物化端口。adapter 每次调用从本快照
/// 推导发现根目录并过滤。
#[derive(Debug, Clone, Default)]
pub struct SkillMaterializationQuery {
    /// 项目根目录：adapter 推导项目级 `.claude/skills` 与 `.agents/skills`。
    pub project_root: PathBuf,
    /// 额外发现目录（最低优先级，按提供顺序扫描）；adapter 会展开 `~` 前缀。
    pub extra_dirs: Vec<PathBuf>,
    /// 当前 Context Window 可用工具名集合，用于过滤 `requires_tools`。
    pub available_tools: BTreeSet<String>,
}

impl SkillMaterializationQuery {
    pub fn new(
        project_root: PathBuf,
        extra_dirs: Vec<PathBuf>,
        available_tools: BTreeSet<String>,
    ) -> Self {
        Self {
            project_root,
            extra_dirs,
            available_tools,
        }
    }
}

// ── Revision（由内容决定） ──────────────────────────────────────────────

/// 物化结果的 revision，由结果内容（stable_key + content 集合）决定。
///
/// 设计目标：同一内容集产生同一 revision，与发现 / 迭代顺序无关；内容
/// 变化产生不同 revision。这使得上层缓存可凭 revision 判断是否需要
/// 重新注入。revision 取内容 SHA-256 的前 16 字节十六进制（32 字符）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SkillMaterializationRevision(String);

impl SkillMaterializationRevision {
    /// 由一组物化片段计算 revision。
    ///
    /// 先按 `stable_key` 排序再哈希，因此对同一内容集合是顺序无关的。
    pub fn from_fragments(fragments: &[PromptFragment]) -> Self {
        let mut keyed: Vec<(&str, &str)> = fragments
            .iter()
            .map(|f| (f.stable_key.as_str(), f.content.as_str()))
            .collect();
        keyed.sort_by(|a, b| a.0.cmp(b.0));

        let mut hasher = Sha256::new();
        for (key, content) in keyed {
            hasher.update(key.as_bytes());
            hasher.update(b"\x1f"); // 单元分隔符，避免键/值粘连碰撞
            hasher.update(content.as_bytes());
            hasher.update(b"\x1e"); // 记录分隔符
        }
        let digest = hasher.finalize();
        let hex: String = digest.iter().take(16).map(|b| format!("{b:02x}")).collect();
        Self(hex)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SkillMaterializationRevision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ── Snapshot ───────────────────────────────────────────────────────────

/// 一次 Context Window 物化的只读快照：片段集合 + 内容决定的 revision。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMaterializationSnapshot {
    fragments: Vec<PromptFragment>,
    revision: SkillMaterializationRevision,
}

impl SkillMaterializationSnapshot {
    pub fn new(fragments: Vec<PromptFragment>, revision: SkillMaterializationRevision) -> Self {
        Self {
            fragments,
            revision,
        }
    }

    /// 由片段构造快照，并按当前内容计算 revision。
    pub fn from_fragments(fragments: Vec<PromptFragment>) -> Self {
        let revision = SkillMaterializationRevision::from_fragments(&fragments);
        Self {
            fragments,
            revision,
        }
    }

    pub fn fragments(&self) -> &[PromptFragment] {
        &self.fragments
    }

    pub fn revision(&self) -> &SkillMaterializationRevision {
        &self.revision
    }

    pub fn len(&self) -> usize {
        self.fragments.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fragments.is_empty()
    }
}

// ── SkillError（typed） ────────────────────────────────────────────────

/// Skill 物化过程中的 typed 错误。
///
/// 调用方按变体分类：读取失败（IO）/ 解析失败（frontmatter）/ 物化失败
/// （聚合层不应发生的内部错误）。错误消息可安全暴露，不泄漏密钥或
/// 协议私有信息。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
pub enum SkillError {
    #[error("读取 Skill 文件失败: {path}: {reason}")]
    ReadFailed { path: String, reason: String },

    #[error("解析 Skill frontmatter 失败: {path}: {reason}")]
    ParseFailed { path: String, reason: String },

    #[error("Skill 物化失败: {reason}")]
    MaterializationFailed { reason: String },
}

impl SkillError {
    pub fn read_failed(path: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::ReadFailed {
            path: path.into(),
            reason: reason.into(),
        }
    }

    pub fn parse_failed(path: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::ParseFailed {
            path: path.into(),
            reason: reason.into(),
        }
    }

    pub fn materialization_failed(reason: impl Into<String>) -> Self {
        Self::MaterializationFailed {
            reason: reason.into(),
        }
    }
}
