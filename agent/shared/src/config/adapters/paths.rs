//! Codex 风格配置路径。

use std::path::{Path, PathBuf};

pub use super::constants::{
    AEMEATH_HOME_DIR_ENV, AEMEATH_HOME_DIR_NAME, AGENTS_DIR_ENV, AGENTS_DIR_NAME, AGENTS_MD,
    CLAUDE_DIR_NAME, CLAUDE_MD, GUARDRAILS_FILE_NAME, GUIDANCE_DIR_NAME, HISTORY_FILE,
    HOOKS_DIR_NAME, LOGS_DIR_NAME, MCP_CONFIG_FILE, MEMORY_DIR_NAME, MODELS_DIR_NAME,
    NEW_CONFIG_FILE, OLD_AEMEATH_DIR_NAME, OLD_CONFIG_FILE, SCRIPTS_DIR_NAME, SESSIONS_DIR_NAME,
    SETTINGS_FILE, SKILLS_DIR_NAME, STATE_DIR_NAME, SYSTEMONE_DIR_NAME, TOOL_GUIDANCE_FILE_NAME,
    TOOL_RESULTS_DIR_NAME, WORKTREES_DIR_NAME,
};

/// 解析 home 目录（读取 `$HOME`）。
///
/// 不依赖 `dirs` crate，以满足 shared kernel 零外部行为依赖约束
///（见 `check-share-minimal-kernel.sh` 依赖白名单）。Unix 下 `$HOME`
/// 始终设置；项目仅发布 macOS/Linux 二进制。
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn home_dir_or_dot() -> PathBuf {
    home_dir().unwrap_or_else(|| PathBuf::from("."))
}

pub fn global_agents_dir() -> PathBuf {
    if let Ok(value) = std::env::var(AGENTS_DIR_ENV) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return expand_home(Path::new(trimmed));
        }
    }

    home_dir_or_dot().join(AGENTS_DIR_NAME)
}

/// `~/.aemeath` — aemeath 私有磁盘根目录（用户级知识 + 运行时状态）。
///
/// 与 [`global_agents_dir`] 分层：`~/.agents` 承载用户通用知识、配置与
/// 跨 agent 资产；`~/.aemeath` 只放 aemeath 专属知识（`tool_guidance.md`）
/// 与运行时状态。支持 `AEMEATH_HOME_DIR` 覆盖（空白值视为未设置，
/// `~` / `~/` 前缀按 home 展开，其余值原样返回）。
pub fn aemeath_home_dir() -> PathBuf {
    aemeath_home_dir_from(std::env::var(AEMEATH_HOME_DIR_ENV).ok(), home_dir())
}

/// `~/.aemeath/state/` — aemeath 运行时状态根目录。
pub fn aemeath_state_dir() -> PathBuf {
    aemeath_home_dir().join(STATE_DIR_NAME)
}

/// `~/.aemeath/state/<leaf>/` — 运行时状态叶子目录。
///
/// 叶子名保持与旧 `~/.agents/<leaf>` 同名（`session` / `sessions` /
/// `harness` / `scoring` / …），便于迁移前后按同名目录对账。
pub fn aemeath_state_leaf_dir(leaf: &str) -> PathBuf {
    aemeath_state_dir().join(leaf)
}

/// `aemeath_home_dir` 的纯函数形态。
///
/// 把 env 覆盖值与 home 显式传入，使「HOME 缺失」「相对路径」「`~` 展开」
/// 等边界可在测试中稳定覆盖，避免并行测试改动进程级环境。
fn aemeath_home_dir_from(env_value: Option<String>, home: Option<PathBuf>) -> PathBuf {
    if let Some(trimmed) = env_value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return expand_home_with(Path::new(trimmed), home.as_deref());
    }
    home.unwrap_or_else(|| PathBuf::from("."))
        .join(AEMEATH_HOME_DIR_NAME)
}

/// `~/.aemeath/models/` — 本地模型资产根目录。
pub fn global_models_dir() -> PathBuf {
    aemeath_home_dir().join(MODELS_DIR_NAME)
}

/// `~/.agents/models/` — 旧模型资产根目录（只读 fallback 与迁移对账）。
pub fn legacy_global_models_dir() -> PathBuf {
    global_agents_dir().join(MODELS_DIR_NAME)
}

/// `~/.agents/models/systemone/` — System One GGUF 模型缓存目录。
///
/// 单一路径真相源：所有 System One 模型资产读写都从此函数取路径。
pub fn systemone_models_dir() -> PathBuf {
    global_models_dir().join(SYSTEMONE_DIR_NAME)
}

