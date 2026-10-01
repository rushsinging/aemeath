use super::*;

const SAMPLE: &str = "@@ -1,3 +1,3 @@\n context\n-let a = 1;\n+let a = 2;";

#[test]
fn test_render_unified_diff_indents_and_colors_by_kind() {
    let lines = render_unified_diff(SAMPLE, None, 80);

    // 每行保留两空格缩进（修 #61 贴最左）。
    assert!(
        lines.iter().all(|line| line.plain.starts_with("  ")),
        "每行应保留 INDENT 缩进, got: {:?}",
        lines.iter().map(|l| l.plain.as_str()).collect::<Vec<_>>()
    );
    // hunk 头行存在且为 dim 色。
    let hunk = lines.iter().find(|l| l.plain.contains("@@")).unwrap();
    assert!(hunk
        .spans
        .iter()
        .any(|s| s.style.fg == Some(theme::TEXT_DIM)));
    // 删除行带 remove 语义色。
    let removed = lines.iter().find(|l| l.plain.contains("1;")).unwrap();
    assert!(
        removed
            .spans
            .iter()
            .any(|s| s.style.fg == Some(theme::DIFF_REMOVE_FG)),
        "删除行应带 DIFF_REMOVE_FG"
    );
    // 新增行带 add 语义色（前缀 + 符号）。
    let added = lines.iter().find(|l| l.plain.contains("2;")).unwrap();
    assert!(
        added
            .spans
            .iter()
            .any(|s| s.style.fg == Some(theme::DIFF_ADD_FG)),
        "新增行应带 DIFF_ADD_FG"
    );
}

#[test]
fn test_render_unified_diff_added_line_syntax_highlight() {
    let lines = render_unified_diff("+fn main() {}", Some("rs"), 80);
    let added = &lines[0];

    // 仍带 INDENT + 前缀 +，且因语法高亮产生多个 span。
    assert!(added.plain.starts_with("  +"));
    assert!(added.plain.contains("fn main"));
    assert!(
        added.spans.len() > 2,
        "语法高亮应产生多个 span, got {}",
        added.spans.len()
    );
}

#[test]
fn test_render_unified_diff_removed_is_plain_red_but_context_and_added_use_syntax_highlight() {
    let lines = render_unified_diff(" fn keep() {}\n-fn old() {}\n+fn new() {}", Some("rs"), 80);
    let context = lines.iter().find(|l| l.plain.contains("keep")).unwrap();
    let removed = lines.iter().find(|l| l.plain.contains("old")).unwrap();
    let added = lines.iter().find(|l| l.plain.contains("new")).unwrap();

    assert!(
        context.spans.len() > 2,
        "context 行正文应走 syntect 高亮，got: {:?}",
        context.spans
    );
    assert!(
        added.spans.len() > 2,
        "added 行正文应走 syntect 高亮，got: {:?}",
        added.spans
    );
    assert_eq!(removed.plain, "  -fn old() {}");
    assert_eq!(
        removed
            .spans
            .last()
            .map(|span| (span.content.as_ref(), span.style.fg)),
        Some(("fn old() {}", Some(theme::DIFF_REMOVE_FG))),
        "removed 行正文应为单个纯 DIFF_REMOVE_FG span，got: {:?}",
        removed.spans
    );
}

#[test]
fn test_render_unified_diff_infers_extension_from_file_headers_for_added_only() {
    let text = "diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1 @@\n-fn old() {}\n+fn new() {}";
    let lines = render_unified_diff(text, None, 80);
    let added = lines.iter().find(|l| l.plain.contains("new")).unwrap();
    let removed = lines.iter().find(|l| l.plain.contains("old")).unwrap();

    assert!(
        added.spans.len() > 2,
        "应从 diff 文件头推断 rs 并高亮新增行，got: {:?}",
        added.spans
    );
    assert_eq!(
        removed
            .spans
            .last()
            .map(|span| (span.content.as_ref(), span.style.fg)),
        Some(("fn old() {}", Some(theme::DIFF_REMOVE_FG))),
        "删除行即使能推断 rs 也应保持纯红，got: {:?}",
        removed.spans
    );
}

#[test]
fn test_render_unified_diff_meta_and_file_headers_not_treated_as_add_remove() {
    let text = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -0,0 +1 @@\n+new";
    let lines = render_unified_diff(text, None, 80);

    // `---`/`+++` 文件头按 Meta（TEXT_MUTED），不当成删除/新增语义色。
    let minus_header = lines.iter().find(|l| l.plain.contains("--- a/x")).unwrap();
    assert!(
        minus_header
            .spans
            .iter()
            .all(|s| s.style.fg != Some(theme::DIFF_REMOVE_FG)),
        "--- 文件头不应着删除色"
    );
    let plus_header = lines.iter().find(|l| l.plain.contains("+++ b/x")).unwrap();
    assert!(
        plus_header
            .spans
            .iter()
            .all(|s| s.style.fg != Some(theme::DIFF_ADD_FG)),
        "+++ 文件头不应着新增色"
    );
    // 真正的新增行才着新增色。
    let added = lines.iter().find(|l| l.plain.ends_with("new")).unwrap();
    assert!(added
        .spans
        .iter()
        .any(|s| s.style.fg == Some(theme::DIFF_ADD_FG)));
}

#[test]
fn test_render_unified_diff_empty_text() {
    let lines = render_unified_diff("", None, 80);
    assert!(lines.is_empty(), "空 diff 文本应产出 0 行");
}

#[test]
fn test_render_unified_diff_pure_context_no_hunk_header() {
    // 无 @@ hunk 头、纯 context：每行 INDENT + TEXT 色，不误判加减。
    let lines = render_unified_diff("just text\nmore text", None, 80);

    assert_eq!(lines.len(), 2);
    assert!(lines.iter().all(|l| l.plain.starts_with("  ")));
    assert!(lines.iter().all(|l| {
        l.spans.iter().all(|s| {
            s.style.fg != Some(theme::DIFF_ADD_FG) && s.style.fg != Some(theme::DIFF_REMOVE_FG)
        })
    }));
}
