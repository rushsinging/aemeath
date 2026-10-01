//! Git 上下文标签文案。
//!
//! 迁自 runtime `git_context.rs` 的 `GitContextLabels` 结构体与双语标签。

/// Git 上下文各字段的标签文案。
pub struct GitContextLabels {
    pub header: &'static str,
    pub branch: &'static str,
    pub default_branch: &'static str,
    pub git_user: &'static str,
    pub status: &'static str,
    pub recent_commits: &'static str,
}

/// 按语言选择 git 上下文标签。未知 lang 回退英文。
pub fn git_context_labels(lang: &str) -> GitContextLabels {
    match lang {
        "zh" => GitContextLabels {
            header: "# Git Context",
            branch: "当前分支",
            default_branch: "默认分支",
            git_user: "Git 用户",
            status: "状态",
            recent_commits: "最近提交",
        },
        _ => GitContextLabels {
            header: "# Git Context",
            branch: "Current branch",
            default_branch: "Default branch",
            git_user: "Git user",
            status: "Status",
            recent_commits: "Recent commits",
        },
    }
}

#[cfg(test)]
#[path = "git_context_labels_tests.rs"]
mod tests;
