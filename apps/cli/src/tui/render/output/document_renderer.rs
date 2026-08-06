//! 输出文档渲染器：遍历 ViewModel.blocks，经 block 级缓存产出 RenderedDocument。

use crate::tui::render::display::safe_text::str_display_width;
use crate::tui::render::output::block_cache::{
    BlockCache, CacheKey, DEFAULT_RENDER_CACHE_CAPACITY,
};
use crate::tui::render::output::bounded_lru::BoundedLruMap;
use crate::tui::render::output::gutter;
use crate::tui::render::output::rendered::{RenderedBlock, RenderedDocument, RenderedLine};
use crate::tui::render::output::tool_display::{result_policy, ResultPolicy, ResultRender};
use crate::tui::render::theme;
use crate::tui::view_model::output::{
    AskUserPhaseView, BlockNode, OutputBlockKind, OutputRenderWindow, OutputViewModel,
};
use ratatui::style::Style;
use ratatui::text::Span;
use std::rc::Rc;

/// 单次窗口渲染结果。`source_total_lines` 来自全部 root 布局索引，
/// `document` 只持请求窗口内的 rendered blocks。
pub struct OutputRenderResult {
    pub document: RenderedDocument,
    pub source_total_lines: usize,
    pub folded_earlier_lines: usize,
}

/// gutted 缓存的 key：唯一决定 gutted block 内容（含 gutter）的所有参数。
/// 动画帧只在 viewport 绘制阶段消费，不参与历史文档缓存。
#[derive(PartialEq, Eq, Clone)]
struct GuttedKey {
    block_version: u64,
    text_width: u16,
    depth: usize,
    markdown_spacing: crate::tui::render::output::spacing::MarkdownSpacingPolicy,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RendererRetainedCacheCapacity {
    pub block_entries: usize,
    pub gutted_entries: usize,
    pub peak_block_entries: usize,
    pub peak_gutted_entries: usize,
}

pub struct OutputDocumentRenderer {
    cache: BlockCache,
    /// 带 gutter 的 block 缓存：key = block_id，value = (GuttedKey, gutted RenderedBlock)。
    /// 命中时直接 clone（lines 为 Rc，廉价）；未命中则走完整 render_self + apply_gutter 路径。
    gutted: BoundedLruMap<String, (GuttedKey, RenderedBlock)>,
    #[cfg(test)]
    retained_cache_peak: RendererRetainedCacheCapacity,
    #[cfg(test)]
    render_count: std::cell::Cell<usize>,
    /// 统计 gutted 缓存未命中（重新渲染）次数，用于测试断言。
    #[cfg(test)]
    gutted_render_count: std::cell::Cell<usize>,
}

impl Default for OutputDocumentRenderer {
    fn default() -> Self {
        Self::with_render_cache_capacity(DEFAULT_RENDER_CACHE_CAPACITY)
    }
}

impl OutputDocumentRenderer {
    fn with_render_cache_capacity(capacity: usize) -> Self {
        Self {
            cache: BlockCache::with_capacity(capacity),
            gutted: BoundedLruMap::with_capacity(capacity),
            #[cfg(test)]
            retained_cache_peak: RendererRetainedCacheCapacity::default(),
            #[cfg(test)]
            render_count: std::cell::Cell::new(0),
            #[cfg(test)]
            gutted_render_count: std::cell::Cell::new(0),
        }
    }

    #[cfg(test)]
    pub fn render_model_document(
        &mut self,
        view_model: &OutputViewModel,
        outer_width: u16,
        fallback_width: usize,
        animation_frame: u64,
        markdown_spacing: crate::tui::render::output::spacing::MarkdownSpacingPolicy,
    ) -> RenderedDocument {
        self.render_model_window(
            view_model,
            outer_width,
            fallback_width,
            animation_frame,
            markdown_spacing,
            OutputRenderWindow::all(),
        )
        .document
    }

    pub fn render_model_window(
        &mut self,
        view_model: &OutputViewModel,
        outer_width: u16,
        fallback_width: usize,
        animation_frame: u64,
        markdown_spacing: crate::tui::render::output::spacing::MarkdownSpacingPolicy,
        window: OutputRenderWindow,
    ) -> OutputRenderResult {
        let render_width = if outer_width > 1 {
            outer_width
        } else {
            u16::try_from(fallback_width.max(1)).unwrap_or(u16::MAX)
        };
        self.render_tree_with_window(
            view_model,
            render_width,
            animation_frame,
            markdown_spacing,
            window,
        )
    }

