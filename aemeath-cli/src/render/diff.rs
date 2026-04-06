use crossterm::style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor};
use crossterm::ExecutableCommand;
use similar::{ChangeTag, TextDiff};
use std::io::{self, Write};
use syntect::easy::HighlightLines;
use syntect::highlighting::Theme as SyntectTheme;
use syntect::parsing::SyntaxSet;

use super::theme::Theme;

// 全局语法高亮资源（懒加载）
static SYNTAX_SET: once_cell::sync::Lazy<SyntaxSet> =
    once_cell::sync::Lazy::new(|| SyntaxSet::load_defaults_newlines());

/// 折叠阈值：超过此行数时触发折叠
const FOLD_THRESHOLD: usize = 20;

/// 对单行代码进行语法高亮，返回带 RGB 颜色的文本段
fn highlight_line(line: &str) -> Vec<((u8, u8, u8), String)> {
    // 尝试检测语法（默认使用 Rust，可以根据需要扩展）
    let syntax = SYNTAX_SET
        .find_syntax_by_extension("rs")
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());
    
    let theme = SyntectTheme::default();
    let mut highlighter = HighlightLines::new(syntax, &theme);
    let highlighted = highlighter
        .highlight_line(line, &SYNTAX_SET)
        .unwrap_or_default();
    
    highlighted
        .into_iter()
        .map(|(style, text)| {
            // 直接使用 syntect 的 RGB 颜色
            let color = (style.foreground.r, style.foreground.g, style.foreground.b);
            (color, text.to_string())
        })
        .collect()
}

pub fn print_diff(old: &str, new: &str) {
    let diff = TextDiff::from_lines(old, new);
    let changes: Vec<_> = diff.iter_all_changes().collect();
    let mut stdout = io::stdout();

    // 按连续块分组：不变块（Equal）和变更块（Delete/Insert）
    let mut blocks = Vec::new();
    let mut current_block = Vec::new();
    let mut current_type: Option<ChangeTag> = None;

    for change in &changes {
        let tag = change.tag();
        let is_change = tag == ChangeTag::Delete || tag == ChangeTag::Insert;
        
        if let Some(ct) = current_type {
            let same_type = (ct == ChangeTag::Equal && tag == ChangeTag::Equal)
                || (ct != ChangeTag::Equal && is_change);
            if same_type {
                current_block.push(*change);
                continue;
            }
        }
        
        if !current_block.is_empty() {
            blocks.push((current_type.unwrap_or(ChangeTag::Equal), current_block));
        }
        current_block = vec![*change];
        current_type = Some(tag);
    }
    
    if !current_block.is_empty() {
        blocks.push((current_type.unwrap_or(ChangeTag::Equal), current_block));
    }

    // 渲染每个块，对长的不变块进行折叠
    for (block_type, block) in blocks {
        if block_type == ChangeTag::Equal && block.len() > FOLD_THRESHOLD {
            // 显示头部
            for change in block.iter().take(5) {
                let _ = stdout.execute(SetForegroundColor(Theme::INFO));
                let _ = stdout.execute(Print(format!("    {change}")));
                let _ = stdout.execute(ResetColor);
            }
            
            // 折叠指示器
            let collapsed_count = block.len() - 10;
            let _ = stdout.execute(SetForegroundColor(Theme::INFO));
            let _ = stdout.execute(Print(format!("    ⋮ ... {} lines omitted ...\n", collapsed_count)));
            let _ = stdout.execute(ResetColor);
            
            // 显示尾部
            for change in block.iter().rev().take(5).rev() {
                let _ = stdout.execute(SetForegroundColor(Theme::INFO));
                let _ = stdout.execute(Print(format!("    {change}")));
                let _ = stdout.execute(ResetColor);
            }
        } else {
            // 正常渲染
            for change in block {
                match block_type {
                    ChangeTag::Delete => {
                        // 删除行：深红色背景，纯白色文字，无语法高亮
                        let _ = stdout.execute(SetBackgroundColor(Theme::DIFF_REMOVE_BG));
                        let _ = stdout.execute(SetForegroundColor(Theme::DIFF_REMOVE_FG));
                        let _ = stdout.execute(Print(format!("  - {change}")));
                        let _ = stdout.execute(ResetColor);
                    }
                    ChangeTag::Insert => {
                        // 新增行：深绿色背景，带语法高亮
                        let _ = stdout.execute(SetBackgroundColor(Theme::DIFF_ADD_BG));
                        print!("  + ");
                        
                        // 对新增内容进行语法高亮
                        let highlighted = highlight_line(&change.to_string());
                        for ((r, g, b), text) in highlighted {
                            let _ = stdout.execute(SetForegroundColor(Color::Rgb { r, g, b }));
                            let _ = stdout.execute(Print(text));
                        }
                        let _ = stdout.execute(ResetColor);
                    }
                    ChangeTag::Equal => {
                        let _ = stdout.execute(SetForegroundColor(Theme::INFO));
                        let _ = stdout.execute(Print(format!("    {change}")));
                        let _ = stdout.execute(ResetColor);
                    }
                }
            }
        }
    }

    let _ = stdout.flush();
}
