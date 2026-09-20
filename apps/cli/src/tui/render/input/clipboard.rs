use std::path::Path;

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

/// 把文本写入系统剪贴板；与读图共用同一个 `arboard` 边界。
pub fn copy_text(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }

    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| clipboard_error("连接系统剪贴板", error))?;
    clipboard
        .set_text(text)
        .map_err(|error| clipboard_error("写入剪贴板", error))
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
