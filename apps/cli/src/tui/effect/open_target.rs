//! 打开目标解析与本机存在性校验（#2196）。
//!
//! markdown 渲染出的 link URL 可能是外部 URL，也可能是相对/绝对本地路径。
//! 旧实现直接把相对路径拼到「第一个可用的工作区基准」上就交给系统打开器，
//! 既不校验文件是否存在，也不看子进程退出码，导致路径错时仍然提示「已打开」。
//!
//! 本模块只做纯解析：把 link URL 归类成外部 URL / 已存在本地路径，并把所有
//! 候选基准回传给调用方用于诊断。真正的进程派发与结果回显在 executor。

use std::path::{Path, PathBuf};

use crate::tui::render::output::markdown::looks_like_file_path;

/// link URL 解析结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum OpenTarget {
    /// 外部 URL，直接交给系统打开器。
    Url(String),
    /// 已通过存在性校验的本地路径。
    Local {
        /// 用户点击时看到的原始文本（用于回显）。
        requested: String,
        /// 解析后的绝对路径。
        resolved: PathBuf,
    },
}

/// 解析失败原因。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum OpenTargetError {
    /// 既不是 http(s) URL，也不像本地路径。
    Unrecognized(String),
    /// 像本地路径，但在所有基准下都不存在。
    NotFound {
        requested: String,
        candidates: Vec<PathBuf>,
    },
}

impl OpenTarget {
    /// 传给系统打开器的实参。
    pub(crate) fn spawn_arg(&self) -> String {
        match self {
            Self::Url(url) => url.clone(),
            Self::Local { resolved, .. } => resolved.to_string_lossy().into_owned(),
        }
    }
}

fn is_external_url(raw: &str) -> bool {
    raw.starts_with("http://") || raw.starts_with("https://")
}

/// 为相对路径生成候选绝对路径：按 `workspace_root → path_base → cwd → 进程 cwd`
/// 顺序尝试，去重并跳过空基准。
pub(crate) fn local_path_candidates(raw: &str, bases: &[Option<&str>]) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    for base in bases.iter().flatten() {
        let base = base.trim();
        if base.is_empty() {
            continue;
        }
        let candidate = Path::new(base).join(raw);
        if !candidates.iter().any(|existing| existing == &candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}

/// 解析 link URL：外部 URL 原样返回；本地路径返回第一个存在（文件或目录）的候选；
/// 全部落空时把候选列表交给调用方报错。
pub(crate) fn resolve_open_target(
    raw: &str,
    workspace_root: Option<&str>,
    path_base: Option<&str>,
    cwd: Option<&str>,
    process_cwd: Option<&Path>,
) -> Result<OpenTarget, OpenTargetError> {
    if is_external_url(raw) {
        return Ok(OpenTarget::Url(raw.to_string()));
    }
    if !looks_like_file_path(raw) {
        return Err(OpenTargetError::Unrecognized(raw.to_string()));
    }

    let process_cwd = process_cwd.map(|path| path.to_string_lossy().into_owned());
    let bases = [workspace_root, path_base, cwd, process_cwd.as_deref()];

    let candidates = if Path::new(raw).is_absolute() {
        vec![PathBuf::from(raw)]
    } else {
        local_path_candidates(raw, &bases)
    };

    match candidates.iter().find(|candidate| candidate.exists()) {
        Some(resolved) => Ok(OpenTarget::Local {
            requested: raw.to_string(),
            resolved: resolved.clone(),
        }),
        None => Err(OpenTargetError::NotFound {
            requested: raw.to_string(),
            candidates,
        }),
    }
}

/// 把系统打开器的退出状态翻译成用户可见结论（#2196）。
///
/// `open`/`xdg-open` 对不存在或无法打开的目标只写 stderr 并返回非 0，
/// 因此「spawn 成功」不能当作打开成功：退出码非 0 一律视为失败。
pub(crate) fn interpret_open_output(
    success: bool,
    code: Option<i32>,
    stderr: &[u8],
) -> Result<(), String> {
    if success {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(stderr);
    let reason = stderr
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| match code {
            Some(code) => format!("退出码 {code}"),
            None => "进程被信号终止".to_string(),
        });
    Err(reason)
}

#[cfg(test)]
#[path = "open_target_tests.rs"]
mod tests;
