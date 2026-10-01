use super::*;

fn edit_result(old: &str, new: &str) -> String {
    format!("replaced 1 occurrence(s) in src/lib.rs\n---DIFF---\n{old}\n---DIFF---\n{new}")
}

#[test]
fn test_parse_edit_diff_extracts_old_and_new() {
    let parsed = parse_edit_diff(&edit_result("let a = 1;", "let a = 2;")).unwrap();

    assert_eq!(parsed.old, "let a = 1;");
    assert_eq!(parsed.new, "let a = 2;");
}

#[test]
fn test_parse_edit_diff_extracts_real_start_line() {
    let result =
        "replaced 1 occurrence(s) in src/lib.rs\n---DIFF:LINE:42---\nold\n---DIFF:LINE:42---\nnew";
    let parsed = parse_edit_diff(result).unwrap();

    assert_eq!(parsed.old, "old");
    assert_eq!(parsed.new, "new");
    assert_eq!(parsed.start_line, 42);
}

#[test]
fn test_parse_edit_diff_legacy_marker_defaults_to_line_one() {
    let parsed = parse_edit_diff(&edit_result("old", "new")).unwrap();

    assert_eq!(parsed.start_line, 1);
}

#[test]
fn test_parse_edit_diff_multiline_preserves_inner_content() {
    let old = "fn f() {\n    1\n}";
    let new = "fn f() {\n    2\n}";
    let parsed = parse_edit_diff(&edit_result(old, new)).unwrap();

    assert_eq!(parsed.old, old);
    assert_eq!(parsed.new, new);
}

#[test]
fn test_parse_edit_diff_returns_none_without_marker() {
    assert!(parse_edit_diff("wrote 10 bytes to a.txt").is_none());
    assert!(parse_edit_diff("done: 3 matches").is_none());
}

#[test]
fn test_file_ext_for_edit_from_args_json() {
    // 正常路径：summary 是入参 JSON，含 file_path → 取扩展名。
    let summary = r#"{"file_path":"src/lib.rs","old_string":"a","new_string":"b"}"#;
    assert_eq!(
        file_ext_for_edit(Some(summary), "replaced 1 occurrence(s) in src/lib.rs").as_deref(),
        Some("rs")
    );
}

#[test]
fn test_file_ext_for_edit_falls_back_to_result_header() {
    // summary 缺失/无 file_path → 从结果 header 的 "in {path}" 解析。
    let result = "replaced 2 occurrence(s) in /a/b/main.py\n---DIFF---\nx\n---DIFF---\ny";
    assert_eq!(file_ext_for_edit(None, result).as_deref(), Some("py"));
    assert_eq!(file_ext_for_edit(Some("{}"), result).as_deref(), Some("py"));
}

