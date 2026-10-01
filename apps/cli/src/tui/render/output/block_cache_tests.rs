use super::*;
use crate::tui::render::output::rendered::RenderedLine;
use std::rc::Rc;

fn block(id: &str, n: usize) -> RenderedBlock {
    RenderedBlock {
        block_id: id.into(),
        lines: Rc::new(vec![RenderedLine::default(); n]),
    }
}

fn key(version: u64) -> CacheKey {
    CacheKey {
        version,
        text_width: 80,
        markdown_spacing: crate::tui::render::output::spacing::MarkdownSpacingPolicy::default(),
    }
}

#[test]
fn test_cache_hit_when_key_unchanged() {
    let mut cache = BlockCache::default();
    let mut calls = 0;
    let key = key(1);
    cache.get_or_render("a", key, |_| {
        calls += 1;
        block("a", 2)
    });
    cache.get_or_render("a", key, |_| {
        calls += 1;
        block("a", 2)
    });

    assert_eq!(calls, 1, "同 key 第二次应命中缓存，不再渲染");
}

#[test]
fn test_cache_miss_when_version_changes() {
    let mut cache = BlockCache::default();
    let mut calls = 0;
    cache.get_or_render("a", key(1), |_| {
        calls += 1;
        block("a", 1)
    });
    cache.get_or_render("a", key(2), |_| {
        calls += 1;
        block("a", 1)
    });

    assert_eq!(calls, 2, "version 变应重渲染");
}

#[test]
fn cache_misses_when_only_markdown_spacing_changes() {
    let mut cache = BlockCache::default();
    let mut calls = 0;
    cache.get_or_render("a", key(1), |_| {
        calls += 1;
        block("a", 1)
    });
    let mut compact = key(1);
    compact.markdown_spacing =
        crate::tui::render::output::spacing::MarkdownSpacingPolicy::compact();
    cache.get_or_render("a", compact, |_| {
        calls += 1;
        block("a", 1)
    });

    assert_eq!(calls, 2);
}

#[test]
fn cache_evicts_least_recently_used_entry_at_capacity() {
    let mut cache = BlockCache::with_capacity(2);
    cache.get_or_render("a", key(1), |_| block("a", 1));
    cache.get_or_render("b", key(1), |_| block("b", 1));
    cache.get_or_render("a", key(1), |_| unreachable!("a should hit cache"));
    cache.get_or_render("c", key(1), |_| block("c", 1));

    assert!(cache.contains("a"), "命中必须刷新 a 的最近使用顺序");
    assert!(!cache.contains("b"), "最久未使用的 b 应先被淘汰");
    assert!(cache.contains("c"));
    assert_eq!(cache.len(), 2);
}

#[test]
fn semantic_retain_removes_absent_entries_without_dropping_live_lru_entries() {
    let mut cache = BlockCache::with_capacity(3);
    cache.get_or_render("a", key(1), |_| block("a", 1));
    cache.get_or_render("b", key(1), |_| block("b", 1));
    cache.get_or_render("c", key(1), |_| block("c", 1));
    let live_set: std::collections::HashSet<&str> = ["a", "c"].into_iter().collect();

    cache.retain(&live_set);

    assert!(cache.contains("a"));
    assert!(!cache.contains("b"));
    assert!(cache.contains("c"));
    assert_eq!(cache.len(), 2);
}
