//! 输出文档渲染器：遍历 ViewModel.blocks，经 block 级缓存产出 RenderedDocument。

use crate::tui::render::output::block_cache::{BlockCache, CacheKey};
use crate::tui::render::output::blocks::render_block;
use crate::tui::render::output::rendered::{RenderedBlock, RenderedDocument};
use crate::tui::render::output_area::types::MAX_LINES;
use crate::tui::view_model::output::OutputViewModel;

#[derive(Default)]
pub struct OutputDocumentRenderer {
    cache: BlockCache,
    #[cfg(test)]
    render_count: std::cell::Cell<usize>,
}

impl OutputDocumentRenderer {
    pub fn render(&mut self, view_model: &OutputViewModel, width: u16) -> RenderedDocument {
        let mut blocks = Vec::with_capacity(view_model.blocks.len());
        let live_ids = view_model
            .blocks
            .iter()
            .map(|block| block.block_id.clone())
            .collect::<Vec<_>>();

        for block in &view_model.blocks {
            let key = CacheKey {
                version: block.block_version,
                width,
            };
            let rendered = self.cache.get_or_render(&block.block_id, key, |ctx| {
                #[cfg(test)]
                self.render_count.set(self.render_count.get() + 1);
                render_block(&block.kind, &block.block_id, ctx)
            });
            blocks.push(rendered);
        }
        self.cache.retain(&live_ids);
        RenderedDocument {
            blocks: trim_blocks_to_max_lines(blocks, MAX_LINES),
        }
    }
    #[cfg(test)]
    pub fn render_count(&self) -> usize {
        self.render_count.get()
    }
}

fn trim_blocks_to_max_lines(blocks: Vec<RenderedBlock>, max_lines: usize) -> Vec<RenderedBlock> {
    if max_lines == 0 {
        return Vec::new();
    }

    let mut kept = Vec::new();
    let mut used = 0usize;
    for block in blocks.into_iter().rev() {
        let line_count = block.lines.len();
        if used > 0 && used.saturating_add(line_count) > max_lines {
            break;
        }
        used = used.saturating_add(line_count);
        kept.push(block);
    }
    kept.reverse();
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::view_model::output::{
        OutputBlockKind, OutputBlockView, OutputViewModel, TextBlockView,
    };
    use crate::tui::view_model::style::SemanticStyle;

    fn vm_with(kind: OutputBlockKind, id: &str) -> OutputViewModel {
        OutputViewModel {
            blocks: vec![OutputBlockView {
                block_id: id.into(),
                block_version: 1,
                kind,
            }],
            version: 1,
            follow_tail_hint: true,
        }
    }

    #[test]
    fn test_renderer_emits_one_block_per_view() {
        let mut renderer = OutputDocumentRenderer::default();
        let vm = vm_with(
            OutputBlockKind::SystemNotice(TextBlockView {
                key: "s".into(),
                text: "ok".into(),
                style: SemanticStyle::Muted,
            }),
            "s",
        );
        let doc = renderer.render(&vm, 80);

        assert_eq!(doc.blocks.len(), 1);
        assert_eq!(doc.blocks[0].block_id, "s");
    }

    #[test]
    fn test_renderer_caches_unchanged_block() {
        let mut renderer = OutputDocumentRenderer::default();
        let vm = vm_with(
            OutputBlockKind::SystemNotice(TextBlockView {
                key: "s".into(),
                text: "ok".into(),
                style: SemanticStyle::Muted,
            }),
            "s",
        );
        let _ = renderer.render(&vm, 80);
        let _ = renderer.render(&vm, 80);

        assert_eq!(
            renderer.render_count(),
            1,
            "同 version+width 第二次应命中缓存"
        );
    }

    #[test]
    fn test_document_drops_oldest_block_when_over_max_lines() {
        use crate::tui::render::output::rendered::{RenderedBlock, RenderedLine};
        use ratatui::text::Span;

        let blocks = vec![
            RenderedBlock {
                block_id: "old".into(),
                lines: vec![RenderedLine::new(vec![Span::raw("old")]); 2],
            },
            RenderedBlock {
                block_id: "new".into(),
                lines: vec![RenderedLine::new(vec![Span::raw("new")]); 2],
            },
        ];
        let trimmed = trim_blocks_to_max_lines(blocks, 3);

        assert_eq!(trimmed.len(), 1);
        assert_eq!(trimmed[0].block_id, "new");
        assert_eq!(trimmed[0].lines.len(), 2);
    }
}
