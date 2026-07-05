use std::io::Write;
use std::process::{Command, Stdio};

/// #567 S10：TUI 本地读取剪贴板图片
pub struct LocalImage {
    pub data: Vec<u8>,
    pub media_type: String,
}

/// #567 S10：TUI 本地读取剪贴板图片（macOS osascript）
pub async fn read_image() -> Result<LocalImage, String> {
    // macOS: 使用 pngpaste 或 osascript 读取剪贴板图片
    let output = Command::new("pngpaste")
        .arg("-")
        .output()
        .map_err(|e| format!("pngpaste 启动失败: {e}"))?;
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
    let data = std::fs::read(path).map_err(|e| format!("读取文件失败: {e}"))?;
    let media_type = match std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
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

    let mut child = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|err| format!("无法启动剪贴板命令 pbcopy：{err}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|err| format!("写入剪贴板失败：{err}"))?;
    }

    let status = child
        .wait()
        .map_err(|err| format!("等待剪贴板命令失败：{err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("剪贴板命令 pbcopy 退出失败：{status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_copy_text_empty_is_ok() {
        assert!(copy_text("").is_ok());
    }

    #[test]
    fn test_copy_text_command_failure_returns_chinese_error() {
        let err = copy_text("测试").err();

        if cfg!(target_os = "macos") {
            assert!(err.is_none() || err.unwrap().contains("剪贴板"));
        } else {
            assert!(err.unwrap().contains("无法启动剪贴板命令 pbcopy"));
        }
    }
}
