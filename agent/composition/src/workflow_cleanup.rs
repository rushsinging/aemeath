//! Workflow 落盘内容统一清理入口（#2089）。
//!
//! CLI 只允许依赖 composition / sdk / utils，因此清理能力在此暴露为
//! 窄函数：内部构造 workflow 的文件持久层并回收无引用实体。
//!
//! 清理的是**用户显式要求的运维动作**，不是 LLM 语义操作——不新增
//! `workflow_*` Tool。

use std::path::PathBuf;

pub use workflow::CleanupReport;
use workflow::FileWorkflowPersistence;

/// Workflow 落盘根目录：`~/.agents/workflows`。
pub fn workflow_root() -> PathBuf {
    share::config::paths::global_agents_dir().join("workflows")
}

/// 清理无引用 Workflow 实体。
///
/// `dry_run = true` 时只统计不删除。索引损坏时 fail-closed，不删除任何实体。
pub fn cleanup_orphan_workflows(dry_run: bool) -> Result<CleanupReport, String> {
    let persistence = FileWorkflowPersistence::new(workflow_root());
    if dry_run {
        persistence
            .cleanup_orphan_entities_dry_run()
            .map_err(|error| error.to_string())
    } else {
        persistence
            .cleanup_orphan_entities()
            .map_err(|error| error.to_string())
    }
}
