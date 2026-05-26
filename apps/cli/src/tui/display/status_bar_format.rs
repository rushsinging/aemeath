use crate::tui::display::safe_text::str_display_width;

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
    pub(crate) session_id: Option<String>,
}

impl Default for StatusLineContext {
    fn default() -> Self {
        Self {
            path_base: String::new(),
            working_root: String::new(),
            worktree_kind: WorktreeKind::Main,
            branch: None,
            permission_mode: "AskMe".to_string(),
            session_id: None,
        }
    }
}

const FIELD_SEPARATOR: &str = " │ ";
const MIN_PATH_WIDTH: usize = 12;
const DEFAULT_PATH_WIDTH: usize = 54;
const DEFAULT_ROOT_WIDTH: usize = 36;
const ELLIPSIS_WIDTH: usize = 1;

pub(crate) fn shorten_path(path: &str, max_cols: usize) -> String {
    if max_cols == 0 || path.is_empty() {
        return String::new();
    }
    let normalized = path.replace('\\', "/");
    if str_display_width(&normalized) <= max_cols {
        return normalized;
    }
    if max_cols <= 2 {
        return "…".repeat(max_cols);
    }
    let prefix = if normalized.starts_with('~') {
        "~"
    } else if normalized.starts_with('/') {
        "/"
    } else {
        "…"
    };
    let prefix_width = str_display_width(prefix);
    let tail_budget = max_cols
        .saturating_sub(prefix_width + ELLIPSIS_WIDTH)
        .max(1);
    let tail = tail_by_display_width(&normalized, tail_budget);
    format!("{prefix}…{tail}")
}

pub(crate) fn truncate_to_display_width(text: &str, max_cols: usize) -> String {
    if max_cols == 0 {
        return String::new();
    }
    if str_display_width(text) <= max_cols {
        return text.to_string();
    }
    if max_cols == 1 {
        return "…".to_string();
    }
    let suffix = tail_by_display_width(text, max_cols - ELLIPSIS_WIDTH);
    format!("…{suffix}")
}

pub(crate) fn context_row_text(context: &StatusLineContext, width: usize) -> String {
    let mut fields = context_row_fields(context, width);
    let row = fields.join(FIELD_SEPARATOR);
    if str_display_width(&row) <= width
        || width == 0
        || row.starts_with('~')
        || row.starts_with('/')
    {
        return row;
    }
    shrink_primary_path_to_fit(&mut fields, width);
    let row = fields.join(FIELD_SEPARATOR);
    if str_display_width(&row) <= width || row.starts_with('~') || row.starts_with('/') {
        return row;
    }
    let fallback = compact_required_fields(&fields, width);
    if fallback.starts_with('~') || fallback.starts_with('/') {
        return fallback;
    }
    if str_display_width(&fallback) <= width {
        return fallback;
    }
    truncate_to_display_width(&fallback, width)
}

fn context_row_fields(context: &StatusLineContext, width: usize) -> Vec<String> {
    let git = git_text(context);
    let permission = context.permission_mode.clone();
    let session = context
        .session_id
        .as_ref()
        .filter(|session| !session.is_empty())
        .map(|session| format!("session {session}"));
    let paths_differ =
        normalized_path(&context.path_base) != normalized_path(&context.working_root);
    let separator_count = (if paths_differ { 3 } else { 2 }) + usize::from(session.is_some());
    let fixed_len = str_display_width(&git)
        + str_display_width(&permission)
        + session.as_ref().map(|s| str_display_width(s)).unwrap_or(0)
        + str_display_width(FIELD_SEPARATOR) * separator_count;
    let available_for_paths = width.saturating_sub(fixed_len).max(MIN_PATH_WIDTH);
    let mut fields = Vec::new();
    if paths_differ {
        let path_width = available_for_paths
            .saturating_sub(DEFAULT_ROOT_WIDTH)
            .max(MIN_PATH_WIDTH);
        fields.push(shorten_path(&context.path_base, path_width));
        fields.push(format!(
            "root {}",
            shorten_path(&context.working_root, DEFAULT_ROOT_WIDTH)
        ));
    } else {
        fields.push(shorten_path(
            &context.path_base,
            available_for_paths.min(DEFAULT_PATH_WIDTH),
        ));
    }
    fields.push(git);
    fields.push(permission);
    if let Some(session) = session {
        fields.push(session);
    }
    fields
}

fn normalized_path(path: &str) -> String {
    path.trim_end_matches('/').replace('\\', "/")
}

fn tail_by_display_width(text: &str, max_cols: usize) -> String {
    if max_cols == 0 {
        return String::new();
    }
    let mut width = 0usize;
    let mut chars = Vec::new();
    for ch in text.chars().rev() {
        let ch_width = str_display_width(&ch.to_string());
        if width + ch_width > max_cols {
            break;
        }
        width += ch_width;
        chars.push(ch);
    }
    chars.into_iter().rev().collect()
}

fn shrink_primary_path_to_fit(fields: &mut [String], width: usize) {
    if fields.is_empty() {
        return;
    }
    let fixed_width = fields
        .iter()
        .skip(1)
        .map(|field| str_display_width(field))
        .sum::<usize>()
        + str_display_width(FIELD_SEPARATOR) * fields.len().saturating_sub(1);
    let path_width = width.saturating_sub(fixed_width).max(2);
    fields[0] = shorten_path(&fields[0], path_width);
}

fn compact_required_fields(fields: &[String], width: usize) -> String {
    let Some(path) = fields.first() else {
        return String::new();
    };
    let session = fields
        .last()
        .filter(|field| field.starts_with("session "))
        .cloned();
    let permission = fields
        .iter()
        .rev()
        .find(|field| !field.starts_with("session "))
        .cloned();
    let mut tail = Vec::new();
    if let Some(permission) = permission {
        tail.push(permission);
    }
    if let Some(session) = session {
        tail.push(session);
    }
    if tail.is_empty() {
        return shorten_path(path, width);
    }
    let tail_text = tail.join(FIELD_SEPARATOR);
    let fixed_width = str_display_width(&tail_text) + str_display_width(FIELD_SEPARATOR);
    let path_width = width.saturating_sub(fixed_width).max(2);
    format!(
        "{}{}{}",
        shorten_path(path, path_width),
        FIELD_SEPARATOR,
        tail_text
    )
}

fn git_text(context: &StatusLineContext) -> String {
    match (&context.worktree_kind, &context.branch) {
        (WorktreeKind::Main, Some(branch)) if branch == context.worktree_kind.label() => {
            context.worktree_kind.label().to_string()
        }
        (WorktreeKind::Main, Some(branch)) if !branch.is_empty() => branch.to_string(),
        (WorktreeKind::Main, _) => context.worktree_kind.label().to_string(),
        (WorktreeKind::Worktree, Some(branch)) if !branch.is_empty() => {
            format!("worktree:{branch}")
        }
        (WorktreeKind::Worktree, _) => context.worktree_kind.label().to_string(),
    }
}
