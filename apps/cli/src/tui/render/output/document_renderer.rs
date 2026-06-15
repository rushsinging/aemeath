//! 输出文档渲染器：遍历 ViewModel.blocks，经 block 级缓存产出 RenderedDocument。

use crate::tui::render::output::block_cache::{BlockCache, CacheKey};
use crate::tui::render::output::rendered::{RenderedBlock, RenderedDocument, RenderedLine};
use crate::tui::render::output_area::types::MAX_LINES;
use crate::tui::render::theme;
use crate::tui::view_model::output::{BlockNode, OutputViewModel};
use ratatui::style::Style;

#[derive(Default)]
pub struct OutputDocumentRenderer {
    cache: BlockCache,
    #[cfg(test)]
    render_count: std::cell::Cell<usize>,
}

impl OutputDocumentRenderer {
    pub fn render_model_document(
        &mut self,
        view_model: &OutputViewModel,
        width: u16,
        fallback_width: usize,
        animation_frame: u64,
    ) -> RenderedDocument {
        let render_width = if width > 1 {
            width
        } else {
            u16::try_from(fallback_width.max(1)).unwrap_or(u16::MAX)
        };
        self.render_tree_with_animation_frame(view_model, render_width, animation_frame)
    }

    /// 递归走 `view_model.roots`（DFS：父块先于子块），经 block 级缓存展平为线性文档。
    /// gutter（depth 缩进 + marker）在组合期注入。
    pub fn render_tree(&mut self, view_model: &OutputViewModel, width: u16) -> RenderedDocument {
        self.render_tree_with_animation_frame(view_model, width, 0)
    }

    /// 带动画帧的 render_tree；动画只进入缓存外 gutter，不参与 block 内容缓存。
    pub fn render_tree_with_animation_frame(
        &mut self,
        view_model: &OutputViewModel,
        width: u16,
        animation_frame: u64,
    ) -> RenderedDocument {
        // 按 root 分组渲染：每个 root 子树（父块 + 全部后代）落入独立 group，
        // 以便 MAX_LINES 裁剪以整棵子树为单位，NEVER 切断 parent/child 关系。
        let mut groups: Vec<Vec<RenderedBlock>> = Vec::new();
        for root in &view_model.roots {
            let mut group = Vec::new();
            self.render_node(root, width, 0, animation_frame, &mut group);
            groups.push(group);
        }
        let blocks = trim_root_groups_to_max_lines(groups, MAX_LINES);
        let live_ids = collect_rendered_block_ids(&blocks);
        self.cache.retain(&live_ids);
        RenderedDocument { blocks }
    }

    fn render_node(
        &mut self,
        node: &BlockNode,
        width: u16,
        depth: usize,
        animation_frame: u64,
        out: &mut Vec<RenderedBlock>,
    ) {
        let key = CacheKey {
            version: node.block_version,
            width,
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
        // gutter（depth 缩进 + marker）在缓存外注入：缓存只存无 gutter 内容，        // gutter 随 depth/status 变化，故组合期叠加（rendered 已 owned，无借用冲突）。
        let mut gutted = crate::tui::render::output::gutter::apply_gutter_with_frame(
            &node.kind,
            depth,
            rendered.lines,
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
        out.push(RenderedBlock {
            block_id: rendered.block_id,
            lines: gutted,
        });
        for child in &node.children {
            self.render_node(child, width, depth + 1, animation_frame, out);
        }
    }

    #[cfg(test)]
    pub fn render_count(&self) -> usize {
        self.render_count.get()
    }
}

fn collect_rendered_block_ids(blocks: &[RenderedBlock]) -> Vec<String> {
    blocks.iter().map(|block| block.block_id.clone()).collect()
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
