//! Memory 注入：每轮 LLM 调用前构建 `<memory-context>` system block。
//!
//! Main 与 Sub Run 都从装配的 `memory::MemoryPort` 读取，调用
//! `retrieve_for_inject`；旧 `storage::MemoryStore` 不再参与运行期注入。

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
