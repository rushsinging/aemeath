//! Memory 注入：每轮 LLM 调用前从 MemoryStore 取 top N 条构建 system block。

use std::path::Path;

use provider::SystemBlock;

/// 从项目 memory store 读取 top N 条目，构建 `<memory-context>` system block。
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
