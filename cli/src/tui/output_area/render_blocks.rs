use std::collections::{HashMap, HashSet};

use ratatui::text::Span;

use super::{markdown, LineStyle, OutputLine};

pub(super) struct CodeBlockInfo {
    pub code_block_lines: HashSet<usize>,
    pub code_fence_lines: HashSet<usize>,
    pub code_lang_label: HashMap<usize, String>,
}

/// 扫描可见行中的代码块信息。
///
/// `all_lines` 用于从文档开头预扫描不可见部分，确定可见区域开始时
/// 是否处于代码块内部（解决开标记滚出视口后状态丢失的问题）。
/// `visible` 为当前可见的行切片。
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
        let is_md = matches!(
            line.style,
            LineStyle::Assistant | LineStyle::Thinking | LineStyle::System
        );
        if is_md && line.content.trim().starts_with("```") {
            in_code_block = !in_code_block;
        }
    }

    let mut code_block_lines = HashSet::new();
    let mut code_fence_lines = HashSet::new();
    let mut code_lang_label = HashMap::new();

    for &(i, line) in visible {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn md_line(content: &str) -> OutputLine {
        OutputLine {
            content: content.to_string(),
            style: LineStyle::Assistant,
            tool_id: None,
        }
    }

    /// 测试开标记在视口内：正常识别代码块
    #[test]
    fn test_scan_code_blocks_fence_in_viewport() {
        let all = vec![
            md_line("hello"),
            md_line("```rust"),
            md_line("fn main() {}"),
            md_line("```"),
            md_line("after"),
        ];
        let vis: Vec<(usize, &OutputLine)> = all.iter().enumerate().collect();
        let info = scan_code_blocks(all.iter(), &vis);
        assert!(info.code_block_lines.contains(&1));
        assert!(info.code_block_lines.contains(&2));
        assert!(info.code_block_lines.contains(&3));
        assert!(!info.code_block_lines.contains(&4));
        assert_eq!(info.code_lang_label.get(&1).unwrap(), "rust");
    }

    /// 测试开标记滚出视口后，代码块内容仍被正确识别（bug #58 回归）
    #[test]
    fn test_scan_code_blocks_open_fence_scrolled_out() {
        let all = vec![
            md_line("```rust"),   // 0: 开标记，滚出视口
            md_line("fn foo()"),  // 1: 代码内容
            md_line("```"),       // 2: 结束标记，在视口内
            md_line("normal"),    // 3: 应该不是代码块
        ];
        // 只可见行 1~3（行 0 滚出）
        let vis: Vec<(usize, &OutputLine)> = all
            .iter()
            .enumerate()
            .skip(1)
            .collect();
        let info = scan_code_blocks(all.iter(), &vis);
        // 行 1（代码内容）和行 2（结束标记）应该被标记为代码块
        assert!(info.code_block_lines.contains(&1), "code line 1 should be in code block");
        assert!(info.code_block_lines.contains(&2), "closing fence should be in code block");
        // 行 3 不在代码块内
        assert!(!info.code_block_lines.contains(&3), "line after block should NOT be code");
        // 结束标记不应被误认为开标记（不应有 lang label）
        assert!(!info.code_lang_label.contains_key(&2), "closing fence should not have lang label");
    }

    /// 测试整个代码块都滚出视口，视口内无代码块
    #[test]
    fn test_scan_code_blocks_all_outside_viewport() {
        let all = vec![
            md_line("```"),       // 0
            md_line("code"),      // 1
            md_line("```"),       // 2
            md_line("visible"),   // 3
        ];
        let vis: Vec<(usize, &OutputLine)> = vec![(3, &all[3])];
        let info = scan_code_blocks(all.iter(), &vis);
        assert!(!info.code_block_lines.contains(&3));
    }

    /// 测试嵌套反引号（行内代码）不应触发代码块
    #[test]
    fn test_scan_code_blocks_inline_code_not_fence() {
        let all = vec![
            md_line("use `code` here"),
        ];
        let vis: Vec<(usize, &OutputLine)> = all.iter().enumerate().collect();
        let info = scan_code_blocks(all.iter(), &vis);
        assert!(info.code_block_lines.is_empty());
    }
}