/// 展开 `~` / `~/` 前缀为 home 目录。
///
/// - `~` → home
/// - `~/foo` → home/foo
/// - 其它原样返回
pub fn expand_home(path: &Path) -> PathBuf {
    expand_home_with(path, home_dir().as_deref())
}

/// `expand_home` 的纯函数形态：home 显式传入，用于测试 HOME 缺失边界。
fn expand_home_with(path: &Path, home: Option<&Path>) -> PathBuf {
    let text = path.to_string_lossy();
    if text == "~" {
        return home
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
    }
    if let Some(rest) = text.strip_prefix("~/") {
        if let Some(home) = home {
            return home.join(rest);
        }
    }
    path.to_path_buf()
}

pub fn global_config_path() -> PathBuf {
    global_agents_dir().join(NEW_CONFIG_FILE)
}

pub fn old_global_config_path() -> PathBuf {
    PathBuf::from(OLD_AEMEATH_DIR_NAME).join(OLD_CONFIG_FILE)
}

pub fn project_config_path(project_dir: &Path) -> PathBuf {
    project_dir.join(AGENTS_DIR_NAME).join(NEW_CONFIG_FILE)
}

pub fn old_project_config_path(project_dir: &Path) -> PathBuf {
    project_dir.join(OLD_AEMEATH_DIR_NAME).join(OLD_CONFIG_FILE)
}

pub fn global_agents_md_path() -> PathBuf {
    global_agents_dir().join(AGENTS_MD)
}

pub fn old_global_claude_md_path() -> PathBuf {
    home_dir_or_dot().join(CLAUDE_DIR_NAME).join(CLAUDE_MD)
}

pub fn project_agents_md_path(cwd: &Path) -> PathBuf {
    cwd.join(AGENTS_MD)
}

pub fn old_project_claude_md_path(cwd: &Path) -> PathBuf {
    cwd.join(CLAUDE_MD)
}

pub fn project_claude_settings_path(cwd: &Path) -> PathBuf {
    cwd.join(CLAUDE_DIR_NAME).join(SETTINGS_FILE)
}

/// 项目 instructions 文件的解析结果：同目录 `AGENTS.md` 优先，否则 `CLAUDE.md`；
/// 两者都不存在返回 `None`（不在此处创建文件）。
///
/// 这是 runtime 指令加载、RSI project overlay 注册与 `/init` 的统一入口。
pub fn resolve_project_instruction_path(dir: &Path) -> Option<PathBuf> {
    resolve_project_instruction_path_with(dir, Path::exists)
}

/// 项目 instructions 的写入目标：`AGENTS.md` 存在则写它，否则 `CLAUDE.md`
/// 存在则写它，都不存在则返回待创建的 `AGENTS.md`。
///
/// `/init` 与 RSI 写入 MUST 使用本函数，避免另起一套优先级判断。
pub fn project_instruction_write_target(dir: &Path) -> PathBuf {
    project_instruction_write_target_with(dir, Path::exists)
}

/// `project_instruction_write_target` 的纯函数形态（exists 注入便于测试）。
fn project_instruction_write_target_with(dir: &Path, exists: impl Fn(&Path) -> bool) -> PathBuf {
    match resolve_project_instruction_path_with(dir, exists) {
        Some(path) => path,
        None => dir.join(AGENTS_MD),
    }
}

/// 「AGENTS 优先、CLAUDE fallback」的纯函数形态（exists 可注入，便于测试）。
fn resolve_project_instruction_path_with(
    dir: &Path,
    exists: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let agents = dir.join(AGENTS_MD);
    if exists(&agents) {
        return Some(agents);
    }
    let claude = dir.join(CLAUDE_MD);
    if exists(&claude) {
        return Some(claude);
    }
    None
}

/// 从 `cwd` 向上探索 `depth` 级祖先目录（含 `cwd` 自身），返回目录路径列表。
///
/// 纯路径拼接，无 fs IO。用于项目指令搜索（`load_agents_md`）与
/// config reload snapshot 监控共享同一套目录发现逻辑。
/// 返回顺序：`[cwd, parent, grandparent, ...]`，共 `depth + 1` 个元素。
pub fn project_instruction_dirs(cwd: &Path, depth: u32) -> Vec<PathBuf> {
    let mut dirs = Vec::with_capacity(depth as usize + 1);
    let mut current = Some(cwd);
    for _ in 0..=depth {
        match current {
            Some(dir) => {
                dirs.push(dir.to_path_buf());
                current = dir.parent();
            }
            None => break,
        }
    }
    dirs
}

