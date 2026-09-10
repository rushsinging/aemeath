use std::io::Write;
use std::process::{Command, Stdio};

/// #567 S10：TUI 本地读取剪贴板图片
pub struct LocalImage {
    pub data: Vec<u8>,
    pub media_type: String,
}

/// #567 S10：TUI 本地读取剪贴板图片（macOS osascript）
pub async fn read_image() -> Result<LocalImage, String> {
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
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(LocalImage {
        data: output.stdout,
        media_type: "image/png".to_string(),
    })
}

/// #567 S10：TUI 本地处理图片文件
pub fn process_image_file(path: &str) -> Result<LocalImage, String> {
    let data = std::fs::read(path).map_err(|error| format!("读取文件失败: {error}"))?;
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
