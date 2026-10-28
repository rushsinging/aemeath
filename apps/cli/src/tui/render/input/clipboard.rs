use std::io::{self, Write};
use std::path::Path;

use super::constants::OSC52_MAX_ENCODED_BYTES;

/// 复制落点：系统剪贴板或终端 OSC 52，仅用于日志诊断——用户可见提示不区分两者。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyOutcome {
    /// 已写入系统剪贴板（系统剪贴板可用且 arboard 成功）。
    SystemClipboard,
    /// 已写入终端 OSC 52 序列（SSH / 无系统剪贴板，或系统剪贴板失败的降级路径）。
    TerminalOsc52,
}

/// TUI 本地读取到的图片：已编码字节 + 媒体类型。
pub struct LocalImage {
    pub data: Vec<u8>,
    pub media_type: String,
}

/// 读取系统剪贴板图片并编码为 PNG。
///
/// 统一走 `arboard` 的平台原生实现（macOS `NSPasteboard`；Linux X11 `x11rb` 与
/// Wayland data-control），**NEVER** 依赖 `pngpaste` / `osascript` / `xclip` 等外部命令，
/// 避免用户未安装对应工具时功能静默失效。
pub async fn read_image() -> Result<LocalImage, String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| clipboard_error("连接系统剪贴板", error))?;
    let image = clipboard
        .get_image()
        .map_err(|error| clipboard_error("读取剪贴板图片", error))?;
    let data = encode_png(image.width, image.height, image.bytes.as_ref())?;
    Ok(LocalImage {
        data,
        media_type: "image/png".to_string(),
    })
}

/// 复制文本：系统剪贴板可用时优先写入，失败或不可用（SSH / tty）时降级为
/// 终端 OSC 52 序列（`ESC]52;c;<base64>BEL`，由终端模拟器写入**本地**剪贴板）。
///
/// OSC 52 是 SSH 远程场景的标准通道；终端不支持时序列无害，**NEVER** 猜测终端能力。
pub fn copy_text(text: &str) -> Result<CopyOutcome, String> {
    if text.is_empty() {
        return Ok(CopyOutcome::SystemClipboard);
    }

    let use_system_clipboard = uses_system_clipboard(
        native_clipboard_target(),
        std::env::var("DISPLAY").ok().as_deref(),
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
    );
    let mut stdout = io::stdout();
    copy_text_via(
        text,
        use_system_clipboard,
        system_clipboard_set,
        &mut stdout,
    )
}

/// `copy_text` 的可测核心：系统剪贴板可用标志、写入器与终端 writer 均可注入。
fn copy_text_via(
    text: &str,
    use_system_clipboard: bool,
    set_system_clipboard: impl FnOnce(&str) -> Result<(), String>,
    writer: &mut impl Write,
) -> Result<CopyOutcome, String> {
    if use_system_clipboard {
        match set_system_clipboard(text) {
            Ok(()) => return Ok(CopyOutcome::SystemClipboard),
            Err(error) => crate::tui::log_warn!("系统剪贴板不可用，降级为终端 OSC 52：{error}"),
        }
    }

    write_osc52_clipboard(writer, text)?;
    Ok(CopyOutcome::TerminalOsc52)
}

/// 写入系统剪贴板（arboard 边界，与读图共用同一错误映射）。
fn system_clipboard_set(text: &str) -> Result<(), String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| clipboard_error("连接系统剪贴板", error))?;
    clipboard
        .set_text(text)
        .map_err(|error| clipboard_error("写入剪贴板", error))
}

/// 目标平台是否自带原生剪贴板实现（macOS `NSPasteboard` / Windows Win32）。
///
/// 这些平台**不通过** `DISPLAY` / `WAYLAND_DISPLAY` 表达图形会话，用环境变量判定会
/// 把本地非 SSH 会话误判为「无图形会话」并降级到 OSC 52。
const fn native_clipboard_target() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows"))
}

/// 是否应优先尝试系统剪贴板：原生剪贴板平台恒为真，其余平台按图形会话环境变量判定。
fn uses_system_clipboard(
    has_native_clipboard: bool,
    display: Option<&str>,
    wayland_display: Option<&str>,
) -> bool {
    has_native_clipboard || graphic_session_present(display, wayland_display)
}

/// 是否存在图形会话（`DISPLAY` / `WAYLAND_DISPLAY` 任一非空）。
fn graphic_session_present(display: Option<&str>, wayland_display: Option<&str>) -> bool {
    let present = |value: Option<&str>| value.is_some_and(|value| !value.trim().is_empty());
    present(display) || present(wayland_display)
}

fn encode_osc52_payload(text: &str) -> String {
    use base64::Engine;

    base64::engine::general_purpose::STANDARD.encode(text)
}

/// 将 OSC 52 序列写入终端 writer（生产入口为 `stdout`）。
fn write_osc52_clipboard(writer: &mut impl Write, text: &str) -> Result<(), String> {
    let encoded = encode_osc52_payload(text);
    if encoded.len() > OSC52_MAX_ENCODED_BYTES {
        return Err(format!(
            "待复制内容过长（编码后 {} 字节），超出终端 OSC 52 上限 {OSC52_MAX_ENCODED_BYTES}",
            encoded.len()
        ));
    }
    writer
        .write_all(format!("\x1b]52;c;{encoded}\x07").as_bytes())
        .and_then(|()| writer.flush())
        .map_err(|error| format!("写入终端 OSC 52 失败：{error}"))
}

/// TUI 本地处理图片文件
pub fn process_image_file(path: &str) -> Result<LocalImage, String> {
    let data = std::fs::read(path).map_err(|error| format!("无法读取图片文件 {path}：{error}"))?;
    let media_type = match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "image/png",
    };
    Ok(LocalImage {
        data,
        media_type: media_type.to_string(),
    })
}

/// RGBA8 像素 → PNG 字节。
///
/// 剪贴板 API 交出的是解码后的像素（无压缩），模型侧需要图片格式，
/// 因此统一编码为无损 PNG。
fn encode_png(width: usize, height: usize, rgba: &[u8]) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;

    let width = u32::try_from(width).map_err(|_| format!("剪贴板图片宽度超出范围: {width}"))?;
    let height = u32::try_from(height).map_err(|_| format!("剪贴板图片高度超出范围: {height}"))?;
    let expected_len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| format!("剪贴板图片尺寸溢出: {width}x{height}"))?;
    if rgba.len() != expected_len {
        return Err(format!(
            "剪贴板图片像素数据不完整: 期望 {expected_len} 字节，实际 {} 字节",
            rgba.len()
        ));
    }

    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded)
        .write_image(rgba, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|error| format!("剪贴板图片 PNG 编码失败: {error}"))?;
    Ok(encoded)
}

/// `arboard` 错误 → 中文用户消息，并带上操作语境。
fn clipboard_error(action: &str, error: arboard::Error) -> String {
    let reason = match &error {
        arboard::Error::ContentNotAvailable => "剪贴板中没有可用内容".to_string(),
        arboard::Error::ClipboardNotSupported => {
            "当前环境没有可用的系统剪贴板（无图形会话或缺少剪贴板服务）".to_string()
        }
        arboard::Error::ClipboardOccupied => "系统剪贴板被其他程序占用".to_string(),
        arboard::Error::ConversionFailure => "剪贴板内容无法转换为所需格式".to_string(),
        arboard::Error::Unknown { description } => format!("未知错误: {description}"),
        other => format!("未知错误: {other}"),
    };
    format!("{action}失败：{reason}")
}

#[cfg(test)]
#[path = "clipboard_tests.rs"]
mod tests;
