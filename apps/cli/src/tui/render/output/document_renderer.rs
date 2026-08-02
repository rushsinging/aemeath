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
    AskUserPhaseView, BlockNode, OutputBlockKind, OutputViewModel,
};
use ratatui::style::Style;
use ratatui::text::Span;
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;

/// 输出文档渲染请求。窗口从完整语义树的最新端按完整 root group 选择。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputRenderWindow {
    pub line_limit: usize,
    pub tail_offset: usize,
}

impl OutputRenderWindow {
    /// 不限制 root 选择，供不需要历史窗口的独立 renderer 调用方使用。
    const fn all() -> Self {
        Self {
            line_limit: usize::MAX,
            tail_offset: 0,
        }
    }
}

/// 单次窗口渲染结果。`source_total_lines` 来自全部 root 布局索引，
/// `document` 只持请求窗口内的 rendered blocks。
pub struct OutputRenderResult {
    pub document: RenderedDocument,
    pub source_total_lines: usize,
    pub folded_earlier_lines: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SelectedRootWindow {
    root_range: Range<usize>,
    source_total_lines: usize,
    folded_earlier_lines: usize,
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

#[derive(Clone, Copy, PartialEq, Eq)]
struct RootLayoutKey {
    fingerprint: u64,
    outer_width: u16,
    markdown_spacing: crate::tui::render::output::spacing::MarkdownSpacingPolicy,
}

impl RootLayoutKey {
    fn matches_semantics(self, other: Self) -> bool {
        self.fingerprint == other.fingerprint && self.markdown_spacing == other.markdown_spacing
    }
}

#[derive(Clone, Copy)]
struct RootLayoutEntry {
    key: RootLayoutKey,
    line_count: usize,
    exact: bool,
}

#[derive(Clone, Copy, Default)]
struct RootLayoutState {
    line_count: usize,
    needs_exact_layout: bool,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RendererRetainedCacheCapacity {
    pub block_entries: usize,
    pub gutted_entries: usize,
    pub root_layout_entries: usize,
    pub peak_block_entries: usize,
    pub peak_gutted_entries: usize,
    pub peak_root_layout_entries: usize,
}

pub struct OutputDocumentRenderer {
    cache: BlockCache,
    /// root 子树的轻量布局索引。只保留 key 与总行数，不持有 rendered lines。
    /// 用于在执行 Markdown/diff/syntax render 前选择请求的历史窗口。
    root_layouts: HashMap<String, RootLayoutEntry>,
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
            root_layouts: HashMap::new(),
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
        window: OutputRenderWindow,
    ) -> OutputRenderResult {
        #[cfg(test)]
        let started = std::time::Instant::now();
        let semantic_root_ids: HashSet<&str> = view_model
            .roots
            .iter()
            .map(|root| root.block_id.as_str())
            .collect();
        self.root_layouts
            .retain(|id, _| semantic_root_ids.contains(id.as_str()));

        let mut root_layout_states = Vec::with_capacity(view_model.roots.len());
        for root in &view_model.roots {
            let key = root_layout_key(root, outer_width, animation_frame, markdown_spacing);
            let state = match self.root_layouts.get(&root.block_id).copied() {
                Some(entry) if entry.key == key && entry.exact => RootLayoutState {
                    line_count: entry.line_count,
                    needs_exact_layout: false,
                },
                Some(entry) if entry.key.matches_semantics(key) => RootLayoutState {
                    line_count: entry.line_count,
                    needs_exact_layout: true,
                },
                _ => {
                    let line_count = estimate_root_group_lines(root, outer_width);
                    self.root_layouts.insert(
                        root.block_id.clone(),
                        RootLayoutEntry {
                            key,
                            line_count,
                            exact: false,
                        },
                    );
                    RootLayoutState {
                        line_count,
                        needs_exact_layout: true,
                    }
                }
            };
            root_layout_states.push(state);
        }

        let mut selected = select_root_window(&root_layout_states, window);
        let mut selected_groups = HashMap::new();
        loop {
            let selected_stale = selected
                .root_range
                .clone()
                .filter(|index| root_layout_states[*index].needs_exact_layout)
                .collect::<Vec<_>>();
            if selected_stale.is_empty() {
                break;
            }
            for index in selected_stale {
                let root = &view_model.roots[index];
                let key = root_layout_key(root, outer_width, animation_frame, markdown_spacing);
                let group =
                    self.render_root_group(root, outer_width, animation_frame, markdown_spacing);
                let line_count = root_group_line_count(&group);
                self.root_layouts.insert(
                    root.block_id.clone(),
                    RootLayoutEntry {
                        key,
                        line_count,
                        exact: true,
                    },
                );
                root_layout_states[index] = RootLayoutState {
                    line_count,
                    needs_exact_layout: false,
                };
                selected_groups.insert(root.block_id.clone(), group);
            }
            selected = select_root_window(&root_layout_states, window);
        }

        let mut groups = Vec::with_capacity(selected.root_range.len());
        for root in &view_model.roots[selected.root_range.clone()] {
            if let Some(group) = selected_groups.remove(&root.block_id) {
                groups.push(group);
            } else {
                groups.push(self.render_root_group(
                    root,
                    outer_width,
                    animation_frame,
                    markdown_spacing,
                ));
            }
        }
        let mut document = RenderedDocument::with_root_groups(groups);
        if selected.folded_earlier_lines > 0 {
            prepend_folded_history_hint(&mut document, selected.folded_earlier_lines);
            document.rebuild_line_index();
        }
        let semantic_block_ids = collect_semantic_block_ids(&view_model.roots);
        self.cache.retain(&semantic_block_ids);
        let gutted_evictions = self
            .gutted
            .retain(|id, _| semantic_block_ids.contains(id.as_str()));
        #[cfg(test)]
        crate::tui::render::performance::record_gutted_cache_retain_evictions(gutted_evictions);
        #[cfg(not(test))]
        let _ = gutted_evictions;
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
            self.retained_cache_peak.peak_root_layout_entries = self
                .retained_cache_peak
                .peak_root_layout_entries
                .max(self.root_layouts.len());
            crate::tui::render::performance::record_document_render(started.elapsed());
        }
        OutputRenderResult {
            document,
            source_total_lines: selected.source_total_lines,
            folded_earlier_lines: selected.folded_earlier_lines,
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
            root_layout_entries: self.root_layouts.len(),
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

fn root_group_line_count(group: &[RenderedBlock]) -> usize {
    group
        .iter()
        .map(|block| block.lines.len())
        .fold(0usize, usize::saturating_add)
}

fn estimate_root_group_lines(root: &BlockNode, outer_width: u16) -> usize {
    fn estimate(node: &BlockNode, outer_width: u16, depth: usize) -> usize {
        let effective_depth = if gutter::is_gutter_suppressed(outer_width) || outer_width < 50 {
            0
        } else {
            depth
        };
        let text_width = gutter::effective_block_width(outer_width, effective_depth) as usize;
        let own_lines =
            estimate_block_lines(&node.kind, text_width).saturating_add(usize::from(depth == 0));
        node.children.iter().fold(own_lines, |total, child| {
            total.saturating_add(estimate(child, outer_width, depth + 1))
        })
    }

    estimate(root, outer_width, 0).max(1)
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
        OutputBlockKind::ToolCall(view) => {
            let activity_lines = view.activity_lines.iter().fold(0usize, |total, line| {
                total.saturating_add(estimate_wrapped_line_count(line, text_width))
            });
            1usize.saturating_add(activity_lines)
        }
        OutputBlockKind::ToolResult(view) => estimate_tool_result_lines(view, text_width),
        OutputBlockKind::DiagnosticNotice(view) | OutputBlockKind::SystemNotice(view) => view
            .text
            .lines()
            .count()
            .saturating_add(usize::from(view.text.ends_with('\n'))),
        OutputBlockKind::AskUserBatch(view) => match (view.confirmed, view.phase) {
            (true, _) => 2usize.saturating_add(view.slots.len().saturating_mul(3)),
            (false, AskUserPhaseView::Confirming) => {
                6usize.saturating_add(view.slots.len().saturating_mul(2))
            }
            (false, AskUserPhaseView::Answering) => {
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

fn collect_semantic_block_ids(roots: &[BlockNode]) -> HashSet<&str> {
    fn collect<'a>(node: &'a BlockNode, ids: &mut HashSet<&'a str>) {
        ids.insert(node.block_id.as_str());
        for child in &node.children {
            collect(child, ids);
        }
    }

    let mut ids = HashSet::new();
    for root in roots {
        collect(root, &mut ids);
    }
    ids
}

fn root_layout_key(
    root: &BlockNode,
    outer_width: u16,
    _animation_frame: u64,
    markdown_spacing: crate::tui::render::output::spacing::MarkdownSpacingPolicy,
) -> RootLayoutKey {
    let mut hasher = DefaultHasher::new();
    hash_node_layout(root, &mut hasher);
    RootLayoutKey {
        fingerprint: hasher.finish(),
        outer_width,
        markdown_spacing,
    }
}

fn hash_node_layout(node: &BlockNode, hasher: &mut DefaultHasher) {
    node.block_id.hash(hasher);
    node.block_version.hash(hasher);
    node.children.len().hash(hasher);
    for child in &node.children {
        hash_node_layout(child, hasher);
    }
}

/// 根据已知 root group 行数，从最新端跳过 `tail_offset` 覆盖的完整 root，
/// 再向旧端选择 `line_limit` 覆盖的完整 root。边界 root 永不拆分。
fn select_root_window_from_counts(
    root_line_counts: &[usize],
    window: OutputRenderWindow,
) -> SelectedRootWindow {
    let source_total_lines = root_line_counts
        .iter()
        .fold(0usize, |total, lines| total.saturating_add(*lines));
    select_root_range(root_line_counts, window, source_total_lines)
}

fn select_root_window(
    root_layout_states: &[RootLayoutState],
    window: OutputRenderWindow,
) -> SelectedRootWindow {
    let source_total_lines = root_layout_states.iter().fold(0usize, |total, state| {
        total.saturating_add(state.line_count)
    });
    if window.tail_offset >= source_total_lines {
        let exact_prefix_lines = root_layout_states
            .iter()
            .scan(true, |prefix_exact, state| {
                *prefix_exact &= !state.needs_exact_layout;
                Some(if *prefix_exact { state.line_count } else { 0 })
            })
            .collect::<Vec<_>>();
        return select_root_range(&exact_prefix_lines, window, source_total_lines);
    }
    let estimated_lines = root_layout_states
        .iter()
        .map(|state| state.line_count)
        .collect::<Vec<_>>();
    select_root_range(&estimated_lines, window, source_total_lines)
}

fn select_root_range(
    root_line_counts: &[usize],
    window: OutputRenderWindow,
    source_total_lines: usize,
) -> SelectedRootWindow {
    if window.line_limit == 0 || root_line_counts.is_empty() {
        return SelectedRootWindow {
            root_range: root_line_counts.len()..root_line_counts.len(),
            source_total_lines,
            folded_earlier_lines: source_total_lines,
        };
    }

    let mut end = root_line_counts.len();
    let mut skipped_newer_lines = 0usize;
    while end > 0 && skipped_newer_lines < window.tail_offset {
        end -= 1;
        skipped_newer_lines = skipped_newer_lines.saturating_add(root_line_counts[end]);
    }

    let mut start = end;
    let mut selected_lines = 0usize;
    while start > 0 {
        let candidate_lines = root_line_counts[start - 1];
        if selected_lines > 0 && selected_lines.saturating_add(candidate_lines) > window.line_limit
        {
            break;
        }
        start -= 1;
        selected_lines = selected_lines.saturating_add(candidate_lines);
        if selected_lines > window.line_limit {
            break;
        }
    }

    SelectedRootWindow {
        root_range: start..end,
        source_total_lines,
        folded_earlier_lines: root_line_counts[..start]
            .iter()
            .fold(0usize, |total, lines| total.saturating_add(*lines)),
    }
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
