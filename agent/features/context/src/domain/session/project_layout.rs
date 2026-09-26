//! session 存储的按 project 分目录布局。
//!
//! session 文件历史上平铺在 `sessions/<id>.json`，所有项目混居；列表查询被迫
//! 全量加载才能判定归属（910 个文件 576MB 的实测代价：78 秒阻塞 + 数百 MB
//! 峰值内存）。新布局把 session 放进 `sessions/<project-dir>/<id>`，
//! `project-dir` 由 project identity 哈希派生：同项目的所有 worktree 落到
//! 同一目录，跨项目 session 互不可见，列表查询天然只读本项目。

use sha2::{Digest, Sha256};
use share::session_types::ProjectIdentityData;
use std::str::FromStr;
use storage::SafePathSegment;

/// project identity → 稳定目录段。
///
/// 派生规则：git 项目用 `git:<git_common_dir>`，非 git 项目用
/// `cwd:<initial_cwd>` 作为哈希输入，取 SHA-256 前 16 个 hex 字符。
/// 同一 identity（含同一仓库的不同 worktree）得到同一段；不同 identity
/// 得到不同段的概率由 64 bit 哈希前缀保证。
pub fn project_dir_segment(identity: &ProjectIdentityData) -> SafePathSegment {
    let canonical = match identity.git_common_dir.as_deref() {
        Some(common_dir) => format!("git:{common_dir}"),
        None => format!("cwd:{}", identity.initial_cwd),
    };
    let digest = Sha256::digest(canonical.as_bytes());
    let segment: String = digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    SafePathSegment::from_str(&segment).expect("16 个 hex 字符必然是合法路径段")
}

/// session 的落盘目录段：workspace 已捕获 identity 时派生目录段；
/// workspace 缺失（无归属信息）时返回 `None`，调用方退回平铺 key。
pub fn session_project_dir(session: &super::CanonicalSession) -> Option<SafePathSegment> {
    match &session.workspace {
        super::SnapshotState::Captured(context) => {
            Some(project_dir_segment(&context.project_identity))
        }
        super::SnapshotState::CapturedEmpty | super::SnapshotState::Missing => None,
    }
}

#[cfg(test)]
#[path = "project_layout_tests.rs"]
mod tests;
