use super::build_header_line;

#[test]
fn build_header_line_no_suffix() {
    let line = build_header_line("Read", "/foo/bar/baz.txt", "");
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(text, "Read /foo/bar/baz.txt");
}

#[test]
fn build_header_line_with_suffix() {
    let line = build_header_line("Read", "/foo/bar/baz.txt", " (5 lines)");
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(text, "Read /foo/bar/baz.txt (5 lines)");
}

#[test]
fn build_header_line_truncates_long_path() {
    let long =
        "/very/very/very/very/very/very/very/very/very/very/very/very/very/long/path/file.txt";
    let line = build_header_line("Read", long, "");
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.starts_with("Read "), "expected Read prefix: {text}");
    assert!(
        text.contains("..."),
        "expected ellipsis in long path: {text}"
    );
    assert!(
        text.len() < long.len() + 10,
        "long path should be truncated: {text}"
    );
}
