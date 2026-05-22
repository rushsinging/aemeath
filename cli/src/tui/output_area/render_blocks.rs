use std::collections::{HashMap, HashSet};

use ratatui::text::Span;

use super::{markdown, LineStyle, OutputLine};

pub(super) struct CodeBlockInfo {
    pub code_block_lines: HashSet<usize>,
    pub code_fence_lines: HashSet<usize>,
    pub code_lang_label: HashMap<usize, String>,
}

pub(super) fn scan_code_blocks<'a>(
    lines: impl Iterator<Item = (usize, &'a OutputLine)>,
) -> CodeBlockInfo {
    let mut in_code_block = false;
    let mut code_block_lines = HashSet::new();
    let mut code_fence_lines = HashSet::new();
    let mut code_lang_label = HashMap::new();

    for (i, line) in lines {
        let is_markdown_style = matches!(
            line.style,
            LineStyle::Assistant | LineStyle::Thinking | LineStyle::System
        );
        if is_markdown_style && line.content.trim().starts_with("```") {
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

pub(super) fn scan_table_blocks(visible: &[(usize, &OutputLine)]) -> HashSet<usize> {
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
                for j in block_start..block_end {
                    table_block_lines.insert(visible[j].0);
                }
            }
            i = block_end;
        } else {
            i += 1;
        }
    }

    table_block_lines
}

pub(super) fn render_table_cache(
    visible: &[(usize, &OutputLine)],
    table_block_lines: &HashSet<usize>,
) -> HashMap<usize, Vec<Vec<Span<'static>>>> {
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
            let block_lines: Vec<&str> = (block_start..block_end)
                .map(|j| visible[j].1.content.as_str())
                .collect();
            let base = visible[block_start].1.style.to_style();
            let rendered = markdown::render_table_block(&block_lines, base);
            table_render_cache.insert(visible[block_start].0, rendered);
            i = block_end;
        } else {
            i += 1;
        }
    }

    table_render_cache
}