    /// 递归走 `view_model.roots`（DFS：父块先于子块），经 block 级缓存展平为线性文档。
    /// gutter（depth 缩进 + marker）在组合期注入。
    ///
    /// **`outer_width` 语义**：output_document_width = content_area.width（不含 gutter），
    /// 由调用方（`App::output_document_width()`）传入。block 内部 wrap 用的 `text_width`
    /// 由 `render_node` 按 depth 扣除 gutter 派生。
    #[cfg(test)]
    pub fn render_tree(
        &mut self,
        view_model: &OutputViewModel,
        outer_width: u16,
    ) -> RenderedDocument {
        self.render_tree_with_window(
            view_model,
            outer_width,
            0,
            crate::tui::render::output::spacing::MarkdownSpacingPolicy::normal(),
            OutputRenderWindow::all(),
        )
        .document
    }

    /// 测试和独立 renderer 调用使用的无窗口兼容入口。
    #[cfg(test)]
    pub fn render_tree_with_animation_frame(
        &mut self,
        view_model: &OutputViewModel,
        outer_width: u16,
        animation_frame: u64,
        markdown_spacing: crate::tui::render::output::spacing::MarkdownSpacingPolicy,
    ) -> RenderedDocument {
        self.render_tree_with_window(
            view_model,
            outer_width,
            animation_frame,
            markdown_spacing,
            OutputRenderWindow::all(),
        )
        .document
    }

    pub fn render_tree_with_window(
        &mut self,
        view_model: &OutputViewModel,
        outer_width: u16,
        animation_frame: u64,
        markdown_spacing: crate::tui::render::output::spacing::MarkdownSpacingPolicy,
        _window: OutputRenderWindow,
    ) -> OutputRenderResult {
        #[cfg(test)]
        let started = std::time::Instant::now();
        let mut groups = Vec::with_capacity(view_model.roots.len());
        for root in &view_model.roots {
            groups.push(self.render_root_group(
                root,
                outer_width,
                animation_frame,
                markdown_spacing,
            ));
        }
        let mut document = RenderedDocument::with_root_groups(groups);
        let folded_earlier_lines = view_model.folded_earlier_lines;
        if folded_earlier_lines > 0 {
            prepend_folded_history_hint(&mut document, folded_earlier_lines);
            document.rebuild_line_index();
        }
        #[cfg(test)]
        {
            self.retained_cache_peak.peak_block_entries = self
                .retained_cache_peak
                .peak_block_entries
                .max(self.cache.len());
            self.retained_cache_peak.peak_gutted_entries = self
                .retained_cache_peak
                .peak_gutted_entries
                .max(self.gutted.len());
            crate::tui::render::performance::record_document_render(started.elapsed());
        }
        let source_total_lines = view_model.source_total_lines.unwrap_or_else(|| {
            document
                .total_lines()
                .saturating_sub(usize::from(folded_earlier_lines > 0))
        });
        OutputRenderResult {
            document,
            source_total_lines,
            folded_earlier_lines,
        }
    }

    fn render_root_group(
        &mut self,
        root: &BlockNode,
        outer_width: u16,
        animation_frame: u64,
        markdown_spacing: crate::tui::render::output::spacing::MarkdownSpacingPolicy,
    ) -> Vec<RenderedBlock> {
        let mut group = Vec::new();
        self.render_node(
            root,
            outer_width,
            0,
            animation_frame,
            markdown_spacing,
            &mut group,
        );
        group
    }

