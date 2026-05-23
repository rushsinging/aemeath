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

const FIELD_SEPARATOR: &str = " │ ";
const WIDE_CTX_WIDTH: usize = 28;
const WIDE_ROOT_WIDTH: usize = 30;
const NARROW_CTX_WIDTH: usize = 16;

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
    truncate_to_char_count(
        &context_row_fields(context, width).join(FIELD_SEPARATOR),
        width,
    )
}

fn context_row_fields(context: &StatusLineContext, width: usize) -> Vec<String> {
    let mut required = vec![
        format!("ctx {}", shorten_path(&context.path_base, NARROW_CTX_WIDTH)),
        git_text(context, width),
        format!("Perm:{}", context.permission_mode),
    ];
    let mut preserve_ctx_tail = width < 70;
    if width >= 70 {
        required.insert(
            1,
            format!(
                "root {}",
                shorten_path(&context.working_root, WIDE_ROOT_WIDTH)
            ),
        );
        required[0] = format!("ctx {}", shorten_path(&context.path_base, WIDE_CTX_WIDTH));
        preserve_ctx_tail = false;
    }
    fit_fields(required, width, preserve_ctx_tail)
}

fn git_text(context: &StatusLineContext, width: usize) -> String {
    match (&context.worktree_kind, &context.branch) {
        (WorktreeKind::Main, Some(branch)) if branch == context.worktree_kind.label() => {
            context.worktree_kind.label().to_string()
        }
        (WorktreeKind::Main, Some(branch)) if !branch.is_empty() && width >= 70 => {
            format!("main:{branch}")
        }
        (WorktreeKind::Main, _) => context.worktree_kind.label().to_string(),
        (WorktreeKind::Worktree, Some(branch)) if !branch.is_empty() => {
            format!("worktree:{branch}")
        }
        (WorktreeKind::Worktree, _) => context.worktree_kind.label().to_string(),
    }
}

fn fit_fields(fields: Vec<String>, width: usize, preserve_ctx_tail: bool) -> Vec<String> {
    let mut result = fields;
    while joined_len(&result) > width && result.len() > 3 {
        result.remove(1);
    }
    if joined_len(&result) <= width {
        return result;
    }
    let len = result.len();
    let separators = FIELD_SEPARATOR.chars().count() * len.saturating_sub(1);
    let fixed: usize = result
        .iter()
        .skip(1)
        .map(|field| field.chars().count())
        .sum();
    let ctx_budget = width.saturating_sub(separators + fixed).max(5);
    if preserve_ctx_tail {
        return result;
    }
    result[0] = fit_ctx_field(&result[0], ctx_budget);
    result
}

fn fit_ctx_field(field: &str, max_chars: usize) -> String {
    if field.chars().count() <= max_chars {
        return field.to_string();
    }
    if let Some(path) = field.strip_prefix("ctx …/") {
        let prefix = "ctx …/";
        let budget = max_chars.saturating_sub(prefix.chars().count());
        return format!("{prefix}{}", truncate_to_char_count(path, budget));
    }
    truncate_to_char_count(field, max_chars)
}

fn joined_len(fields: &[String]) -> usize {
    let separators = FIELD_SEPARATOR.chars().count() * fields.len().saturating_sub(1);
    separators
        + fields
            .iter()
            .map(|field| field.chars().count())
            .sum::<usize>()
}
