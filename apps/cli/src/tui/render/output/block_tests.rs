use super::block::{scan_code_blocks, scan_table_blocks};
use crate::tui::output_area::{LineStyle, OutputLine};

fn md_line(content: &str) -> OutputLine {
    OutputLine {
        content: content.to_string(),
        style: LineStyle::Assistant,
        tool_id: None,
        spans: None,
    }
}

// ── Code Block Tests ──

#[test]
fn test_scan_code_blocks_fence_in_viewport() {
    let all = [
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

#[test]
fn test_scan_code_blocks_open_fence_scrolled_out() {
    let all = [
        md_line("```rust"),
        md_line("fn foo()"),
        md_line("```"),
        md_line("normal"),
    ];
    let vis: Vec<(usize, &OutputLine)> = all.iter().enumerate().skip(1).collect();
    let info = scan_code_blocks(all.iter(), &vis);
    assert!(
        info.code_block_lines.contains(&1),
        "code line 1 should be in code block"
    );
    assert!(
        info.code_block_lines.contains(&2),
        "closing fence should be in code block"
    );
    assert!(
        !info.code_block_lines.contains(&3),
        "line after block should NOT be code"
    );
    assert!(
        !info.code_lang_label.contains_key(&2),
        "closing fence should not have lang label"
    );
}

#[test]
fn test_scan_code_blocks_all_outside_viewport() {
    let all = [
        md_line("```"),
        md_line("code"),
        md_line("```"),
        md_line("visible"),
    ];
    let vis: Vec<(usize, &OutputLine)> = vec![(3, &all[3])];
    let info = scan_code_blocks(all.iter(), &vis);
    assert!(!info.code_block_lines.contains(&3));
}

#[test]
fn test_scan_code_blocks_inline_code_not_fence() {
    let all = [md_line("use `code` here")];
    let vis: Vec<(usize, &OutputLine)> = all.iter().enumerate().collect();
    let info = scan_code_blocks(all.iter(), &vis);
    assert!(info.code_block_lines.is_empty());
}

// ── Table Block Tests ──

#[test]
fn test_scan_table_blocks_in_viewport() {
    let all = [
        md_line("| a | b |"),
        md_line("|---|---|"),
        md_line("| 1 | 2 |"),
        md_line("normal"),
    ];
    let vis: Vec<(usize, &OutputLine)> = all.iter().enumerate().collect();
    let result = scan_table_blocks(all.iter(), &vis);
    assert!(result.contains(&0) && result.contains(&1) && result.contains(&2));
    assert!(!result.contains(&3));
}

#[test]
fn test_scan_table_blocks_header_scrolled_out() {
    let all = [
        md_line("before"),
        md_line("| Name | Age |"),
        md_line("|------|-----|"),
        md_line("| Alice | 30 |"),
        md_line("| Bob | 25 |"),
        md_line("after"),
    ];
    let vis: Vec<(usize, &OutputLine)> = all.iter().enumerate().skip(3).collect();
    let result = scan_table_blocks(all.iter(), &vis);
    assert!(
        result.contains(&3),
        "row 3 should be table (header scrolled out)"
    );
    assert!(
        result.contains(&4),
        "row 4 should be table (header scrolled out)"
    );
    assert!(!result.contains(&5), "row 5 should NOT be table");
}

#[test]
fn test_scan_table_blocks_no_separator_at_all() {
    let all = [md_line("| a | b |"), md_line("| c | d |")];
    let vis: Vec<(usize, &OutputLine)> = all.iter().enumerate().collect();
    let result = scan_table_blocks(all.iter(), &vis);
    assert!(
        result.is_empty(),
        "table without separator should not be recognized"
    );
}