    fn render_node(
        &mut self,
        node: &BlockNode,
        outer_width: u16,
        depth: usize,
        animation_frame: u64,
        markdown_spacing: crate::tui::render::output::spacing::MarkdownSpacingPolicy,
        out: &mut Vec<RenderedBlock>,
    ) {
        // #329 契约：block 内部 wrap 宽度 = outer_width - gutter_width(depth)，
        // 保证 wrap 后 line 加回 gutter 总可见宽 ≤ outer_width（content_area.width）。
        // 窄屏模式：极窄屏完全移除 gutter；窄屏消除缩进（depth=0）。
        let effective_depth = if gutter::is_gutter_suppressed(outer_width) || outer_width < 50 {
            0
        } else {
            depth
        };
        let text_width = gutter::effective_block_width(outer_width, effective_depth);
        let _ = animation_frame;

        let gkey = GuttedKey {
            block_version: node.block_version,
            text_width,
            depth,
            markdown_spacing,
        };
        // gutted 缓存命中：key 完全一致时直接复用（lines 为 Rc，clone 廉价）。
        if let Some((cached_key, cached_block)) = self.gutted.get(&node.block_id) {
            if *cached_key == gkey {
                #[cfg(test)]
                crate::tui::render::performance::record_gutted_cache_hit();
                out.push(cached_block.clone());
                for child in &node.children {
                    self.render_node(
                        child,
                        outer_width,
                        depth + 1,
                        animation_frame,
                        markdown_spacing,
                        out,
                    );
                }
                return;
            }
            #[cfg(test)]
            {
                if cached_key.block_version != gkey.block_version {
                    crate::tui::render::performance::record_gutted_cache_version_miss();
                }
                if cached_key.text_width != gkey.text_width {
                    crate::tui::render::performance::record_gutted_cache_width_miss();
                }
                if cached_key.depth != gkey.depth {
                    crate::tui::render::performance::record_gutted_cache_depth_miss();
                }
                if cached_key.markdown_spacing != gkey.markdown_spacing {
                    crate::tui::render::performance::record_gutted_cache_spacing_miss();
                }
            }
        } else {
            #[cfg(test)]
            crate::tui::render::performance::record_gutted_cache_absent_miss();
        }
        #[cfg(test)]
        crate::tui::render::performance::record_gutted_cache_miss();

        // gutted 缓存未命中：走完整 render_self + apply_gutter 路径。
        #[cfg(test)]
        self.gutted_render_count
            .set(self.gutted_render_count.get() + 1);

        let key = CacheKey {
            version: node.block_version,
            text_width,
            markdown_spacing,
        };
        let mut rendered = self.cache.get_or_render(&node.block_id, key, |ctx| {
            #[cfg(test)]
            self.render_count.set(self.render_count.get() + 1);
            node.kind.component().render_self(&node.block_id, ctx)
        });
        if matches!(
            node.kind,
            crate::tui::view_model::output::OutputBlockKind::UserMessage(_)
        ) {
            rendered = rendered.with_line_fill_style(Style::default().bg(theme::USER_BG));
        }
        // gutter（depth 缩进 + marker）在缓存外注入：缓存只存无 gutter 内容，
        // gutter 随 depth/status 变化，故组合期叠加（rendered 已 owned，无借用冲突）。
        // 注：(*rendered.lines).clone() 解 Rc 为 Vec，仅在未命中路径付此开销。
        let gutted = if gutter::is_gutter_suppressed(outer_width) {
            // 极窄屏：完全跳过 gutter
            (*rendered.lines).clone()
        } else {
            crate::tui::render::output::gutter::apply_gutter(
                &node.kind,
                effective_depth,
                (*rendered.lines).clone(),
            )
        };
        let mut gutted = gutted;
        if matches!(
            node.kind,
            crate::tui::view_model::output::OutputBlockKind::UserMessage(_)
        ) {
            wrap_user_message_card_lines(&mut gutted);
        }
        // 每个 root block（depth 0）前加一个空行，分隔相邻对话块（视觉呼吸）；
        // 子块（depth>0，如 tool result）紧贴父块、不额外空行。
        if depth == 0 {
            gutted.insert(0, RenderedLine::default());
        }
        let block = RenderedBlock {
            block_id: rendered.block_id,
            lines: std::rc::Rc::new(gutted),
        };
        // 存入 gutted 缓存，供后续帧复用。
        self.gutted
            .insert(node.block_id.clone(), (gkey, block.clone()));
        out.push(block);
        for child in &node.children {
            self.render_node(
                child,
                outer_width,
                depth + 1,
                animation_frame,
                markdown_spacing,
                out,
            );
        }
    }

    #[cfg(test)]
    pub(crate) fn retained_cache_capacity(&self) -> RendererRetainedCacheCapacity {
        RendererRetainedCacheCapacity {
            block_entries: self.cache.len(),
            gutted_entries: self.gutted.len(),
            ..self.retained_cache_peak
        }
    }

    #[cfg(test)]
    pub fn render_count(&self) -> usize {
        self.render_count.get()
    }

