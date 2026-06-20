//! block 级渲染缓存：key=(block_version,width)，命中复用，未命中重渲。

use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock};
use std::collections::HashMap;
use std::rc::Rc;

/// block cache key。`text_width` 与 `RenderCtx.text_width` 同义：
/// 已扣除 gutter 的可用文本宽度（参见 #329 语义约定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub version: u64,
    pub text_width: u16,
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
                return cached.rendered.clone();
            }
        }
        let ctx = RenderCtx {
            text_width: key.text_width,
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
        self.map.retain(|id, _| live_set.contains(id.as_str()));
    }

    pub fn contains(&self, block_id: &str) -> bool {
        self.map.contains_key(block_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::render::output::rendered::RenderedLine;

    fn block(id: &str, n: usize) -> RenderedBlock {
        RenderedBlock {
            block_id: id.into(),
            lines: Rc::new(vec![RenderedLine::default(); n]),
        }
    }

    #[test]
    fn test_cache_hit_when_key_unchanged() {
        let mut cache = BlockCache::default();
        let mut calls = 0;
        let key = CacheKey {
            version: 1,
            text_width: 80,
        };
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
        cache.get_or_render(
            "a",
            CacheKey {
                version: 1,
                text_width: 80,
            },
            |_| {
                calls += 1;
                block("a", 1)
            },
        );
        cache.get_or_render(
            "a",
            CacheKey {
                version: 2,
                text_width: 80,
            },
            |_| {
                calls += 1;
                block("a", 1)
            },
        );

        assert_eq!(calls, 2, "version 变应重渲染");
    }

    #[test]
    fn test_retain_evicts_absent_blocks() {
        let mut cache = BlockCache::default();
        cache.get_or_render(
            "a",
            CacheKey {
                version: 1,
                text_width: 80,
            },
            |_| block("a", 1),
        );
        cache.get_or_render(
            "b",
            CacheKey {
                version: 1,
                text_width: 80,
            },
            |_| block("b", 1),
        );
        let live_set: std::collections::HashSet<&str> = ["a"].into_iter().collect();
        cache.retain(&live_set);

        assert!(cache.contains("a"));
        assert!(
            !cache.contains("b"),
            "ViewModel 中不存在的 block 应被清除防泄漏"
        );
    }
}