pub fn project_claude_skills_dir(cwd: &Path) -> PathBuf {
    cwd.join(CLAUDE_DIR_NAME).join(SKILLS_DIR_NAME)
}

pub fn global_skills_dir() -> PathBuf {
    global_agents_dir().join(SKILLS_DIR_NAME)
}

pub fn global_logs_dir() -> PathBuf {
    aemeath_home_dir().join(LOGS_DIR_NAME)
}

/// `~/.agents/logs/` — 旧日志目录（只读 fallback）。
pub fn legacy_global_logs_dir() -> PathBuf {
    global_agents_dir().join(LOGS_DIR_NAME)
}

pub fn global_guidance_dir() -> PathBuf {
    aemeath_home_dir().join(GUIDANCE_DIR_NAME)
}

/// `~/.agents/guidance/` — 旧 guidance 目录（只读 fallback）。
pub fn legacy_global_guidance_dir() -> PathBuf {
    global_agents_dir().join(GUIDANCE_DIR_NAME)
}

pub fn global_memory_dir() -> PathBuf {
    aemeath_state_leaf_dir(MEMORY_DIR_NAME)
}

pub fn global_sessions_dir() -> PathBuf {
    aemeath_state_leaf_dir(SESSIONS_DIR_NAME)
}

/// `~/.agents/worktrees/` — EnterWorktree 默认创建 worktree 的根目录。
pub fn global_worktrees_dir() -> PathBuf {
    global_agents_dir().join(WORKTREES_DIR_NAME)
}

/// `~/.aemeath/state/tool-results/` — 超长工具结果的落盘根目录。
pub fn global_tool_results_dir() -> PathBuf {
    aemeath_state_leaf_dir(TOOL_RESULTS_DIR_NAME)
}

/// `~/.agents/tool-results/` — 旧工具结果根（只读 fallback 与迁移对账）。
pub fn legacy_global_tool_results_dir() -> PathBuf {
    global_agents_dir().join(TOOL_RESULTS_DIR_NAME)
}

/// `~/.agents/tool-results/{session_id}/` — 某个 session 的工具结果子目录。
///
/// 工具结果按 session ID 归档，但删除 Session 不级联删除历史工具结果。
pub fn session_tool_results_dir(session_id: &str) -> PathBuf {
    global_tool_results_dir().join(session_id)
}

pub fn global_hooks_dir() -> PathBuf {
    global_agents_dir().join(HOOKS_DIR_NAME)
}

pub fn global_mcp_config_path() -> PathBuf {
    global_agents_dir().join(MCP_CONFIG_FILE)
}

pub fn global_history_path() -> PathBuf {
    global_agents_dir().join(HISTORY_FILE)
}

pub fn global_settings_path() -> PathBuf {
    global_agents_dir().join(SETTINGS_FILE)
}

pub fn old_global_skills_dir() -> PathBuf {
    PathBuf::from(OLD_AEMEATH_DIR_NAME).join(SKILLS_DIR_NAME)
}

pub fn project_skills_dir(cwd: &Path) -> PathBuf {
    cwd.join(AGENTS_DIR_NAME).join(SKILLS_DIR_NAME)
}

pub fn old_project_skills_dir(cwd: &Path) -> PathBuf {
    cwd.join(OLD_AEMEATH_DIR_NAME).join(SKILLS_DIR_NAME)
}

/// 全局 guardrails 组件文件（c2，#2021）。
pub fn global_guardrails_path() -> PathBuf {
    global_agents_dir().join(GUARDRAILS_FILE_NAME)
}

/// 项目级 guardrails 组件文件（c2，#2021）。
pub fn project_guardrails_path(cwd: &Path) -> PathBuf {
    cwd.join(AGENTS_DIR_NAME).join(GUARDRAILS_FILE_NAME)
}

/// `~/.aemeath/tool_guidance.md` — aemeath 工具纪律（canonical 落点）。
pub fn global_tool_guidance_path() -> PathBuf {
    aemeath_home_dir().join(TOOL_GUIDANCE_FILE_NAME)
}

/// `~/.agents/tool_guidance.md` — 旧落点（只读 fallback，不再写入）。
pub fn legacy_global_tool_guidance_path() -> PathBuf {
    global_agents_dir().join(TOOL_GUIDANCE_FILE_NAME)
}