#[test]
fn test_file_ext_for_edit_none_when_no_extension_or_no_source() {
    // 边界/错误：无扩展名、无 in 路径、非 JSON summary 均返回 None。
    assert!(file_ext_for_edit(Some("not json"), "no path here").is_none());
    assert!(file_ext_for_edit(Some(r#"{"file_path":"Makefile"}"#), "done").is_none());
    assert!(file_ext_for_edit(None, "replaced 1 occurrence(s) in Dockerfile").is_none());
}

#[test]
fn test_render_edit_diff_emits_line_numbers_signs_indent_and_color() {
    let result = edit_result("let a = 1;", "let a = 2;");
    let summary = r#"{"file_path":"src/lib.rs"}"#;
    let lines = render_edit_diff(None, Some(summary), &result, 80).unwrap();

    let plains: Vec<&str> = lines.iter().map(|line| line.plain.as_str()).collect();

    // 删除行带 "- " 与原文本，新增行带 "+ " 与新文本（加减语义）。
    assert!(
        plains.iter().any(|p| p.contains("- ") && p.contains("1;")),
        "应含删除行，got: {plains:?}"
    );
    assert!(
        plains.iter().any(|p| p.contains("+ ") && p.contains("2;")),
        "应含新增行，got: {plains:?}"
    );
    // 块缩进由 gutter 注入（#60/#63）：diff 行不再自拼行首 INDENT，删除行从行号区起。
    let del = lines
        .iter()
        .find(|line| line.plain.contains("- ") && line.plain.contains("1;"))
        .expect("删除行存在");
    assert!(
        !del.plain.starts_with("  "),
        "删除行不应自拼行首块缩进，got: {:?}",
        del.plain
    );
    // 至少一行带前景色 span（语义色 / 语法高亮）。
    assert!(
        lines
            .iter()
            .any(|line| line.spans.iter().any(|span| span.style.fg.is_some())),
        "应有带前景色的 span"
    );
}

#[test]
fn test_render_edit_diff_none_for_non_diff_result() {
    assert!(render_edit_diff(None, Some(r#"{"file_path":"a.rs"}"#), "120 lines", 80).is_none());
}

#[test]
fn test_render_edit_diff_does_not_contain_raw_marker() {
    let result = edit_result("a", "b");
    let lines = render_edit_diff(None, Some(r#"{"file_path":"x.rs"}"#), &result, 80).unwrap();

    assert!(
        lines
            .iter()
            .all(|line| !line.plain.contains(LEGACY_DIFF_MARKER)),
        "渲染后不应残留原始标记"
    );
}

#[test]
fn test_render_edit_diff_real_bare_title_summary_drives_syntax_highlight() {
    // M1 回归：运行时 title 是裸 "Edit"（无括号路径），ext 必须从 summary 的
    // file_path 推断。注入真实 summary，断言 Rust 语法高亮被激活
    //（新增行因高亮产生 >2 个 span，而非单色 1 个内容 span）。
    // header 无可解析扩展名（Dockerfile），确保基线不会经 header 回退拿到 ext。
    let result = "edited Dockerfile\n---DIFF---\nfn old() {}\n---DIFF---\nfn new() {}".to_string();
    let summary = r#"{"file_path":"src/lib.rs","old_string":"fn old() {}"}"#;

    let with_ext = render_edit_diff(None, Some(summary), &result, 80).unwrap();
    let without_ext = render_edit_diff(None, Some("{}"), &result, 80).unwrap();

    // 新增行（含 "new"）。
    let added_with = with_ext
        .iter()
        .find(|l| l.plain.contains("new"))
        .expect("新增行存在");
    let added_without = without_ext
        .iter()
        .find(|l| l.plain.contains("new"))
        .expect("新增行存在");

    // 有 ext → 语法高亮产生更多 span；无 ext → 单色少 span。
    assert!(
        added_with.spans.len() > added_without.spans.len(),
        "summary 含 file_path 时应激活语法高亮（更多 span）: with={} without={}",
        added_with.spans.len(),
        added_without.spans.len()
    );
}

// ── #546：结构化 data 通道测试 ──────────────────────────────────

#[test]
fn test_edit_diff_from_data_extracts_structured_fields() {
    let data = serde_json::json!({
        "file_path": "src/lib.rs",
        "replacements_made": 1,
        "dry_run": false,
        "old": "let a = 1;",
        "new": "let a = 2;",
        "start_line": 5
    });
    let parsed = edit_diff_from_data(Some(&data)).unwrap();

    assert_eq!(parsed.old, "let a = 1;");
    assert_eq!(parsed.new, "let a = 2;");
    assert_eq!(parsed.start_line, 5);
}

#[test]
fn test_edit_diff_from_data_returns_none_when_missing_fields() {
    // 缺 old/new/start_line → None（回退到 parse_edit_diff）
    let data = serde_json::json!({"file_path": "a.rs", "replacements_made": 1});
    assert!(edit_diff_from_data(Some(&data)).is_none());
    assert!(edit_diff_from_data(None).is_none());
}

#[test]
fn test_render_edit_diff_prefers_data_over_text() {
    // data 含结构化 diff 时优先走 data，即使 text 不含 ---DIFF--- 标记也能渲染。
    let data = serde_json::json!({
        "old": "let a = 1;",
        "new": "let a = 2;",
        "start_line": 1
    });
    let summary = r#"{"file_path":"src/lib.rs"}"#;
    let lines = render_edit_diff(Some(&data), Some(summary), "Replaced 1 occurrence(s)", 80)
        .expect("data 通道应成功渲染 diff");

    let plains: Vec<&str> = lines.iter().map(|line| line.plain.as_str()).collect();
    assert!(
        plains.iter().any(|p| p.contains("- ") && p.contains("1;")),
        "应含删除行，got: {plains:?}"
    );
    assert!(
        plains.iter().any(|p| p.contains("+ ") && p.contains("2;")),
        "应含新增行，got: {plains:?}"
    );
}

#[test]
fn test_render_edit_diff_falls_back_to_text_when_data_absent() {
    // data 为 None（历史 session）时回退到 parse_edit_diff。
    let result = edit_result("let a = 1;", "let a = 2;");
    let lines = render_edit_diff(None, Some(r#"{"file_path":"src/lib.rs"}"#), &result, 80)
        .expect("回退 parse_edit_diff 应成功渲染");

    assert!(
        lines.iter().any(|l| l.plain.contains("1;")),
        "回退路径也应正确渲染 diff"
    );
}

fn numbered_source(lines: usize, changed: bool) -> String {
    (0..lines)
        .map(|index| {
            if changed && index == lines / 2 {
                format!("fn item_{index}() {{ new_value(); }}")
            } else {
                format!("fn item_{index}() {{ old_value(); }}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn render_budget_uses_measured_threshold_boundaries() {
    let highlighted = numbered_source(20_000, false);
    let plain = numbered_source(20_001, false);
    let extreme = numbered_source(100_001, false);

    assert_eq!(
        DiffRenderBudget::classify(&highlighted, &highlighted).mode,
        DiffRenderMode::Highlighted
    );
    assert_eq!(
        DiffRenderBudget::classify(&plain, &plain).mode,
        DiffRenderMode::Plain
    );
    assert_eq!(
        DiffRenderBudget::classify(&extreme, &extreme).mode,
        DiffRenderMode::HeadTailPlain
    );
}

#[test]
fn edit_within_highlight_budget_keeps_syntax_highlighting() {
    let old = numbered_source(20, false);
    let new = numbered_source(20, true);
    let data = serde_json::json!({"old": old, "new": new, "start_line": 100});

    let (lines, snapshot) = crate::tui::render::performance::capture(|| {
        render_edit_diff(
            Some(&data),
            Some(r#"{"file_path":"src/lib.rs"}"#),
            "edited src/lib.rs",
            80,
        )
        .unwrap()
    });

    assert!(snapshot.syntax_highlight_calls > 0);
    assert!(lines.iter().all(|line| !line.plain.contains("省略")));
}

#[test]
fn edit_over_highlight_line_budget_keeps_full_diff_without_syntax_highlighting() {
    let old = numbered_source(20_001, false);
    let new = numbered_source(20_001, true);
    let data = serde_json::json!({"old": old, "new": new, "start_line": 1});

    let (lines, snapshot) = crate::tui::render::performance::capture(|| {
        render_edit_diff(
            Some(&data),
            Some(r#"{"file_path":"src/lib.rs"}"#),
            "edited src/lib.rs",
            80,
        )
        .unwrap()
    });

    assert_eq!(snapshot.syntax_highlight_calls, 0);
    assert_eq!(lines.len(), 20_002);
    assert!(lines.iter().all(|line| !line.plain.contains("省略")));
    assert!(lines.iter().any(|line| line.plain.contains("+ ")));
    assert!(lines.iter().any(|line| line.plain.contains("- ")));
}

#[test]
fn extreme_edit_keeps_head_and_tail_with_real_line_numbers() {
    let old = numbered_source(100_001, false);
    let new = numbered_source(100_001, true);
    let data = serde_json::json!({"old": old, "new": new, "start_line": 42});

    let (lines, snapshot) = crate::tui::render::performance::capture(|| {
        render_edit_diff(
            Some(&data),
            Some(r#"{"file_path":"src/lib.rs"}"#),
            "edited src/lib.rs",
            80,
        )
        .unwrap()
    });

    assert_eq!(snapshot.syntax_highlight_calls, 0);
    assert!(lines.len() <= 503, "首尾窗口必须有硬上限: {}", lines.len());
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.plain.contains("省略"))
            .count(),
        1
    );
    assert!(lines.iter().any(|line| line.plain.contains("item_0")));
    assert!(lines.iter().any(|line| line.plain.contains("item_100000")));
    assert!(
        lines
            .iter()
            .any(|line| line.plain.trim_start().starts_with("100042")),
        "尾部必须保留原文件真实行号"
    );
}

#[test]
fn line_over_render_limit_is_utf8_safely_truncated() {
    let long = format!("{}尾", "界".repeat((4 * 1024 * 1024) / 3 + 10));
    let data = serde_json::json!({"old": long, "new": "短行", "start_line": 7});

    let (lines, snapshot) = crate::tui::render::performance::capture(|| {
        render_edit_diff(
            Some(&data),
            Some(r#"{"file_path":"src/lib.rs"}"#),
            "edited src/lib.rs",
            80,
        )
        .unwrap()
    });

    assert_eq!(snapshot.syntax_highlight_calls, 0);
    let removed = lines
        .iter()
        .find(|line| line.plain.contains("- "))
        .expect("删除行存在");
    assert!(removed.plain.len() < 4 * 1024 * 1024);
    assert!(removed.plain.contains("单行已截断"));
    assert!(std::str::from_utf8(removed.plain.as_bytes()).is_ok());
}
