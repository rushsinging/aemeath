//! Memory 注入：每轮 LLM 调用前从 MemoryStore 取 top N 条构建 system block。

use std::path::Path;

use provider::api::SystemBlock;
use share::config::MemoryConfig;
use share::memory::entry::MemoryEntry;

/// 按配置将 Memory block 追加到已有 system blocks。
///
/// Main Agent 与 Sub Agent 共用此入口，确保两条调用链的门控与注入语义一致。
pub fn system_blocks_with_memory(
    system_blocks: &[SystemBlock],
    initial_cwd: &Path,
    memory_config: &MemoryConfig,
) -> Vec<SystemBlock> {
    append_memory_block(system_blocks, memory_config, || {
        build_memory_block(initial_cwd, memory_config.inject_count)
    })
}

fn append_memory_block(
    system_blocks: &[SystemBlock],
    memory_config: &MemoryConfig,
    build_block: impl FnOnce() -> Option<SystemBlock>,
) -> Vec<SystemBlock> {
    let mut effective_blocks = system_blocks.to_vec();
    if memory_config.enabled && memory_config.inject_count > 0 {
        if let Some(block) = build_block() {
            effective_blocks.push(block);
        }
    }
    effective_blocks
}

/// 从项目 memory store 读取 top N 条目，构建 `<memory-context>` system block。
///
/// - 使用 `top_for_inject_readonly`（不 touch 条目，避免排序漂移）
/// - 同时读取 global + project 两层 active 条目
/// - 返回 `None` 表示无可用 memory（store 打开失败或无条目）
pub fn build_memory_block(initial_cwd: &Path, inject_count: usize) -> Option<SystemBlock> {
    let store = open_memory_store(initial_cwd).ok()?;
    build_memory_block_from_store(&store, inject_count)
}

fn build_memory_block_from_store(
    store: &storage::api::MemoryStore,
    inject_count: usize,
) -> Option<SystemBlock> {
    let entries = store.top_for_inject_readonly(inject_count).ok()?;
    build_memory_block_from_entries(&entries)
}

fn build_memory_block_from_entries(entries: &[MemoryEntry]) -> Option<SystemBlock> {
    if entries.is_empty() {
        return None;
    }

    let mut lines = Vec::with_capacity(entries.len());
    for entry in entries {
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

fn open_memory_store(initial_cwd: &Path) -> Result<storage::api::MemoryStore, String> {
    use storage::api::{memory_base_dir, project_file_name, MemoryStore};
    let base_dir = memory_base_dir();
    MemoryStore::new(
        base_dir,
        project_file_name(&initial_cwd.to_string_lossy()),
        100,
        0.8,
    )
    .map_err(|e| format!("打开 MemoryStore 失败：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use share::memory::entry::{MemoryCategory, MemoryLayer, MemorySource};
    use storage::api::MemoryStore;

    fn memory_entry(content: &str, pinned: bool) -> MemoryEntry {
        let mut entry = MemoryEntry::new(
            content,
            100,
            MemoryLayer::Project,
            MemoryCategory::Decision,
            content,
            MemorySource::User,
        );
        entry.pinned = pinned;
        entry
    }

    fn temp_store() -> (MemoryStore, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "aemeath-memory-inject-test-{}",
            uuid::Uuid::new_v4()
        ));
        let store = MemoryStore::new(&dir, "project", 100, 0.8).expect("应创建临时 store");
        (store, dir)
    }

    #[test]
    fn test_build_memory_block_from_entries_formats_dynamic_context() {
        let entries = vec![
            memory_entry("优先根因修复", true),
            memory_entry("默认使用中文", false),
        ];

        let block = build_memory_block_from_entries(&entries).expect("应构造 memory block");

        assert_eq!(block.block_type, "text");
        assert!(block.cache_control.is_none());
        assert_eq!(
            block.text,
            "<memory-context>\n- ★ [Decision] 优先根因修复\n- [Decision] 默认使用中文\n</memory-context>"
        );
    }

    #[test]
    fn test_build_memory_block_from_store_respects_inject_count() {
        let (mut store, dir) = temp_store();
        store
            .add(memory_entry("第一条 memory", false))
            .expect("应写入第一条 memory");
        store
            .add(memory_entry("第二条 memory", false))
            .expect("应写入第二条 memory");

        let block = build_memory_block_from_store(&store, 1).expect("应构造 memory block");

        assert_eq!(block.text.matches("- [Decision]").count(), 1);
        assert_eq!(block.text.matches("第一条 memory").count(), 1);
        assert!(!block.text.contains("第二条 memory"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_build_memory_block_from_store_returns_none_when_empty() {
        let (store, dir) = temp_store();

        assert!(build_memory_block_from_store(&store, 5).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_build_memory_block_from_entries_returns_none_for_empty_entries() {
        assert!(build_memory_block_from_entries(&[]).is_none());
    }

    #[test]
    fn test_append_memory_block_appends_context_after_existing_blocks() {
        let existing = vec![SystemBlock::cached("static guidance".to_string())];
        let config = MemoryConfig::default();
        let memory = build_memory_block_from_entries(&[memory_entry("主动注入", false)]);

        let effective = append_memory_block(&existing, &config, || memory);

        assert_eq!(effective.len(), 2);
        assert_eq!(effective[0].text, "static guidance");
        assert!(effective[1].text.contains("主动注入"));
    }

    #[test]
    fn test_append_memory_block_preserves_existing_blocks_when_store_is_empty() {
        let existing = vec![SystemBlock::cached("static guidance".to_string())];
        let config = MemoryConfig::default();

        let effective = append_memory_block(&existing, &config, || None);

        assert_eq!(effective.len(), 1);
        assert_eq!(effective[0].text, "static guidance");
    }

    #[test]
    fn test_system_blocks_with_memory_preserves_existing_blocks_when_disabled() {
        let existing = vec![SystemBlock::cached("static guidance".to_string())];
        let config = MemoryConfig {
            enabled: false,
            ..MemoryConfig::default()
        };

        let effective = system_blocks_with_memory(&existing, Path::new("/missing"), &config);

        assert_eq!(effective.len(), 1);
        assert_eq!(effective[0].text, "static guidance");
    }

    #[test]
    fn test_system_blocks_with_memory_preserves_existing_blocks_when_count_is_zero() {
        let existing = vec![SystemBlock::cached("static guidance".to_string())];
        let config = MemoryConfig {
            inject_count: 0,
            ..MemoryConfig::default()
        };

        let effective = system_blocks_with_memory(&existing, Path::new("/missing"), &config);

        assert_eq!(effective.len(), 1);
        assert_eq!(effective[0].text, "static guidance");
    }
}
