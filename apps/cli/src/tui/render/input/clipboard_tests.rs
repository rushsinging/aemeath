use super::*;

#[test]
fn test_copy_text_empty_is_ok() {
    assert!(copy_text("").is_ok());
}

#[test]
fn test_copy_text_command_failure_returns_chinese_error() {
    let error = copy_text("测试").err();

    if cfg!(target_os = "macos") {
        assert!(error.is_none() || error.unwrap().contains("剪贴板"));
    } else {
        assert!(error
            .expect("non-macOS pbcopy should fail")
            .contains("无法启动剪贴板命令 pbcopy"));
    }
}

#[test]
fn decode_base64_output_accepts_wrapped_lines() {
    let decoded = decode_base64_output("iVBORw0KGgo=\n").expect("合法 base64 应解码成功");

    assert_eq!(decoded, b"\x89PNG\r\n\x1a\n");
}

#[test]
fn decode_base64_output_ignores_surrounding_whitespace() {
    let decoded = decode_base64_output("  iVBO\n  Rw0KGgo=  \n").expect("换行与缩进应被忽略");

    assert_eq!(decoded, b"\x89PNG\r\n\x1a\n");
}

#[test]
fn decode_base64_output_rejects_blank_output() {
    let error = decode_base64_output("  \n\t").expect_err("空输出必须失败");

    assert!(
        error.contains("剪贴板"),
        "错误消息应说明剪贴板语境，实际: {error}"
    );
}

#[test]
fn decode_base64_output_rejects_invalid_base64() {
    let error = decode_base64_output("not base64!!").expect_err("非法输入必须失败");

    assert!(
        error.contains("base64"),
        "错误消息应说明解码失败，实际: {error}"
    );
}

#[test]
fn clipboard_image_error_lists_both_strategies_and_install_hint() {
    let message = clipboard_image_error(
        "pngpaste 启动失败: No such file or directory",
        "osascript 失败: 剪贴板中没有 PNG 图片",
    );

    assert!(message.contains("pngpaste"), "实际: {message}");
    assert!(message.contains("osascript"), "实际: {message}");
    assert!(
        message.contains("brew install pngpaste"),
        "错误消息应给出可执行的安装建议，实际: {message}"
    );
}

/// 回归护栏：`dataForType("PNG")` 不是合法粘贴板类型（实测返回 nil 并抛 -2700），
/// 脚本必须使用 UTI `public.png`，并保留 legacy `PNGf` 兜底。
#[test]
fn clipboard_png_script_uses_valid_pasteboard_types() {
    assert!(
        CLIPBOARD_PNG_BASE64_SCRIPT.contains("public.png"),
        "必须使用 UTI public.png 读取 PNG：{CLIPBOARD_PNG_BASE64_SCRIPT}"
    );
    assert!(
        CLIPBOARD_PNG_BASE64_SCRIPT.contains("PNGf"),
        "必须保留 legacy PNGf 兜底：{CLIPBOARD_PNG_BASE64_SCRIPT}"
    );
    assert!(
        !CLIPBOARD_PNG_BASE64_SCRIPT.contains("dataForType(\"PNG\")"),
        "NEVER 回退到非法类型名 PNG"
    );
}
