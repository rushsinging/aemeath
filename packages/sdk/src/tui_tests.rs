//! `tui.rs` 的 SDK 边界契约测试。
//!
//! 覆盖两块稳定契约：图片视图字段完整性，以及终端粘贴文本 → `PasteKind` 的判定。

use super::*;

#[test]
fn clipboard_image_view_keeps_render_fields() {
    let image = ClipboardImageView {
        base64: "abc".to_string(),
        media_type: "image/png".to_string(),
        final_size: 3,
        display_path: Some("/tmp/a.png".to_string()),
        width: Some(10),
        height: Some(20),
    };

    assert_eq!(image.base64, "abc");
    assert_eq!(image.media_type, "image/png");
    assert_eq!(image.final_size, 3);
    assert_eq!(image.display_path.as_deref(), Some("/tmp/a.png"));
}

#[test]
fn memory_config_view_default_is_disabled_safe() {
    let config = MemoryConfigView::default();

    assert!(!config.enabled);
    assert_eq!(config.max_entries, 0);
    assert!(!config.reflection.enabled);
}

#[test]
fn classify_paste_treats_blank_text_as_empty() {
    assert_eq!(classify_paste("   \n\t "), PasteKind::Empty);
}

#[test]
fn classify_paste_treats_plain_text_as_text() {
    assert_eq!(classify_paste("帮我看看这个函数"), PasteKind::Text);
}

#[test]
fn classify_paste_keeps_unquoted_local_image_path() {
    assert_eq!(
        classify_paste("/tmp/aemeath-shot.png"),
        PasteKind::LocalImageFile(PathBuf::from("/tmp/aemeath-shot.png"))
    );
}

#[test]
fn classify_paste_treats_non_image_local_path_as_text() {
    assert_eq!(classify_paste("/tmp/aemeath-notes.txt"), PasteKind::Text);
}

#[test]
fn classify_paste_treats_http_image_url_as_text() {
    assert_eq!(
        classify_paste("http://example.com/assets/diagram.png"),
        PasteKind::Text
    );
}

#[test]
fn classify_paste_treats_https_image_url_with_query_as_text() {
    assert_eq!(
        classify_paste("https://example.com/assets/diagram.png?token=abc#frag"),
        PasteKind::Text
    );
}

#[test]
fn classify_paste_treats_data_url_as_text() {
    assert_eq!(
        classify_paste("data:image/png;base64,iVBORw0KGgo="),
        PasteKind::Text
    );
}

#[test]
fn classify_paste_treats_other_scheme_image_url_as_text() {
    assert_eq!(
        classify_paste("ftp://example.com/assets/diagram.webp"),
        PasteKind::Text
    );
}

#[test]
fn classify_paste_decodes_backslash_escaped_space_in_gif_path() {
    let pasted = "/Users/me/Library/Application\\ Support/LarkShell/stickers/a.gif";

    assert_eq!(
        classify_paste(pasted),
        PasteKind::LocalImageFile(PathBuf::from(
            "/Users/me/Library/Application Support/LarkShell/stickers/a.gif"
        ))
    );
}

#[test]
fn classify_paste_decodes_double_backslash_as_literal_backslash() {
    let pasted = "/tmp/aemeath\\\\shot.png";

    assert_eq!(
        classify_paste(pasted),
        PasteKind::LocalImageFile(PathBuf::from("/tmp/aemeath\\shot.png"))
    );
}

#[test]
fn classify_paste_treats_escaped_non_image_path_as_text() {
    let pasted = "/Users/me/My\\ Notes.txt";

    assert_eq!(classify_paste(pasted), PasteKind::Text);
}

#[test]
fn classify_paste_decodes_file_url_percent_encoding() {
    assert_eq!(
        classify_paste("file:///Users/me/Pictures/diagram%20final.png"),
        PasteKind::LocalImageFile(PathBuf::from("/Users/me/Pictures/diagram final.png"))
    );
}

#[test]
fn classify_paste_decodes_file_url_with_localhost_host() {
    assert_eq!(
        classify_paste("file://localhost/tmp/shot.jpeg"),
        PasteKind::LocalImageFile(PathBuf::from("/tmp/shot.jpeg"))
    );
}

#[test]
fn classify_paste_ignores_query_suffix_in_file_url_extension() {
    assert_eq!(
        classify_paste("file:///tmp/shot.png?download=1"),
        PasteKind::LocalImageFile(PathBuf::from("/tmp/shot.png"))
    );
}

#[test]
fn classify_paste_treats_file_url_without_image_extension_as_text() {
    assert_eq!(classify_paste("file:///tmp/report.pdf"), PasteKind::Text);
}

#[test]
fn classify_paste_treats_missing_scheme_separator_as_image_path() {
    // `data:` 之外的其它值若不带 `//`，仍按本地路径语义处理。
    assert_eq!(
        classify_paste("/tmp/shot.PNG"),
        PasteKind::LocalImageFile(PathBuf::from("/tmp/shot.PNG"))
    );
}
