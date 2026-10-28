use super::*;
use tempfile::tempdir;

#[test]
fn copy_text_empty_is_ok() {
    assert!(copy_text("").is_ok());
}

/// 剪贴板可用性依赖运行环境（CI 无显示会话）：成功或无结果都必须给出中文剪贴板语境。
#[test]
fn copy_text_reports_chinese_error_or_succeeds() {
    match copy_text("测试") {
        Ok(_) => {}
        Err(error) => assert!(
            error.contains("剪贴板"),
            "错误消息应为中文剪贴板语境，实际: {error}"
        ),
    }
}

// ---- 系统剪贴板可用性判定 ----

/// macOS / Windows 自带原生剪贴板实现，不通过 `DISPLAY` / `WAYLAND_DISPLAY` 表达图形会话，
/// 空环境变量也 MUST 判为系统剪贴板可用（否则本地非 SSH 会话被误判降级到 OSC 52）。
#[test]
fn uses_system_clipboard_trusts_native_clipboard_platforms() {
    assert!(uses_system_clipboard(true, None, None));
    assert!(uses_system_clipboard(true, Some("   "), Some("")));
}

/// 其余平台（Linux 等）仍按图形会话环境变量判定，SSH / tty（两者皆空）降级 OSC 52。
#[test]
fn uses_system_clipboard_requires_graphic_session_elsewhere() {
    assert!(!uses_system_clipboard(false, None, None));
    assert!(!uses_system_clipboard(false, Some("   "), Some("")));
    assert!(uses_system_clipboard(false, Some(":0"), None));
    assert!(uses_system_clipboard(false, None, Some("wayland-0")));
}

/// 当前测试平台的原生剪贴板判定必须与 `arboard` 支持面一致：
/// macOS / Windows 恒真（不需要 `DISPLAY`），其余平台跟随环境变量。
#[test]
fn native_clipboard_target_matches_current_platform_capability() {
    let expected = cfg!(any(target_os = "macos", target_os = "windows"));
    assert_eq!(native_clipboard_target(), expected);
}

// ---- OSC 52 终端剪贴板（SSH / 无图形会话路径） ----

#[test]
fn graphic_session_present_detects_display_variants() {
    assert!(graphic_session_present(Some(":0"), None));
    assert!(graphic_session_present(None, Some("wayland-0")));
    assert!(graphic_session_present(Some(":0"), Some("wayland-0")));
    assert!(!graphic_session_present(None, None));
    assert!(!graphic_session_present(Some("   "), Some("")));
}

#[test]
fn write_osc52_wraps_base64_payload() {
    let mut buffer = Vec::new();

    write_osc52_clipboard(&mut buffer, "hello").expect("写入成功");

    assert_eq!(buffer, b"\x1b]52;c;aGVsbG8=\x07");
}

/// 内容中的控制字符 / Unicode 经 base64 编码后不得破坏序列边界。
#[test]
fn write_osc52_round_trips_unicode_and_control_characters() {
    use base64::Engine;

    let text = "中文\t复制\n🙂\x1b";
    let mut buffer = Vec::new();
    write_osc52_clipboard(&mut buffer, text).expect("写入成功");
    let sequence = String::from_utf8(buffer).expect("序列是 utf8");

    let encoded = sequence
        .strip_prefix("\x1b]52;c;")
        .and_then(|body| body.strip_suffix('\x07'))
        .expect("序列必须为 OSC 52 包裹格式");
    let decoded = String::from_utf8(
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("payload 必须是合法 base64"),
    )
    .expect("解码结果必须是 utf8");
    assert_eq!(decoded, text);
    assert_eq!(
        sequence.matches('\x07').count(),
        1,
        "BEL 仅允许作为终止符出现: {sequence:?}"
    );
}

/// 无图形会话（SSH / tty）：NEVER 连接系统剪贴板，直接走 OSC 52。
#[test]
fn copy_text_uses_terminal_osc52_when_no_graphic_session() {
    let mut buffer = Vec::new();

    let outcome = copy_text_via(
        "选中文本",
        false,
        |_| panic!("无图形会话时 NEVER 连接系统剪贴板"),
        &mut buffer,
    )
    .expect("OSC 52 写入成功");

    assert!(matches!(outcome, CopyOutcome::TerminalOsc52));
    assert!(!buffer.is_empty(), "必须写出 OSC 52 序列");
}

#[test]
fn copy_text_prefers_system_clipboard_when_available() {
    let mut buffer = Vec::new();
    let mut received = String::new();

    let outcome = copy_text_via(
        "文本",
        true,
        |text| {
            received = text.to_string();
            Ok(())
        },
        &mut buffer,
    )
    .expect("系统剪贴板写入成功");

    assert!(matches!(outcome, CopyOutcome::SystemClipboard));
    assert_eq!(received, "文本");
    assert!(buffer.is_empty(), "系统剪贴板成功时 NEVER 写 OSC 52");
}

/// 有图形会话但系统剪贴板失败（X11 超时等）：降级为 OSC 52。
#[test]
fn copy_text_falls_back_to_osc52_when_system_clipboard_fails() {
    let mut buffer = Vec::new();

    let outcome = copy_text_via(
        "文本",
        true,
        |_| Err("连接系统剪贴板失败：x11 server connection timeout".to_string()),
        &mut buffer,
    )
    .expect("降级 OSC 52 成功");

    assert!(matches!(outcome, CopyOutcome::TerminalOsc52));
    assert!(!buffer.is_empty(), "必须写出 OSC 52 序列");
}

