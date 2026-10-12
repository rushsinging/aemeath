//! Tools domain 常量（#1146 双轨归位）。

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

// ─── background_processes.rs（#1931 / #1946 Watch）───

/// `BackgroundProcessWatch` 的 Tool 框架超时（秒）。
/// 唯一超时源：`Tool::timeout_secs()` / execution_supervisor effective deadline。
pub const WATCH_TOOL_TIMEOUT_SECS: u64 = 300;

/// 内部 `wait_task` 相对框架超时提前收口的 grace（秒），避免撞
/// `effective deadline; cleanup unconfirmed`。
pub const WATCH_INTERNAL_GRACE_SECS: u64 = 5;

/// Watch 等待轮询间隔（毫秒）；runtime `wait_task` 与工具侧共用语义。
pub const WATCH_POLL_INTERVAL_MS: u64 = 100;

/// Watch 等待中每次增量读日志的字节上限（流式 ToolProgress）。
pub const WATCH_STREAM_LOG_CHUNK_BYTES: usize = 4_096;
