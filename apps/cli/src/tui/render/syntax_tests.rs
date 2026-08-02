use super::*;

#[test]
fn test_extension_from_path() {
    assert_eq!(extension_from_path("src/lib.rs"), Some("rs"));
    assert_eq!(extension_from_path("foo.tsx"), Some("tsx"));
    assert_eq!(extension_from_path("Makefile"), None);
    assert_eq!(extension_from_path("dir/file"), None);
}

#[test]
fn test_language_by_extension() {
    let syntax = language_by_extension("rs");
    assert!(syntax.is_some(), "Rust syntax should be found");
}

#[test]
fn test_language_by_fence_info_maps_rust_name() {
    let by_name = language_by_fence_info("rust").expect("rust fence should resolve");
    let by_ext = language_by_extension("rs").expect("rs extension should resolve");
    assert_eq!(by_name.name, by_ext.name);
}

#[test]
fn test_language_by_fence_info_keeps_extension_path() {
    let by_info = language_by_fence_info("rs").expect("rs fence should resolve");
    let by_ext = language_by_extension("rs").expect("rs extension should resolve");
    assert_eq!(by_info.name, by_ext.name);
}

#[test]
fn test_language_by_fence_info_resolves_typescript() {
    let ts = language_by_fence_info("ts").expect("ts fence should resolve");
    assert_eq!(ts.name, "TypeScript");
    let by_name = language_by_fence_info("typescript").expect("typescript fence should resolve");
    assert_eq!(by_name.name, "TypeScript");
}

#[test]
fn test_language_by_fence_info_resolves_tsx() {
    let tsx = language_by_fence_info("tsx").expect("tsx fence should resolve");
    assert_eq!(tsx.name, "TypeScriptReact");
}

#[test]
fn test_language_by_fence_info_maps_ts_module_variants_to_typescript() {
    for info in ["mts", "cts"] {
        let syntax =
            language_by_fence_info(info).unwrap_or_else(|| panic!("{info} fence should resolve"));
        assert_eq!(syntax.name, "TypeScript", "{info} 应映射到 TypeScript");
    }
}

#[test]
fn test_highlight_line_with_typescript() {
    let syntax = language_by_fence_info("ts").expect("ts fence should resolve");
    let spans = highlight_line("import { readFile } from \"fs\";", Some(&syntax))
        .expect("TypeScript 行应可高亮");
    assert!(!spans.is_empty());
    let text: String = spans.iter().map(|span| span.text.as_str()).collect();
    assert!(text.contains("import"));
    let colors: std::collections::HashSet<_> = spans.iter().map(|span| span.color).collect();
    assert!(
        colors.len() > 1,
        "TypeScript 行应产生多色高亮，实际颜色数 {}",
        colors.len()
    );
}

#[test]
fn test_highlight_line_with_tsx() {
    let syntax = language_by_fence_info("tsx").expect("tsx fence should resolve");
    let spans = highlight_line("const el = <div className=\"app\" />;", Some(&syntax))
        .expect("TSX 行应可高亮");
    let colors: std::collections::HashSet<_> = spans.iter().map(|span| span.color).collect();
    assert!(
        colors.len() > 1,
        "TSX 行应产生多色高亮，实际颜色数 {}",
        colors.len()
    );
}

#[test]
fn test_highlight_line_uses_catppuccin_macchiato_keyword_color() {
    let syntax = language_by_extension("rs").unwrap();
    let spans = highlight_line("if true {", Some(&syntax)).unwrap();
    let keyword = spans.iter().find(|span| span.text == "if").unwrap();
    assert_eq!(keyword.color, crate::tui::render::theme::ACCENT_BRIGHT);
}

#[test]
fn test_highlight_line_with_rust() {
    let syntax = language_by_extension("rs").unwrap();
    let spans = highlight_line("fn main() {", Some(&syntax)).expect("Rust 行应可高亮");
    assert!(!spans.is_empty());
    let text: String = spans.iter().map(|span| span.text.as_str()).collect();
    assert!(text.contains("fn"));
}

#[test]
fn test_highlight_line_none_syntax() {
    assert!(highlight_line("hello", None).is_none());
}

#[test]
fn highlight_line_records_call_bytes_and_duration_when_capture_active() {
    let syntax = language_by_extension("rs").unwrap();
    let line = "fn main() { println!(\"你好\"); }";

    let (result, snapshot) =
        crate::tui::render::performance::capture(|| highlight_line(line, Some(&syntax)));

    assert!(result.is_some());
    assert_eq!(snapshot.syntax_highlight_calls, 1);
    assert_eq!(snapshot.syntax_highlight_input_bytes, line.len() as u64);
    assert!(snapshot.syntax_highlight_ns > 0);
}
