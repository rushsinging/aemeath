//! Memory 注入：每轮 LLM 调用前构建 `<memory-context>` system block。
//!
//! 两条路径：
//! - **Main**（`build_memory_block_from_port`）：从注入的 `memory::MemoryPort`
//!   读取，调用 `retrieve_for_inject`。不接触文件系统——旧 `storage::MemoryStore`
//!   文件不参与 Main 注入。
//! - **Sub Run**（`build_memory_block`）：仍走旧 storage 路径。Sub 尚未接入
//!   port，不在本次重构范围内，故保留原实现。

use std::path::Path;

use provider::SystemBlock;

/// 从注入的 [`memory::MemoryPort`] 读取 top N 条目，构建 `<memory-context>`
/// system block（Main 路径）。
///
/// - 调用 `retrieve_for_inject`（不 touch 条目，避免排序漂移）
/// - 同时读取 global + project 两层 active 条目
/// - 返回 `None` 表示无可用 memory（port 无条目）
///
/// 此函数签名不含任何路径：Main 注入完全脱离旧 `storage::MemoryStore` 文件。
pub fn build_memory_block_from_port(
    memory: &dyn memory::MemoryPort,
    now: u64,
    limit: usize,
) -> Option<SystemBlock> {
    let query = memory::MemoryQuery {
        limit,
        layer: None,
        category: None,
        now,
    };
    let result = memory.retrieve_for_inject(&query);
    if result.hits.is_empty() {
        return None;
    }

    let mut lines = Vec::with_capacity(result.hits.len());
    for hit in &result.hits {
        let pinned = if hit.entry.pinned { "★ " } else { "" };
        lines.push(format!(
            "- {pinned}[{:?}] {}",
            hit.entry.category, hit.entry.content
        ));
    }

    Some(SystemBlock::dynamic(format!(
        "<memory-context>\n{}\n</memory-context>",
        lines.join("\n")
    )))
}

/// 从项目 memory store 读取 top N 条目，构建 `<memory-context>` system block
/// （Sub Run 路径，保留旧 storage 实现）。
///
/// - 使用 `top_for_inject_readonly`（不 touch 条目，避免排序漂移）
/// - 同时读取 global + project 两层 active 条目
/// - 返回 `None` 表示无可用 memory（store 打开失败或无条目）
pub fn build_memory_block(initial_cwd: &Path, inject_count: usize) -> Option<SystemBlock> {
    let store = open_memory_store(initial_cwd).ok()?;
    let entries = store.top_for_inject_readonly(inject_count).ok()?;
    if entries.is_empty() {
        return None;
    }

    let mut lines = Vec::with_capacity(entries.len());
    for entry in &entries {
        let pinned = if entry.pinned { "★ " } else { "" };
        lines.push(format!(
            "- {pinned}[{:?}] {}",
            entry.category, entry.content
        ));
    }

    Some(SystemBlock::dynamic(format!(
        "<memory-context>\n{}\n</memory-context>",
        lines.join("\n")
    )))
}

fn open_memory_store(initial_cwd: &Path) -> Result<storage::MemoryStore, String> {
    use storage::{memory_base_dir, project_file_name, MemoryStore};
    let base_dir = memory_base_dir();
    MemoryStore::new(
        base_dir,
        project_file_name(&initial_cwd.to_string_lossy()),
        100,
        0.8,
    )
    .map_err(|e| format!("打开 MemoryStore 失败：{e}"))
}
