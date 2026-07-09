//! 输出文档渲染器：遍历 ViewModel.blocks，经 block 级缓存产出 RenderedDocument。

use crate::tui::render::output::block_cache::{BlockCache, CacheKey};
use crate::tui::render::output::gutter;
use crate::tui::render::output::rendered::{RenderedBlock, RenderedDocument, RenderedLine};
use crate::tui::render::output_area::types::MAX_LINES;
use crate::tui::render::theme;
use crate::tui::view_model::output::{BlockNode, OutputViewModel};
use ratatui::style::Style;
use std::collections::{HashMap, HashSet};

/// gutted 缓存的 key：唯一决定 gutted block 内容（含 gutter）的所有参数。
/// 静态 block 的 `marker_frame` 为 `None`；运行中 ToolCall 每次闪烁周期推进时失效。
#[derive(PartialEq, Eq, Clone)]
struct GuttedKey {
    block_version: u64,
    text_width: u16,
    depth: usize,
    /// 仅运行中 ToolCall 有值（= animation_frame / BLINK_DIVISOR），其他 block 为 None。
    marker_frame: Option<u64>,
}

#[derive(Default)]
pub struct OutputDocumentRenderer {
    cache: BlockCache,
    /// 带 gutter 的 block 缓存：key = block_id，value = (GuttedKey, gutted RenderedBlock)。
    /// 命中时直接 clone（lines 为 Rc，廉价）；未命中则走完整 render_self + apply_gutter 路径。
    gutted: HashMap<String, (GuttedKey, RenderedBlock)>,
    #[cfg(test)]
    render_count: std::cell::Cell<usize>,
    /// 统计 gutted 缓存未命中（重新渲染）次数，用于测试断言。
    #[cfg(test)]
    gutted_render_count: std::cell::Cell<usize>,
}

impl OutputDocumentRenderer {
    pub fn render_model_document(
        &mut self,
        view_model: &OutputViewModel,
        outer_width: u16,
        fallback_width: usize,
        animation_frame: u64,
    ) -> RenderedDocument {
        let render_width = if outer_width > 1 {
            outer_width
        } else {
            u16::try_from(fallback_width.max(1)).unwrap_or(u16::MAX)
        };
        self.render_tree_with_animation_frame(view_model, render_width, animation_frame)
    }

    /// 递归走 `view_model.roots`（DFS：父块先于子块），经 block 级缓存展平为线性文档。
    /// gutter（depth 缩进 + marker）在组合期注入。
    ///
    /// **`outer_width` 语义**：output_document_width = content_area.width（不含 gutter），
    /// 由调用方（`App::output_document_width()`）传入。block 内部 wrap 用的 `text_width`
    /// 由 `render_node` 按 depth 扣除 gutter 派生。
    pub fn render_tree(
        &mut self,
        view_model: &OutputViewModel,
        outer_width: u16,
    ) -> RenderedDocument {
        self.render_tree_with_animation_frame(view_model, outer_width, 0)
    }

    /// 带动画帧的 render_tree；动画只进入缓存外 gutter，不参与 block 内容缓存。
    pub fn render_tree_with_animation_frame(
        &mut self,
        view_model: &OutputViewModel,
        outer_width: u16,
        animation_frame: u64,
    ) -> RenderedDocument {
        // 按 root 分组渲染：每个 root 子树（父块 + 全部后代）落入独立 group，
        // 以便 MAX_LINES 裁剪以整棵子树为单位，NEVER 切断 parent/child 关系。
        let mut groups: Vec<Vec<RenderedBlock>> = Vec::new();
        for root in &view_model.roots {
            let mut group = Vec::new();
            self.render_node(root, outer_width, 0, animation_frame, &mut group);
            groups.push(group);
        }
        let blocks = trim_root_groups_to_max_lines(groups, MAX_LINES);
        // O(n) 构建 HashSet，使后续两处 retain 各降为 O(n) 而非 O(n²)。
        let live_set: HashSet<&str> = blocks.iter().map(|b| b.block_id.as_str()).collect();
        self.cache.retain(&live_set);
        // gutted 缓存同步清理：移除已不在渲染树中的条目，防止内存泄漏。
        self.gutted.retain(|id, _| live_set.contains(id.as_str()));
        RenderedDocument { blocks }
    }