#[test]
fn copy_text_rejects_oversized_osc52_payload() {
    let mut buffer = Vec::new();
    let oversized = "a".repeat(OSC52_MAX_ENCODED_BYTES);

    let error = copy_text_via(&oversized, false, |_| Ok(()), &mut buffer)
        .expect_err("超出终端 OSC 52 上限必须失败");

    assert!(error.contains("过长"), "错误应说明内容过长，实际: {error}");
}

#[test]
fn encode_png_round_trips_rgba_pixels() {
    let pixels: Vec<u8> = vec![
        255, 0, 0, 255, //
        1, 2, 3, 255, //
        10, 20, 30, 128, //
        40, 50, 60, 0,
    ];

    let encoded = encode_png(2, 2, &pixels).expect("RGBA 像素应编码成功");

    let decoded = image::load_from_memory_with_format(&encoded, image::ImageFormat::Png)
        .expect("编码结果应是合法 PNG")
        .to_rgba8();
    assert_eq!(decoded.width(), 2);
    assert_eq!(decoded.height(), 2);
    assert_eq!(decoded.as_raw(), &pixels);
}

#[test]
fn encode_png_rejects_incomplete_pixel_buffer() {
    let error = encode_png(2, 2, &[0, 0, 0]).expect_err("像素不足必须失败");

    assert!(
        error.contains("像素"),
        "错误应说明像素数据不完整，实际: {error}"
    );
}

#[test]
fn encode_png_rejects_oversized_dimension() {
    let error = encode_png(usize::MAX, 2, &[]).expect_err("尺寸溢出必须失败");

    assert!(
        error.contains("尺寸") || error.contains("宽"),
        "实际: {error}"
    );
}

#[test]
fn clipboard_error_maps_content_missing_to_chinese_message() {
    let message = clipboard_error("读取剪贴板图片", arboard::Error::ContentNotAvailable);

    assert!(message.contains("读取剪贴板图片"), "实际: {message}");
    assert!(message.contains("剪贴板中没有可用内容"), "实际: {message}");
}

#[test]
fn clipboard_error_maps_unsupported_environment_to_chinese_message() {
    let message = clipboard_error("连接系统剪贴板", arboard::Error::ClipboardNotSupported);

    assert!(message.contains("没有可用的系统剪贴板"), "实际: {message}");
}

#[test]
fn clipboard_error_maps_occupied_clipboard_to_chinese_message() {
    let message = clipboard_error("写入剪贴板", arboard::Error::ClipboardOccupied);

    assert!(message.contains("占用"), "实际: {message}");
}

#[test]
fn process_image_file_infers_png_media_type_and_keeps_bytes() {
    let directory = tempdir().expect("临时目录");
    let path = directory.path().join("shot.png");
    std::fs::write(&path, b"png-bytes").expect("写入 fixture");

    let image = process_image_file(path.to_str().expect("utf8 路径")).expect("读取成功");

    assert_eq!(image.media_type, "image/png");
    assert_eq!(image.data, b"png-bytes");
}

/// 真机往返：原生剪贴板平台上 `copy_text` MUST 写系统剪贴板（而非降级 OSC 52），内容可读回。
/// 会覆盖当前剪贴板文本，因此跑完恢复原内容；默认忽略（CI 无剪贴板环境）。
#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
#[ignore = "需要真实系统剪贴板：本地手动 cargo test -p cli -- --ignored 运行"]
fn copy_text_uses_system_clipboard_on_native_platforms() {
    let previous = {
        let mut clipboard = arboard::Clipboard::new().expect("连接系统剪贴板");
        clipboard.get_text().ok()
    };

    let text = "aemeath clipboard smoke test";
    let outcome = copy_text(text).expect("复制成功");
    assert!(
        matches!(outcome, CopyOutcome::SystemClipboard),
        "原生剪贴板平台 MUST 写系统剪贴板，实际降级为 OSC 52"
    );

    let mut clipboard = arboard::Clipboard::new().expect("连接系统剪贴板");
    assert_eq!(clipboard.get_text().expect("读回剪贴板文本"), text);

    match previous {
        Some(previous) => clipboard.set_text(previous).expect("恢复原剪贴板文本"),
        None => {
            clipboard.clear().ok();
        }
    }
}

/// 真机往返：写入系统剪贴板 → 读回 → 校验像素。
/// 依赖真实剪贴板与会话环境，CI 无显示会话时必然失败，因此默认忽略。
#[test]
#[ignore = "需要真实系统剪贴板：本地手动 cargo test -p cli -- --ignored 运行"]
fn clipboard_image_round_trip_restores_written_pixels() {
    let pixels: Vec<u8> = vec![
        9, 8, 7, 255, //
        6, 5, 4, 255, //
        3, 2, 1, 255, //
        0, 0, 0, 255, //
        255, 255, 255, 255, //
        128, 64, 32, 255, //
        1, 1, 1, 255, //
        2, 2, 2, 255,
    ];
    let mut clipboard = arboard::Clipboard::new().expect("连接系统剪贴板");
    clipboard
        .set_image(arboard::ImageData {
            width: 4,
            height: 2,
            bytes: pixels.clone().into(),
        })
        .expect("写入剪贴板图片");

    let image = futures::executor::block_on(read_image()).expect("读回剪贴板图片");

    let decoded = image::load_from_memory_with_format(&image.data, image::ImageFormat::Png)
        .expect("读回结果应是合法 PNG")
        .to_rgba8();
    assert_eq!(decoded.width(), 4);
    assert_eq!(decoded.height(), 2);
    assert_eq!(decoded.as_raw(), &pixels);
    assert_eq!(image.media_type, "image/png");
}
