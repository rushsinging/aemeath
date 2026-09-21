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
        Ok(()) => {}
        Err(error) => assert!(
            error.contains("剪贴板"),
            "错误消息应为中文剪贴板语境，实际: {error}"
        ),
    }
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