/// tool_guidance 的解析结果：新路径存在则用新；否则旧路径存在则只读 fallback；
/// 两者都不存在时返回新路径（调用方据此静默跳过或新建）。
///
/// 两者同时存在时**只用新路径**：不合并、不覆盖旧文件。
pub fn resolved_tool_guidance_path() -> PathBuf {
    resolve_canonical_then_legacy(
        global_tool_guidance_path(),
        legacy_global_tool_guidance_path(),
        Path::exists,
    )
}

/// 「canonical 优先、legacy 只读 fallback、都不存在回落 canonical」的纯函数形态。
fn resolve_canonical_then_legacy(
    canonical: PathBuf,
    legacy: PathBuf,
    exists: impl Fn(&Path) -> bool,
) -> PathBuf {
    if exists(&canonical) {
        return canonical;
    }
    if exists(&legacy) {
        return legacy;
    }
    canonical
}

/// 全局 scripts 组件目录（c7，#2020）。
pub fn global_scripts_dir() -> PathBuf {
    global_agents_dir().join(SCRIPTS_DIR_NAME)
}

/// session dataset 落盘根（单数 `session/`；`sessions/` 是 lock 目录）。
pub fn global_session_datasets_dir() -> PathBuf {
    aemeath_state_leaf_dir("session")
}

/// 旧 session dataset 落盘根（`~/.agents/session/`，只读 fallback）。
pub fn legacy_global_session_datasets_dir() -> PathBuf {
    global_agents_dir().join("session")
}

/// `~/.aemeath/state/<leaf>/` 的旧位置（`~/.agents/<leaf>/`）。
///
/// 用于运行时状态迁移对账与只读 fallback：新路径缺失时读旧位置，
/// 但**新写入 MUST 走** [`aemeath_state_leaf_dir`]。
pub fn legacy_agents_state_leaf_dir(leaf: &str) -> PathBuf {
    global_agents_dir().join(leaf)
}

/// `~/.aemeath/state/harness/` — RSI 经验库、事件流与组件版本。
pub fn global_harness_dir() -> PathBuf {
    aemeath_state_leaf_dir("harness")
}

/// `~/.aemeath/state/scoring/` — 评分 daemon 的 UDS 与状态。
pub fn global_scoring_dir() -> PathBuf {
    aemeath_state_leaf_dir("scoring")
}

/// `~/.aemeath/state/workflows/` — Workflow 实体落盘根。
pub fn global_workflows_dir() -> PathBuf {
    aemeath_state_leaf_dir("workflows")
}

/// `~/.aemeath/state/audit/` — 审计落盘根。
pub fn global_audit_dir() -> PathBuf {
    aemeath_state_leaf_dir("audit")
}

/// `~/.aemeath/state/background-process/` — 后台进程账本与日志。
pub fn global_background_process_dir() -> PathBuf {
    aemeath_state_leaf_dir("background-process")
}

/// `~/.aemeath/state/background-task/` — 后台任务锁与账本。
pub fn global_background_task_dir() -> PathBuf {
    aemeath_state_leaf_dir("background-task")
}

/// `~/.aemeath/state/meta/` — 运行期元数据。
pub fn global_meta_dir() -> PathBuf {
    aemeath_state_leaf_dir("meta")
}

/// `~/.aemeath/state/tmp/` — 运行期临时文件。
pub fn global_tmp_dir() -> PathBuf {
    aemeath_state_leaf_dir("tmp")
}

/// 运行时状态迁移涉及的叶子目录（与旧 `~/.agents/<leaf>` 同名）。
pub const RUNTIME_STATE_LEAVES: &[&str] = &[
    "session",
    "sessions",
    "harness",
    "scoring",
    "background-process",
    "background-task",
    "workflows",
    "audit",
    "tool-results",
    "memory",
    "meta",
    "tmp",
];

/// 全局 plugins 组件目录（c8，#2030）。
pub fn global_plugins_dir() -> PathBuf {
    global_agents_dir().join("plugins")
}

/// 项目级 scripts 组件目录（c7，#2020）。
pub fn project_scripts_dir(cwd: &Path) -> PathBuf {
    cwd.join(AGENTS_DIR_NAME).join(SCRIPTS_DIR_NAME)
}

#[cfg(test)]
#[path = "paths_tests.rs"]
mod tests;
