use std::collections::{HashMap, HashSet};

use ratatui::text::Span;

use crate::tui::render::output::markdown;
use crate::tui::render::output_area::{LineStyle, OutputLine};

#[allow(dead_code)]
pub(super) struct CodeBlockInfo {
    pub code_block_lines: HashSet<usize>,
    pub code_fence_lines: HashSet<usize>,
    pub code_lang_label: HashMap<usize, String>,
}

/// 扫描可见行中的代码块信息。
#[allow(dead_code)]
pub(super) fn scan_code_blocks<'a, L>(
    all_lines: L,
    visible: &[(usize, &'a OutputLine)],
) -> CodeBlockInfo
where
    L: Iterator<Item = &'a OutputLine>,
{
    // 从文档开头预扫描到可见区域起始位置，确定 in_code_block 初始状态
    let start_idx = visible.first().map(|(i, _)| *i).unwrap_or(0);
    let mut in_code_block = false;
    for line in all_lines.take(start_idx) {
        let is_md = matches!(line.style, LineStyle::Assistant | LineStyle::Thinking);
        if !is_md {
            in_code_block = false;
            continue;
        }
        if line.content.trim().starts_with("```") {
            in_code_block = !in_code_block;
        }
    }

    let mut code_block_lines = HashSet::new();
    let mut code_fence_lines = HashSet::new();
    let mut code_lang_label = HashMap::new();

    for &(i, line) in visible {
        let is_markdown_style = matches!(line.style, LineStyle::Assistant | LineStyle::Thinking);
        if !is_markdown_style {
            in_code_block = false;
            continue;
        }
        if line.content.trim().starts_with("```") {
            if !in_code_block {
                let lang = line
                    .content
                    .trim()
                    .strip_prefix("```")
                    .unwrap_or("")
                    .trim()
                    .to_string();
                code_lang_label.insert(i, lang);
            }
            in_code_block = !in_code_block;
            code_fence_lines.insert(i);
            code_block_lines.insert(i);
        } else if in_code_block && is_markdown_style {
            code_block_lines.insert(i);
        }
    }

    CodeBlockInfo {
        code_block_lines,
        code_fence_lines,
        code_lang_label,
    }
}

/// 扫描可见行中的表格块信息。
#[allow(dead_code)]
pub(super) fn scan_table_blocks<'a, L>(
    all_lines: L,
    visible: &[(usize, &'a OutputLine)],
) -> HashSet<usize>
where
    L: Iterator<Item = &'a OutputLine>,
{
    let start_idx = visible.first().map(|(i, _)| *i).unwrap_or(0);

    // 预扫描不可见部分，收集完整的表格块（header + separator + 数据行）。
    // 一个表格块的定义是：连续的 is_table_row / is_table_separator 行，
    // 且其中至少包含一个 separator 行。
    let mut pending_block_start: Option<usize> = None;
    let mut pending_has_sep = false;
    let mut crossed_blocks: Vec<std::ops::Range<usize>> = Vec::new();

    for (i, line) in all_lines.enumerate().take(start_idx) {
        let is_md = matches!(
            line.style,
            LineStyle::Assistant | LineStyle::Thinking | LineStyle::System
        );
        let trimmed = line.content.trim();
        let is_tbl =
            is_md && (markdown::is_table_row(trimmed) || markdown::is_table_separator(trimmed));

        if is_tbl {
            if pending_block_start.is_none() {
                pending_block_start = Some(i);
            }
            if markdown::is_table_separator(trimmed) {
                pending_has_sep = true;
            }
        } else if let Some(s) = pending_block_start.take() {
            if pending_has_sep {
                crossed_blocks.push(s..i);
            }
            pending_has_sep = false;
        }
    }
    // 处理末尾未关闭的块（可能延伸到可见区域）
    if let Some(s) = pending_block_start.take() {
        if pending_has_sep {
            // 找到该块在可见区域的延续
            let mut end = start_idx;
            for &(vi, line) in visible {
                let is_md = matches!(
                    line.style,
                    LineStyle::Assistant | LineStyle::Thinking | LineStyle::System
                );
                let trimmed = line.content.trim();
                if is_md
                    && (markdown::is_table_row(trimmed) || markdown::is_table_separator(trimmed))
                {
                    end = vi + 1;
                } else {
                    break;
                }
            }
            if end > s {
                crossed_blocks.push(s..end);
            }
        }
    }

    // 扫描可见区域内完整的表格块
    let mut table_block_lines = HashSet::new();
    let mut i = 0;
    while i < visible.len() {
        let (_, line) = visible[i];
        let is_md = matches!(
            line.style,
            LineStyle::Assistant | LineStyle::Thinking | LineStyle::System
        );
        if is_md && markdown::is_table_row(&line.content) {
            let block_start = i;
            let mut block_end = i + 1;
            while block_end < visible.len() {
                let (_, next_line) = visible[block_end];
                let next_is_md = matches!(
                    next_line.style,
                    LineStyle::Assistant | LineStyle::Thinking | LineStyle::System
                );
                let trimmed = next_line.content.trim();
                if next_is_md
                    && (markdown::is_table_row(trimmed) || markdown::is_table_separator(trimmed))
                {
                    block_end += 1;
                } else {
                    break;
                }
            }

            let has_separator = (block_start..block_end)
                .any(|j| markdown::is_table_separator(visible[j].1.content.trim()));
            if has_separator {
                for &(line_idx, _) in visible.iter().take(block_end).skip(block_start) {
                    table_block_lines.insert(line_idx);
                }
            }
            i = block_end;
        } else {
            i += 1;
        }
    }

    // 标记跨视口边界的表格块的可见行
    for range in crossed_blocks {
        for &(vi, _) in visible {
            if range.contains(&vi) {
                table_block_lines.insert(vi);
            }
        }
    }

    table_block_lines
}

/// 表格渲染缓存，key 为可见区域起始行索引。
#[allow(dead_code)]
pub(super) fn render_table_cache<'a, L>(
    all_lines: L,
    visible: &[(usize, &'a OutputLine)],
    table_block_lines: &HashSet<usize>,
) -> HashMap<usize, Vec<Vec<Span<'static>>>>
where
    L: Iterator<Item = &'a OutputLine>,
{
    let all_vec: Vec<&OutputLine> = all_lines.collect();
    let mut table_render_cache = HashMap::new();
    let mut i = 0;

    while i < visible.len() {
        let (idx, _) = visible[i];
        if table_block_lines.contains(&idx) {
            let block_start = i;
            let mut block_end = i + 1;
            while block_end < visible.len() && table_block_lines.contains(&visible[block_end].0) {
                block_end += 1;
            }

            // 向前查找滚出视口的表格行，找到完整表格块的起始
            let mut full_start = idx;
            for scan_idx in (0..idx).rev() {
                let Some(line) = all_vec.get(scan_idx) else {
                    break;
                };
                let is_md = matches!(
                    line.style,
                    LineStyle::Assistant | LineStyle::Thinking | LineStyle::System
                );
                let trimmed = line.content.trim();
                if is_md
                    && (markdown::is_table_row(trimmed) || markdown::is_table_separator(trimmed))
                {
                    full_start = scan_idx;
                } else {
                    break;
                }
            }

            // 向后查找视口之后的表格行，找到完整表格块的结束
            let last_vis_idx = visible[block_end - 1].0;
            let mut full_end = last_vis_idx + 1;
            for scan_idx in last_vis_idx + 1..all_vec.len() {
                let line = all_vec.get(scan_idx);
                let Some(line) = line else { break };
                let is_md = matches!(
                    line.style,
                    LineStyle::Assistant | LineStyle::Thinking | LineStyle::System
                );
                let trimmed = line.content.trim();
                if is_md
                    && (markdown::is_table_row(trimmed) || markdown::is_table_separator(trimmed))
                {
                    full_end = scan_idx + 1;
                } else {
                    break;
                }
            }

            // 构建完整的表格行列表（全量行参与列宽计算）
            let table_lines: Vec<&str> = (full_start..full_end)
                .filter_map(|li| all_vec.get(li).map(|line| line.content.trim()))
                .filter(|trimmed| {
                    markdown::is_table_row(trimmed) || markdown::is_table_separator(trimmed)
                })
                .collect();

            let base = visible[block_start].1.style.to_style();
            let rendered = markdown::render_table_block(&table_lines, base, 120);

            // render_table_block 输出与 table_lines 一一对应
            // 可见行在 table_lines 中的偏移
            let vis_offset = idx - full_start;
            let vis_rendered: Vec<Vec<Span<'static>>> = rendered
                .into_iter()
                .skip(vis_offset)
                .take(block_end - block_start)
                .collect();

            table_render_cache.insert(visible[block_start].0, vis_rendered);
            i = block_end;
        } else {
            i += 1;
        }
    }

    table_render_cache
}