    /// gutted 缓存未命中次数（即实际重新渲染次数）；用于测试断言缓存命中行为。
    #[cfg(test)]
    pub fn gutted_render_count(&self) -> usize {
        self.gutted_render_count.get()
    }
}

fn estimate_block_lines(kind: &OutputBlockKind, text_width: usize) -> usize {
    match kind {
        OutputBlockKind::UserMessage(view) => {
            estimate_wrapped_text_lines(&view.text, text_width, false)
                .max(1)
                .saturating_add(2)
        }
        OutputBlockKind::AssistantMessage(view) => {
            estimate_wrapped_text_lines(&view.text, text_width, true).max(1)
        }
        OutputBlockKind::ThinkingMessage(view) => {
            estimate_wrapped_text_lines(&view.text, text_width, false).max(1)
        }
        OutputBlockKind::ToolCall(_) => {
            // #1547：streaming preview 已升为独立 ToolResult 子块，ToolCall 自身
            // 仅渲染 header + detail 行，不再估算 activity 行。
            1usize
        }
        OutputBlockKind::ToolResult(view) => estimate_tool_result_lines(view, text_width),
        OutputBlockKind::HookNotice(view) => 1usize.saturating_add(
            view.body
                .lines()
                .count()
                .saturating_add(usize::from(view.body.ends_with('\n'))),
        ),
        OutputBlockKind::DiagnosticNotice(view) | OutputBlockKind::SystemNotice(view) => view
            .text
            .lines()
            .count()
            .saturating_add(usize::from(view.text.ends_with('\n'))),
        OutputBlockKind::AskUserBatch(view) => match (view.completion, view.phase) {
            (
                crate::tui::view_model::output::AskUserCompletionView::Answered
                | crate::tui::view_model::output::AskUserCompletionView::Cancelled
                | crate::tui::view_model::output::AskUserCompletionView::ReplyPending
                | crate::tui::view_model::output::AskUserCompletionView::CancelPending,
                _,
            ) => 2usize.saturating_add(view.slots.len().saturating_mul(3)),
            (
                crate::tui::view_model::output::AskUserCompletionView::Active,
                AskUserPhaseView::Confirming,
            ) => 6usize.saturating_add(view.slots.len().saturating_mul(2)),
            (
                crate::tui::view_model::output::AskUserCompletionView::Active,
                AskUserPhaseView::Answering,
            ) => {
                let answered = view
                    .slots
                    .iter()
                    .enumerate()
                    .filter(|(index, slot)| *index != view.active_index && slot.answer.is_some())
                    .count();
                let active = view.slots.get(view.active_index);
                let options = active.map(|slot| slot.options.len()).unwrap_or(0);
                5usize
                    .saturating_add(answered)
                    .saturating_add(options.saturating_mul(2))
            }
        },
    }
}

fn estimate_tool_result_lines(
    view: &crate::tui::view_model::output::ToolResultBlockView,
    text_width: usize,
) -> usize {
    match result_policy(&view.tool_title) {
        ResultPolicy::Hidden => 0,
        ResultPolicy::Visible {
            max_lines,
            render_kind,
            tail_mode: _,
        } => match render_kind {
            ResultRender::Plain => max_lines.unwrap_or(5).saturating_add(1),
            ResultRender::Diff => {
                let (old_lines, new_lines) = view
                    .data
                    .as_ref()
                    .and_then(|data| Some((data.get("old")?.as_str()?, data.get("new")?.as_str()?)))
                    .map(|(old, new)| (old.lines().count(), new.lines().count()))
                    .unwrap_or_else(|| {
                        let marker_count = view
                            .result_text
                            .lines()
                            .filter(|line| line.starts_with("---DIFF"))
                            .count();
                        let source_lines = view
                            .result_text
                            .lines()
                            .count()
                            .saturating_sub(marker_count + 1);
                        (
                            source_lines / 2,
                            source_lines.saturating_sub(source_lines / 2),
                        )
                    });
                old_lines
                    .max(new_lines)
                    .saturating_add(2)
                    .max(estimate_wrapped_line_count(&view.result_text, text_width).min(8))
            }
        },
    }
}

fn estimate_wrapped_text_lines(text: &str, width: usize, preserve_blank: bool) -> usize {
    text.lines().fold(0usize, |total, line| {
        let lines = if line.is_empty() {
            usize::from(preserve_blank)
        } else {
            estimate_wrapped_line_count(line, width)
        };
        total.saturating_add(lines)
    })
}

fn estimate_wrapped_line_count(line: &str, width: usize) -> usize {
    if line.is_empty() {
        return 1;
    }
    let width = width.max(1);
    str_display_width(line).max(1).div_ceil(width)
}

fn prepend_folded_history_hint(document: &mut RenderedDocument, folded_lines: usize) {
    let text = format!("─── 更早的消息已折叠（{folded_lines} 行）───");
    let hint = RenderedBlock {
        block_id: "_folded_hint".into(),
        lines: Rc::new(vec![RenderedLine::with_plain(
            vec![Span::styled(
                text.clone(),
                Style::default().fg(crate::tui::render::theme::TEXT_DIM),
            )],
            text,
        )]),
    };
    document.blocks.insert(0, hint);
    document.root_group_block_counts.insert(0, 1);
}

fn wrap_user_message_card_lines(lines: &mut Vec<RenderedLine>) {
    let gutter_cols = lines.first().map(|line| line.gutter_cols).unwrap_or(0);
    let spacer = user_message_card_spacer_line(gutter_cols);
    lines.insert(0, spacer.clone());
    lines.push(spacer);
}

fn user_message_card_spacer_line(gutter_cols: usize) -> RenderedLine {
    let mut line = RenderedLine::empty().with_fill_style(Style::default().bg(theme::USER_BG));
    line.gutter_cols = gutter_cols;
    line
}

#[cfg(test)]
mod tests;
