use std::collections::HashSet;

use crate::tui::render::output::RenderedCache;

#[derive(Debug)]
pub struct OutputRenderCacheState {
    pub line_cache: RenderedCache,
}

impl Default for OutputRenderCacheState {
    fn default() -> Self {
        Self {
            line_cache: RenderedCache::new(),
        }
    }
}

#[derive(Debug, Default)]
pub struct ViewRenderCache {
    pub output: OutputRenderCacheState,
    pub dirty_blocks: HashSet<String>,
}

impl ViewRenderCache {
    pub fn mark_output_dirty(&mut self) {
        self.output.line_cache.invalidate();
    }

    pub fn mark_block_dirty(&mut self, key: impl Into<String>) {
        self.dirty_blocks.insert(key.into());
        self.mark_output_dirty();
    }

    pub fn clear_dirty_blocks(&mut self) {
        self.dirty_blocks.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_view_render_cache_marks_output_dirty() {
        let mut cache = ViewRenderCache::default();
        cache.output.line_cache.mark_clean_for_test(80);

        cache.mark_output_dirty();

        assert!(cache.output.line_cache.is_dirty());
    }

    #[test]
    fn test_view_render_cache_tracks_dirty_block() {
        let mut cache = ViewRenderCache::default();

        cache.mark_block_dirty("block-1");

        assert!(cache.dirty_blocks.contains("block-1"));
        assert!(cache.output.line_cache.is_dirty());
    }

    #[test]
    fn test_view_render_cache_clears_dirty_blocks() {
        let mut cache = ViewRenderCache::default();
        cache.mark_block_dirty("block-1");

        cache.clear_dirty_blocks();

        assert!(cache.dirty_blocks.is_empty());
    }
}