    fn render_node(
        &mut self,
        node: &BlockNode,
        outer_width: u16,
        depth: usize,
        animation_frame: u64,
        out: &mut Vec<RenderedBlock>,
    ) {
        // #329 契约：block 内部 wrap 宽度 = outer_width - gutter_width(depth)，
        // 保证 wrap 后 line 加回 gutter 总可见宽 ≤ outer_width（content_area.width）。
        let text_width = gutter::effective_block_width(outer_width, depth);

        // 计算 gutted 缓存 key：运行中 ToolCall 的 marker_frame 随动画帧推进而变化，
        // 导致每次闪烁周期自动失效；其他 block 的 marker_frame 为 None，跨帧稳定命中。
        let marker_frame = match &node.kind {
            crate::tui::view_model::output::OutputBlockKind::ToolCall(t)
                if t.semantic_status
                    == crate::tui::view_model::output::ToolSemanticStatus::Running =>
            {
                Some(animation_frame / gutter::TOOL_MARKER_BLINK_DIVISOR)
            }
            crate::tui::view_model::output::OutputBlockKind::ModelStreamPlaceholder(_) => Some(
                animation_frame
                    / crate::tui::render::output::blocks::thinking_placeholder::THINKING_DOT_FRAME_DIVISOR,
            ),
            _ => None,
        };
        let gkey = GuttedKey {
            block_version: node.block_version,
            text_width,
            depth,
            marker_frame,
        };

        // gutted 缓存命中：key 完全一致时直接复用（lines 为 Rc，clone 廉价）。
        if let Some((cached_key, cached_block)) = self.gutted.get(&node.block_id) {
            if *cached_key == gkey {
                out.push(cached_block.clone());
                for child in &node.children {
                    self.render_node(child, outer_width, depth + 1, animation_frame, out);
                }
                return;
            }
        }

        // gutted 缓存未命中：走完整 render_self + apply_gutter 路径。
        #[cfg(test)]
        self.gutted_render_count
            .set(self.gutted_render_count.get() + 1);

        let key = CacheKey {
            version: match marker_frame {
                Some(frame)
                    if matches!(
                        node.kind,
                        crate::tui::view_model::output::OutputBlockKind::ModelStreamPlaceholder(_)
                    ) =>
                {
                    node.block_version ^ frame
                }
                _ => node.block_version,
            },
            text_width,
        };
        let mut rendered = self.cache.get_or_render(&node.block_id, key, |ctx| {
            #[cfg(test)]
            self.render_count.set(self.render_count.get() + 1);
            match &node.kind {
                crate::tui::view_model::output::OutputBlockKind::ModelStreamPlaceholder(
                    placeholder,
                ) => crate::tui::render::output::blocks::thinking_placeholder::render_model_stream_placeholder(
                    &node.block_id,
                    placeholder,
                    ctx,
                    animation_frame,
                ),
                kind => kind.component().render_self(&node.block_id, ctx),
            }
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
        let mut gutted = crate::tui::render::output::gutter::apply_gutter_with_frame(
            &node.kind,
            depth,
            (*rendered.lines).clone(),
            animation_frame,
        );
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
            self.render_node(child, outer_width, depth + 1, animation_frame, out);
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

/// 按 root 子树整组裁剪：从尾部（最新）向前累计每个 group 的总行数，
/// 仅当加入该 group 不超过 `max_lines` 时保留整组；NEVER 拆分 group
/// （即父块与其后代要么整体保留、要么整体丢弃）。
/// 边界语义与旧的 per-block 裁剪一致：最新一组即便单独超限也始终保留
/// （首组跳过超限判断），避免输出为空。
fn trim_root_groups_to_max_lines(
    groups: Vec<Vec<RenderedBlock>>,
    max_lines: usize,
) -> Vec<RenderedBlock> {
    if max_lines == 0 {
        return Vec::new();
    }

    let mut kept: Vec<Vec<RenderedBlock>> = Vec::new();
    let mut used = 0usize;
    for group in groups.into_iter().rev() {
        let group_lines: usize = group.iter().map(|b| b.lines.len()).sum();
        if used > 0 && used.saturating_add(group_lines) > max_lines {
            break;
        }
        used = used.saturating_add(group_lines);
        kept.push(group);
    }
    kept.reverse();
    kept.into_iter().flatten().collect()
}

#[cfg(test)]
mod tests;
