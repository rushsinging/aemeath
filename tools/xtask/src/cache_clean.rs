//! worktree target 缓存清理（#2063）：`~/.cache/aemeath-target/<key>` 按
//! checkout/worktree 隔离，worktree 删除后缓存成孤儿（17 个退役缓存 34GB
//! 实证，曾写满磁盘导致 rustc IO 失败）。
//!
//! `cache-clean`：枚举缓存目录 → 与 `git worktree list` 的活跃键比对 →
//! 删除孤儿。键匹配规则：缓存键是 `<分支安全名>-<目录内容短哈希>`，取
//! 前缀（首个 `-` 前段）对 worktree 分支名的安全形态；实现取「worktree
//! 路径安全形态」与「分支名安全形态」双匹配，NEVER 误删活跃缓存。

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// 扫描并清理孤儿缓存。返回 (删除的目录数, 释放字节数)。
pub fn clean(cache_root: &Path, repo_root: &Path, dry_run: bool) -> Result<(usize, u64)> {
    let active_keys = active_worktree_keys(repo_root)?;
    let entries = match std::fs::read_dir(cache_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((0, 0));
        }
        Err(error) => return Err(error).context("读取缓存目录失败"),
    };
    let mut removed = 0usize;
    let mut freed_bytes = 0u64;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let key = match path.file_name().and_then(|name| name.to_str()) {
            Some(key) => key.to_string(),
            None => continue,
        };
        if is_active(&key, &active_keys) {
            continue;
        }
        let size = dir_size(&path);
        if dry_run {
            println!("[dry-run] 将删除 {key}（{} MB）", size / 1024 / 1024);
        } else {
            if let Err(error) = std::fs::remove_dir_all(&path) {
                eprintln!("删除失败 {key}：{error}");
                continue;
            }
            println!("已删除 {key}（{} MB）", size / 1024 / 1024);
        }
        removed += 1;
        freed_bytes += size;
    }
    Ok((removed, freed_bytes))
}

/// 活跃 worktree 的缓存键匹配原料：每个 worktree 的（分支名安全形态，
/// 路径安全形态）。
fn active_worktree_keys(repo_root: &Path) -> Result<Vec<(String, String)>> {
    // git 子进程统一经 project::run_git_command 窄面（全仓唯一 spawn 点，
    // 守卫 pattern.all.git-command-single-owner）。
    let outcome = project::run_git_command(repo_root, &["worktree", "list", "--porcelain"])
        .map_err(|_error| anyhow::anyhow!("git worktree list 失败"))?;
    if !outcome.is_success() {
        anyhow::bail!("git worktree list 退出非零");
    }
    let text = String::from_utf8_lossy(outcome.stdout_bytes()).to_string();
    let mut keys = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_branch: Option<String> = None;
    for line in text.lines() {
        if let Some(path_text) = line.strip_prefix("worktree ") {
            if let (Some(path), Some(branch)) = (current_path.take(), current_branch.take()) {
                keys.push(worktree_key_forms(&path, &branch));
            }
            current_path = Some(PathBuf::from(path_text));
        } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
            current_branch = Some(branch.to_string());
        }
    }
    if let (Some(path), Some(branch)) = (current_path, current_branch) {
        keys.push(worktree_key_forms(&path, &branch));
    }
    Ok(keys)
}

/// worktree 的两种键形态：（分支名安全化，路径绝对安全化）。
fn worktree_key_forms(path: &Path, branch: &str) -> (String, String) {
    let safe_branch = sanitize(branch);
    let safe_path = sanitize(&path.to_string_lossy());
    (safe_branch, safe_path)
}

/// 缓存键安全化规则：`/` 等路径分隔符 → `_`（**保留 `-`**，与
/// target-dir 键生成规则一致——分支 `feature/2063-followups` →
/// `feature_2063-followups`），小写。
fn sanitize(raw: &str) -> String {
    raw.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// 缓存键是否属于活跃 worktree：键以形态为前缀且后随 `-哈希` 边界
///（形态自身可含 `-`，NEVER 按首段切割——那会把
/// `feature_2063-followups` 误切为 `feature_2063`）。
fn is_active(cache_key: &str, active: &[(String, String)]) -> bool {
    active.iter().any(|(branch_form, path_form)| {
        [branch_form, path_form].iter().any(|form| {
            cache_key.starts_with(form.as_str())
                && cache_key[form.len()..]
                    .chars()
                    .next()
                    .is_none_or(|next| next == '-')
        })
    })
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        if entry_path.is_dir() {
            total += dir_size(&entry_path);
        } else if let Ok(meta) = entry.metadata() {
            total += meta.len();
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_hyphen_replaces_slash() {
        assert_eq!(sanitize("feature/2063-followups"), "feature_2063-followups");
        assert_eq!(
            sanitize("/Users/x/.agents/worktrees/aemeath/wt"),
            "_users_x__agents_worktrees_aemeath_wt"
        );
    }

    #[test]
    fn is_active_matches_form_prefix_with_boundary() {
        let active = vec![(
            "feature_2063-followups".to_string(),
            "_users_x_worktrees_feature_2063-followups".to_string(),
        )];
        // 形态含 `-`：NEVER 按首段切割（旧实现把 2063-followups 误切）
        assert!(is_active(
            "feature_2063-followups-549bf34bae0cadae",
            &active
        ));
        assert!(is_active(
            "_users_x_worktrees_feature_2063-followups-abc",
            &active
        ));
        // 前缀相同但边界后非哈希（更长的不同键）——形态边界检查：
        // "feature_2063-followups-v2-…" 前缀匹配且后随 '-' → 视为同形态
        // 变体（保守保留，NEVER 误删）
        assert!(is_active("feature_2063-followups-v2-abc", &active));
        assert!(!is_active("feature_2050-rsi-dry-run-8bab946", &active));
    }

    #[test]
    fn is_active_matches_main_checkout() {
        let active = vec![("main".to_string(), "_users_x_aemeath".to_string())];
        assert!(is_active("main-3a72a5351c604f31", &active));
        // 前缀命中但边界非 '-'（mainXYZ 是别的键）——不匹配
        assert!(!is_active("mainxyz-123", &active));
    }
}
