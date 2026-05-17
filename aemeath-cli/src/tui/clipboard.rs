use std::io::Write;
use std::process::{Command, Stdio};

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
