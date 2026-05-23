#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorktreeKind {
    Main,
    Worktree,
}

impl WorktreeKind {
    pub fn label(self) -> &'static str {
        match self {
            WorktreeKind::Main => "main",
            WorktreeKind::Worktree => "worktree",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct StatusLineContext {
    pub(crate) path_base: String,
    pub(crate) working_root: String,
    pub(crate) worktree_kind: WorktreeKind,
    pub(crate) branch: Option<String>,
    pub(crate) permission_mode: String,
}

impl Default for StatusLineContext {
    fn default() -> Self {
        Self {
            path_base: String::new(),
            working_root: String::new(),
            worktree_kind: WorktreeKind::Main,
            branch: None,
            permission_mode: "AskMe".to_string(),
        }
    }
}

pub(crate) fn context_path_width(width: usize) -> usize {
    if width < 70 {
        16
    } else {
        42
    }
}

pub(crate) fn root_path_width(width: usize) -> usize {
    if width < 70 {
        0
    } else {
        28
    }
}

pub(crate) fn shorten_path(path: &str, max_chars: usize) -> String {
    if max_chars == 0 || path.is_empty() {
        return String::new();
    }
    let normalized = path.replace('\\', "/");
    let parts: Vec<&str> = normalized
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() {
        return truncate_to_char_count(&normalized, max_chars);
    }
    let tail_parts = if max_chars < 18 {
        1
    } else {
        3.min(parts.len())
    };
    let tail = parts[parts.len() - tail_parts..].join("/");
    let candidate = if parts.len() > tail_parts {
        format!("…/{tail}")
    } else {
        tail
    };
    truncate_to_char_count(&candidate, max_chars)
}

pub(crate) fn truncate_to_char_count(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    if max_chars == 1 {
        return "…".to_string();
    }
    let suffix: String = text
        .chars()
        .rev()
        .take(max_chars - 1)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{suffix}")
}

pub(crate) fn context_row_text(context: &StatusLineContext, width: usize) -> String {
    let path = shorten_path(&context.path_base, context_path_width(width));
    let root = shorten_path(&context.working_root, root_path_width(width));
    let git = match &context.branch {
        Some(branch) if !branch.is_empty() && width < 70 => branch.clone(),
        Some(branch) if !branch.is_empty() => {
            format!("{}:{}", context.worktree_kind.label(), branch)
        }
        _ => context.worktree_kind.label().to_string(),
    };
    let full = if width < 70 {
        format!("ctx {} │ {} │ Perm:{}", path, git, context.permission_mode)
    } else {
        format!(
            "ctx {} │ root {} │ {} │ Perm:{}",
            path, root, git, context.permission_mode
        )
    };
    truncate_to_char_count(&full, width)
}
