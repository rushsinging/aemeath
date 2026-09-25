//! 粘贴分派场景：远端图片链接按文本处理，本地图片引用经解码后加载，空白粘贴读剪贴板。

use super::super::testing::{ExpectedEffect, TuiScenarioHarness};
use crate::tui::effect::effect::Effect;

fn image_effects(harness: &TuiScenarioHarness) -> Vec<&Effect> {
    harness
        .effects()
        .iter()
        .filter(|effect| {
            matches!(
                effect,
                Effect::ReadClipboardImage | Effect::ProcessImageFile { .. }
            )
        })
        .collect()
}

#[test]
fn paste_http_image_url_inserts_text_without_image_effect() {
    let mut harness = TuiScenarioHarness::new(100, 30);

    harness.paste("https://example.com/assets/diagram.png");

    assert_eq!(
        harness.input_text(),
        "https://example.com/assets/diagram.png"
    );
    assert!(
        image_effects(&harness).is_empty(),
        "远端图片链接 MUST NOT 当作图片导入"
    );
    harness.assert_idle();
}

#[test]
fn paste_data_url_image_inserts_text_without_image_effect() {
    let mut harness = TuiScenarioHarness::new(100, 30);

    harness.paste("data:image/png;base64,iVBORw0KGgo=");

    assert_eq!(harness.input_text(), "data:image/png;base64,iVBORw0KGgo=");
    assert!(image_effects(&harness).is_empty());
    harness.assert_idle();
}

#[test]
fn paste_file_url_image_path_queues_decoded_local_path() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.expect_effect(ExpectedEffect::ProcessImageFile {
        path: "/Users/me/Pictures/diagram final.png".to_string(),
        fallback_text: "file:///Users/me/Pictures/diagram%20final.png".to_string(),
    });

    harness.paste("file:///Users/me/Pictures/diagram%20final.png");

    assert_eq!(harness.input_text(), "");
    harness.assert_idle();
}

#[test]
fn paste_backslash_escaped_local_path_queues_decoded_path() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    let pasted = "/Users/me/Library/Application\\ Support/LarkShell/stickers/a.gif";
    harness.expect_effect(ExpectedEffect::ProcessImageFile {
        path: "/Users/me/Library/Application Support/LarkShell/stickers/a.gif".to_string(),
        fallback_text: pasted.to_string(),
    });

    harness.paste(pasted);

    assert_eq!(harness.input_text(), "");
    harness.assert_idle();
}

#[test]
fn paste_non_image_local_path_inserts_text() {
    let mut harness = TuiScenarioHarness::new(100, 30);

    harness.paste("/Users/me/notes.txt");

    assert_eq!(harness.input_text(), "/Users/me/notes.txt");
    assert!(image_effects(&harness).is_empty());
    harness.assert_idle();
}

#[test]
fn paste_blank_requests_clipboard_image() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.expect_effect(ExpectedEffect::ReadClipboardImage);

    harness.paste("   ");

    assert_eq!(harness.input_text(), "");
    harness.assert_idle();
}

#[test]
fn paste_while_processing_queues_image_effect_with_original_text() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.chat.start_processing();
    harness.expect_effect(ExpectedEffect::ProcessImageFile {
        path: "/tmp/queued-fixture.png".to_string(),
        fallback_text: " /tmp/queued-fixture.png ".to_string(),
    });

    harness.paste(" /tmp/queued-fixture.png ");

    assert_eq!(harness.input_text(), "");
    harness.assert_idle();
}

#[test]
fn paste_while_processing_keeps_http_image_url_as_queued_text() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.chat.start_processing();

    harness.paste("https://example.com/assets/diagram.png");

    assert_eq!(
        harness.input_text(),
        "https://example.com/assets/diagram.png"
    );
    assert!(image_effects(&harness).is_empty());
    harness.assert_idle();
}

#[test]
fn paste_nonexistent_local_image_path_is_not_read_from_disk_during_update() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.expect_effect(ExpectedEffect::ProcessImageFile {
        path: "/definitely/missing/photo.png".to_string(),
        fallback_text: "/definitely/missing/photo.png".to_string(),
    });

    harness.paste("/definitely/missing/photo.png");

    assert_eq!(harness.input_text(), "");
    assert_eq!(image_effects(&harness).len(), 1);
    harness.assert_idle();
}
