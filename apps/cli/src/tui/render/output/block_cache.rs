//! block 级渲染缓存：key=(block_version,width)，命中复用，未命中重渲。

use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock};
use std::collections::HashMap;

/// block cache key。`text_width` 与 `RenderCtx.text_width` 同义：
/// 已扣除 gutter 的可用文本宽度（参见 #329 语义约定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub version: u64,
    pub text_width: u16,
    pub markdown_spacing: crate::tui::render::output::spacing::MarkdownSpacingPolicy,
}

struct CachedBlock {
    key: CacheKey,
    rendered: RenderedBlock,
}

#[derive(Default)]
pub struct BlockCache {
    map: HashMap<String, CachedBlock>,
}

impl BlockCache {
    /// 命中(key 一致)直接返回缓存 clone；否则调用 `render` 重渲染并缓存。
    pub fn get_or_render(
        &mut self,
        block_id: &str,
        key: CacheKey,
        render: impl FnOnce(&RenderCtx) -> RenderedBlock,
    ) -> RenderedBlock {
        if let Some(cached) = self.map.get(block_id) {
            if cached.key == key {
                #[cfg(test)]
                crate::tui::render::performance::record_block_cache_hit();
                return cached.rendered.clone();
            }
            #[cfg(test)]
            {
                if cached.key.version != key.version {
                    crate::tui::render::performance::record_block_cache_version_miss();
                }
                if cached.key.text_width != key.text_width {
                    crate::tui::render::performance::record_block_cache_width_miss();
                }
                if cached.key.markdown_spacing != key.markdown_spacing {
                    crate::tui::render::performance::record_block_cache_spacing_miss();
                }
            }
        } else {
            #[cfg(test)]
            crate::tui::render::performance::record_block_cache_absent_miss();
        }
        #[cfg(test)]
        crate::tui::render::performance::record_block_cache_miss();
        let ctx = RenderCtx {
            text_width: key.text_width,
            markdown_spacing: key.markdown_spacing,
        };
        let rendered = render(&ctx);
        self.map.insert(
            block_id.to_string(),
            CachedBlock {
                key,
                rendered: rendered.clone(),
            },
        );
        rendered
    }

    /// 清除不在 `live_set` 中的缓存条目（防内存泄漏）。
    /// 调用方应先将 live ids 收入 `HashSet<&str>`（O(n) 构建），
    /// 使此处每个条目的成员查询为 O(1)，整体 O(n) 而非 O(n²)。
    pub fn retain(&mut self, live_set: &std::collections::HashSet<&str>) {
        #[cfg(test)]
        let before = self.map.len();
        self.map.retain(|id, _| live_set.contains(id.as_str()));
        #[cfg(test)]
        crate::tui::render::performance::record_block_cache_retain_evictions(
            before.saturating_sub(self.map.len()),
        );
    }

    pub fn contains(&self, block_id: &str) -> bool {
        self.map.contains_key(block_id)
    }
}

#[cfg(test)]
mod tests {
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
            markdown_spacing: crate::tui::render::output::spacing::MarkdownSpacingPolicy::normal(),
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
    fn test_retain_evicts_absent_blocks() {
        let mut cache = BlockCache::default();
        cache.get_or_render("a", key(1), |_| block("a", 1));
        cache.get_or_render("b", key(1), |_| block("b", 1));
        let live_set: std::collections::HashSet<&str> = ["a"].into_iter().collect();
        cache.retain(&live_set);

        assert!(cache.contains("a"));
        assert!(
            !cache.contains("b"),
            "ViewModel 中不存在的 block 应被清除防泄漏"
        );
    }
}
