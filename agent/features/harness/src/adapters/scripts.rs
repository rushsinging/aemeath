//! scripts 组件（c7）：目录扫描、frontmatter 注释块解析与语法预检。
//!
//! 脚本落盘格式（v1）：可选 shebang 后跟随首个连续注释块，其中
//! `name:` / `description:` 键值即 frontmatter；语言由扩展名推断
//! （`.sh` / `.py`）。执行经 Bash 工具（天然热载，无独立执行通道）。
//! 引用可见性规则（防死件）：脚本应被 guardrails/tool_guidance/skill
//! 文本引用才对模型可见——v1 以 warn 提示，NEVER 阻塞合法落盘。

use crate::constants::{ERROR_DOMAIN, SCRIPT_MAX_BYTES};
use crate::domain::errors::HarnessError;
use std::collections::HashMap;
use std::path::Path;

/// 脚本语言（由扩展名推断）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptLang {
    Sh,
    Py,
}

impl ScriptLang {
    pub fn of_path(path: &Path) -> Option<Self> {
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("sh") => Some(Self::Sh),
            Some("py") => Some(Self::Py),
            _ => None,
        }
    }
}

/// 脚本 frontmatter（注释块键值）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScriptFrontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
}

/// 解析首个连续注释块中的 `name:` / `description:` 键值。
///
/// 跳过 shebang（首行 `#!`）；注释块以 `#` 行连续构成，
/// 首个非注释行（空行之外）终止扫描。
pub fn parse_script_frontmatter(source: &str) -> ScriptFrontmatter {
    let mut frontmatter = ScriptFrontmatter::default();
    let mut lines = source.lines().peekable();
    if let Some(first) = lines.peek() {
        if first.starts_with("#!") {
            lines.next();
        }
    }
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some(comment) = trimmed.strip_prefix('#') else {
            break;
        };
        let comment = comment.trim();
        if let Some(name) = comment.strip_prefix("name:") {
            frontmatter.name = Some(name.trim().to_string());
        } else if let Some(description) = comment.strip_prefix("description:") {
            frontmatter.description = Some(description.trim().to_string());
        }
    }
    frontmatter
}

/// 校验单个脚本（大小 + frontmatter 必填 + 语法预检）。
pub fn validate_script(path: &Path, source: &str) -> Result<(), HarnessError> {
    let byte_len = source.len() as u64;
    if byte_len > SCRIPT_MAX_BYTES {
        return Err(HarnessError::invalid(
            ERROR_DOMAIN,
            format!("脚本超过大小上限 {SCRIPT_MAX_BYTES} 字节"),
        ));
    }
    let lang = ScriptLang::of_path(path).ok_or_else(|| {
        HarnessError::invalid(
            ERROR_DOMAIN,
            format!("脚本扩展名不受支持（仅 .sh / .py）：{}", path.display()),
        )
    })?;
    let frontmatter = parse_script_frontmatter(source);
    if frontmatter.description.as_deref().is_none_or(str::is_empty) {
        return Err(HarnessError::invalid(
            ERROR_DOMAIN,
            "脚本 frontmatter 缺少非空 description 注释（# description: …）",
        ));
    }
    syntax_check(path, lang, source)
}

/// 语法预检：sh 经 `bash -n`；py 经 `python3 -m py_compile`。
/// 解释器不可用时跳过该校验并 warn（NEVER 因环境缺工具否决内容）。
fn syntax_check(path: &Path, lang: ScriptLang, source: &str) -> Result<(), HarnessError> {
    match lang {
        ScriptLang::Sh => {
            let mut command = std::process::Command::new("bash");
            command
                .arg("-n")
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped());
            let _ = utils::configure_std_noninteractive(&mut command);
            let output = command.spawn().and_then(|mut child| {
                use std::io::Write;
                if let Some(stdin) = child.stdin.as_mut() {
                    stdin.write_all(source.as_bytes())?;
                }
                child.wait_with_output()
            });
            match output {
                Ok(output) if output.status.success() => Ok(()),
                Ok(output) => Err(HarnessError::invalid(
                    ERROR_DOMAIN,
                    format!(
                        "bash -n 语法预检失败：{}",
                        String::from_utf8_lossy(&output.stderr).trim()
                    ),
                )),
                Err(error) => {
                    log::warn!(target: crate::LOG_TARGET, "bash 不可用，跳过 sh 语法预检：{error}");
                    Ok(())
                }
            }
        }
        ScriptLang::Py => {
            // py_compile 需要真实文件；写临时文件后删除
            let temp_dir = std::env::temp_dir();
            let temp_path = temp_dir.join(format!(
                "harness-script-check-{}-{}.py",
                std::process::id(),
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("x")
            ));
            std::fs::write(&temp_path, source).map_err(|error| {
                HarnessError::storage(ERROR_DOMAIN, format!("写临时文件失败：{error}"))
            })?;
            let mut command = std::process::Command::new("python3");
            command.arg("-m").arg("py_compile").arg(&temp_path);
            let _ = utils::configure_std_noninteractive(&mut command);
            let output = command.output();
            let _ = std::fs::remove_file(&temp_path);
            match output {
                Ok(output) if output.status.success() => Ok(()),
                Ok(output) => Err(HarnessError::invalid(
                    ERROR_DOMAIN,
                    format!(
                        "py_compile 语法预检失败：{}",
                        String::from_utf8_lossy(&output.stderr).trim()
                    ),
                )),
                Err(error) => {
                    log::warn!(target: crate::LOG_TARGET, "python3 不可用，跳过 py 语法预检：{error}");
                    Ok(())
                }
            }
        }
    }
}

/// 扫描 scripts 目录：返回脚本 stem → 路径映射（仅 .sh/.py）。
pub fn scan_scripts_directory(directory: &Path) -> HashMap<String, std::path::PathBuf> {
    let mut scripts = HashMap::new();
    let Ok(entries) = std::fs::read_dir(directory) else {
        return scripts;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if ScriptLang::of_path(&path).is_none() {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
            scripts.insert(stem.to_string(), path.clone());
        }
    }
    scripts
}
