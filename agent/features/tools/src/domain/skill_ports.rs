//! Skill 双端口（DDD §6.4.3，Issue #912）。
//!
//! 设计来源：`docs/design/02-modules/tools/02-ports-and-lifecycle.md` §6。
//!
//! # SkillCatalogPort
//!
//! 廉价只读投影端口：返回 [`SkillDescriptor`]，不读取 / 物化 Skill 正文。
//!
//! # SkillMaterializationPort
//!
//! 异步物化端口：为一次 Context Window 读取、解析并验证 Skill，输出带
//! 内容决定 revision 的 [`SkillMaterializationSnapshot`]（一组
//! [`PromptFragment`]）。Context Management 接收片段后决定注入位置、
//! 预算、去重、顺序与缓存分段。
//!
//! Skill 不是 Tool，不走 `ToolExecutionPort`；Context Management 不直接
//! 读取 Skill 文件或依赖其 adapter。

use async_trait::async_trait;

use super::skill_pl::{
    SkillDescriptor, SkillError, SkillMaterializationQuery, SkillMaterializationSnapshot,
    SkillQuery,
};

/// Skill Catalog 只读投影端口。
///
/// 消费方（Context Management / 交付层）通过此端口获取可见 Skill 的廉价
/// 描述，不接触文件 IO、adapter 或物化后的 content。
pub trait SkillCatalogPort: Send + Sync {
    /// 列出当前可见 Skill 的描述符。
    ///
    /// 保证：
    /// - 输出按 name 稳定排序；
    /// - 隐藏来源实现（文件遍历 / 内置注入）；
    /// - 不返回 content、文件句柄或 adapter。
    fn list(&self, query: SkillQuery) -> Vec<SkillDescriptor>;
}

/// Skill 异步物化端口。
///
/// 消费方通过此端口为一次 Context Window 取得已物化、已验证的
/// [`SkillMaterializationSnapshot`]。每次调用都重新读取文件系统——
/// 实现方 **MUST NOT** 维护进程级全局单槽缓存（Issue #912）。
#[async_trait]
pub trait SkillMaterializationPort: Send + Sync {
    /// 物化当前可用的 Skill。
    ///
    /// 返回带内容决定 revision 的快照。单个 Skill 的读取 / 解析失败以
    /// typed [`SkillError`] 记录并跳过该 Skill，不使整次物化失败。
    async fn materialize_available(
        &self,
        query: SkillMaterializationQuery,
    ) -> Result<SkillMaterializationSnapshot, SkillError>;
}
