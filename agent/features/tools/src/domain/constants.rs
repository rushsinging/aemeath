//! Tools domain 常量（#1146 双轨归位）。

/// `goal_text` 合成/回传的预览上限（字符），防止整段 command/JSON 进账本。
pub(crate) const GOAL_TEXT_PREVIEW_CHARS: usize = 80;

pub const RUNTIME_META_KEYS: &[&str] = &["phase"];

pub(crate) const CHAIN_START_COMMANDS: &[&str] = &["cd", "pushd", "popd", "dirs"];

pub(crate) const DEDICATED_FILE_READ_COMMANDS: &[&str] = &["cat", "head", "tail"];

pub(crate) const READONLY_COMMANDS: &[&str] = &[
    "ls",
    "cat",
    "head",
    "tail",
    "wc",
    "nl",
    "stat",
    "file",
    "du",
    "df",
    "pwd",
    "whoami",
    "hostname",
    "uname",
    "date",
    "uptime",
    "env",
    "printenv",
    "echo",
    "printf",
    "which",
    "where",
    "type",
    "find",
    "locate",
    "tree",
    "grep",
    "rg",
    "ag",
    "ack",
    "git status",
    "git log",
    "git diff",
    "git show",
    "git branch",
    "git remote",
    "git tag",
    "git blame",
    "git stash list",
    "cargo check",
    "cargo test",
    "cargo clippy",
    "cargo doc",
    "npm test",
    "npm run lint",
    "npx tsc --noEmit",
    "jq",
    "yq",
    "sort",
    "uniq",
    "cut",
    "tr",
    "docker ps",
    "docker images",
    "docker logs",
    "kubectl get",
    "kubectl describe",
    "kubectl logs",
    "gh pr view",
    "gh issue view",
    "man",
    "help",
    "less",
    "more",
];

// ─── shell_safety.rs ───

pub(crate) const SAFE_DEVS: &[&str] = &["/dev/null", "/dev/stdout", "/dev/stderr", "/dev/tty"];

// ─── timeout policy（全工具统一入参）───

/// 未启用后台化时工具的通用默认 `timeout`（秒，30 分钟）。
///
/// 差异化工具（WebFetch / Agent / Watch 等）在自己的
/// `get_timeout_secs` 覆写里使用各自常量，不读本值。
pub const DEFAULT_TOOL_TIMEOUT_SECS: u64 = 1800;

/// 启用后台化（`tool_background_threshold_secs > 0`）时，无显式入参
/// 调用的默认预算（秒，24 小时）：后台任务允许跑满一天。
pub const BACKGROUND_TOOL_TIMEOUT_SECS: u64 = 86400;

/// 入参 `timeout` 可设置的上限（秒，24 小时）。Watch 在自己的
/// `get_timeout_secs` 覆写里收敛到 300。
pub const DEFAULT_TOOL_MAX_TIMEOUT_SECS: u64 = 86400;

/// 框架级通用 `timeout` 入参的 JSON 字段名（秒）。
///
/// Catalog 用它注入发布 schema，`ExecutionAdapter` 用它在校验与派发前摘除。
/// **NEVER** 使用 `timeout_secs` 作为该字段名。
pub const TIMEOUT_INPUT_FIELD: &str = "timeout";

// ─── background_processes.rs（#1931 / #1946 Watch）───

/// `BackgroundProcessWatch` 的 Tool 框架超时（秒）。
/// 省略入参时的 default，同时作为该工具 max / 框架硬上限。
pub const WATCH_TOOL_TIMEOUT_SECS: u64 = 300;

/// 内部 `wait_task` 相对框架超时提前收口的 grace（秒），避免撞
/// `effective deadline; cleanup unconfirmed`。
pub const WATCH_INTERNAL_GRACE_SECS: u64 = 5;

/// Watch 等待轮询间隔（毫秒）；runtime `wait_task` 与工具侧共用语义。
pub const WATCH_POLL_INTERVAL_MS: u64 = 100;

/// Watch 等待中每次增量读日志的字节上限（流式 ToolProgress）。
pub const WATCH_STREAM_LOG_CHUNK_BYTES: usize = 4_096;
