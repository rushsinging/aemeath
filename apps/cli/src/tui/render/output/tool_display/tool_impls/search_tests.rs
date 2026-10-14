//! Grep 显示：typed result 汇总行 + 行级 matches。

use crate::tui::render::output::tool_display::lookup_display;

#[test]
fn grep_result_lines_include_summary_and_matches() {
    let display = lookup_display("Grep").expect("Grep display registered");
    let payload = serde_json::json!({
        "matches": [
            {
                "file_path": "src/a.rs",
                "line_number": 12,
                "line": "  match_me here"
            },
            {
                "file_path": "src/b.rs",
                "line_number": 3,
                "line": "match_me too"
            }
        ],
        "total_matches": 2,
        "shown": 2,
        "query": "match_me"
    });
    let lines = display
        .format_result_lines(Some(&payload))
        .expect("typed Grep result lines");

    assert_eq!(lines[0], "Found 2 matches");
    assert!(lines[1].contains("src/a.rs:12:"));
    assert!(lines[1].contains("match_me here"));
    assert!(lines[2].contains("src/b.rs:3:"));
}

#[test]
fn grep_result_lines_mark_truncation_in_summary() {
    let display = lookup_display("Grep").expect("Grep display registered");
    let payload = serde_json::json!({
        "matches": [
            {
                "file_path": "only.rs",
                "line_number": 1,
                "line": "hit"
            }
        ],
        "total_matches": 9,
        "shown": 1,
        "query": "hit"
    });
    let lines = display
        .format_result_lines(Some(&payload))
        .expect("typed Grep result lines");

    assert_eq!(lines[0], "Found 9 matches (showing 1)");
    assert_eq!(lines.len(), 2);
}

#[test]
fn grep_empty_typed_result_shows_no_hit_placeholder() {
    let display = lookup_display("Grep").expect("Grep display registered");
    let payload = serde_json::json!({
        "matches": [],
        "total_matches": 0,
        "shown": 0,
        "query": "missing"
    });
    let lines = display
        .format_result_lines(Some(&payload))
        .expect("empty Grep shape");
    assert_eq!(lines, vec!["（无命中）".to_string()]);
}

#[test]
fn grep_display_ignores_legacy_files_index_field() {
    // 旧索引字段若仍出现在 JSON，也不应再渲染 file · count。
    let display = lookup_display("Grep").expect("Grep display registered");
    let payload = serde_json::json!({
        "matches": [
            {
                "file_path": "x.rs",
                "line_number": 1,
                "line": "x"
            }
        ],
        "total_matches": 1,
        "shown": 1,
        "query": "x",
        "files": [{ "file_path": "old.txt", "match_count": 99 }],
        "total_files": 1
    });
    let lines = display.format_result_lines(Some(&payload)).expect("lines");
    assert_eq!(lines[0], "Found 1 matches");
    assert!(lines
        .iter()
        .all(|line| !line.contains("old.txt") && !line.contains('·')));
}
