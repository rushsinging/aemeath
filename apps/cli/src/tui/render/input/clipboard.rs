use std::io::Write;
use std::process::{Command, Stdio};

/// #567 S10：TUI 本地读取剪贴板图片
pub struct LocalImage {
    pub data: Vec<u8>,
    pub media_type: String,
}

/// TUI 本地读取剪贴板图片。
///
/// 策略链：优先第三方 `pngpaste`（用户可选安装），失败后回退系统自带 `osascript`
/// 的 ObjC bridge 读取 `NSPasteboard` 的 PNG 数据；两者都失败时返回聚合错误。
pub async fn read_image() -> Result<LocalImage, String> {
    match read_image_with_pngpaste() {
        Ok(image) => Ok(image),
        Err(pngpaste_error) => match read_image_with_osascript() {
            Ok(image) => Ok(image),
            Err(osascript_error) => Err(clipboard_image_error(&pngpaste_error, &osascript_error)),
        },
    }
}

fn read_image_with_pngpaste() -> Result<LocalImage, String> {
    let mut command = Command::new("pngpaste");
    command.arg("-");
    utils::configure_std_noninteractive(&mut command)
        .map_err(|error| format!("pngpaste 进程隔离失败: {error}"))?;
    let output = command
        .output()
        .map_err(|error| format!("pngpaste 启动失败: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "pngpaste 失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(LocalImage {
        data: output.stdout,
        media_type: "image/png".to_string(),
    })
}

/// JXA 脚本：读系统剪贴板的 PNG 数据并输出标准 base64，避免落临时文件。
///
/// 类型名必须是 UTI `public.png`，旧系统/旧写入方用 legacy 四字符码 `PNGf` 兜底；
/// 实测 `dataForType("PNG")` 会返回 nil 并抛 -2700，**NEVER** 回退到该写法。
const CLIPBOARD_PNG_BASE64_SCRIPT: &str = r#"
ObjC.import("AppKit");
const pasteboard = $.NSPasteboard.generalPasteboard;
let pngData = pasteboard.dataForType("public.png");
if (pngData.isNil()) { pngData = pasteboard.dataForType("PNGf"); }
if (pngData.isNil()) { throw new Error("剪贴板中没有 PNG 图片"); }
pngData.base64EncodedStringWithOptions(0).js;
"#;

fn read_image_with_osascript() -> Result<LocalImage, String> {
    let mut command = Command::new("osascript");
    command
        .arg("-l")
        .arg("JavaScript")
        .arg("-e")
        .arg(CLIPBOARD_PNG_BASE64_SCRIPT);
    utils::configure_std_noninteractive(&mut command)
        .map_err(|error| format!("osascript 进程隔离失败: {error}"))?;
    let output = command
        .output()
        .map_err(|error| format!("osascript 启动失败: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "osascript 失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let data = decode_base64_output(&String::from_utf8_lossy(&output.stdout))?;
    Ok(LocalImage {
        data,
        media_type: "image/png".to_string(),
    })
}

/// 解码 `osascript` 输出的 base64，容忍换行与缩进；空输出与非 base64 内容给出中文错误。
fn decode_base64_output(stdout: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    let compact: String = stdout.split_whitespace().collect();
    if compact.is_empty() {
        return Err("剪贴板图片为空: osascript 未输出 PNG 数据".to_string());
    }
    base64::engine::general_purpose::STANDARD
        .decode(compact.as_bytes())
        .map_err(|error| format!("剪贴板 PNG base64 解码失败: {error}"))
}

/// 两条读取策略都失败时聚合原因，并给出可执行建议。
fn clipboard_image_error(pngpaste_error: &str, osascript_error: &str) -> String {
    format!(
        "读取剪贴板图片失败：pngpaste: {pngpaste_error}；osascript: {osascript_error}。\
         可执行 `brew install pngpaste` 安装第三方工具，或确认系统 osascript 可用"
    )
}

/// TUI 本地处理图片文件
pub fn process_image_file(path: &str) -> Result<LocalImage, String> {
    let data = std::fs::read(path).map_err(|error| format!("无法读取图片文件 {path}：{error}"))?;
    let media_type = match std::path::Path::new(path)
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

pub fn copy_text(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }

    let mut command = Command::new("pbcopy");
    command.stdin(Stdio::piped());
    utils::configure_std_noninteractive(&mut command)
        .map_err(|error| format!("无法隔离剪贴板命令 pbcopy：{error}"))?;
    let mut child = command
        .spawn()
        .map_err(|error| format!("无法启动剪贴板命令 pbcopy：{error}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|error| format!("写入剪贴板失败：{error}"))?;
    }

    let status = child
        .wait()
        .map_err(|error| format!("等待剪贴板命令失败：{error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("剪贴板命令 pbcopy 退出失败：{status}"))
    }
}

#[cfg(test)]
#[path = "clipboard_tests.rs"]
mod tests;
