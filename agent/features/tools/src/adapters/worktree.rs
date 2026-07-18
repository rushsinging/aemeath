//! Enter/Exit Worktree tools
//!
//! These tools allow the agent to switch between git worktree directories
//! while maintaining a context stack for nested worktree navigation.

use crate::domain::types::enter_worktree::{EnterWorktreeInput, EnterWorktreeResult};
use crate::domain::types::exit_worktree::{ExitWorktreeInput, ExitWorktreeResult};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::i18n::tools::worktree as t;
use std::borrow::Cow;
use std::path::{Path, PathBuf};

fn current_branch(ctx: &ToolExecutionContext) -> String {
    ctx.workspace_read()
        .current_branch()
        .ok()
        .flatten()
        .unwrap_or_else(|| "(unknown)".to_string())
}
/// Tool to enter a git worktree directory
pub struct EnterWorktreeTool;

/// Tool to exit the current worktree and restore the previous context
pub struct ExitWorktreeTool;

fn workspace_context_payload(
    _headline: &str,
    branch: &str,
    path_base: &Path,
    workspace_root: &Path,
    lang: &str,
) -> EnterWorktreeResult {
    EnterWorktreeResult {
        branch: branch.to_string(),
        path_base: path_base.to_path_buf(),
        workspace_root: workspace_root.to_path_buf(),
        guidance: t::enter_guidance(lang).to_string(),
    }
}

#[async_trait]
impl TypedTool for EnterWorktreeTool {
    type Output = EnterWorktreeResult;
    fn name(&self) -> &'static str {
        "EnterWorktree"
    }

    fn description(&self) -> &'static str {
        t::enter_description(share::i18n::DEFAULT_LANG)
    }

    fn description_for(&self, lang: &str) -> Cow<'_, str> {
        Cow::Borrowed(t::enter_description(lang))
    }

    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        EnterWorktreeInput::data_schema()
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolExecutionContext,
    ) -> TypedToolResult<EnterWorktreeResult> {
        let args: EnterWorktreeInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(e) => {
                return TypedToolResult::error(t::invalid_input_error(&ctx.resources.lang, e))
            }
        };

        let display_target = args.path.clone().unwrap_or_else(|| {
            args.branch
                .clone()
                .map(|branch| format!("branch {branch}"))
                .unwrap_or_else(|| {
                    if ctx.resources.lang == "zh" {
                        "未指定目标".to_string()
                    } else {
                        "(unspecified)".to_string()
                    }
                })
        });

        match ctx
            .workspace_control()
            .enter(args.path.as_ref().map(PathBuf::from), args.branch.clone())
        {
            Ok(_frame) => {
                let path_base = ctx.workspace_read().current_path_base();
                let workspace_root = ctx.workspace_read().current_workspace_root();
                let branch = current_branch(ctx);
                let headline = if ctx.resources.lang == "zh" {
                    format!("已进入 worktree：{}", display_target)
                } else {
                    format!("Entered worktree: {}", display_target)
                };
                TypedToolResult::success(
                    headline.clone(),
                    workspace_context_payload(
                        &headline,
                        &branch,
                        &path_base,
                        &workspace_root,
                        &ctx.resources.lang,
                    ),
                )
            }
            Err(e) => TypedToolResult::error(t::enter_error(&ctx.resources.lang, e)),
        }
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }
}

#[async_trait]
impl TypedTool for ExitWorktreeTool {
    type Output = ExitWorktreeResult;
    fn name(&self) -> &'static str {
        "ExitWorktree"
    }

    fn description(&self) -> &'static str {
        t::exit_description(share::i18n::DEFAULT_LANG)
    }

    fn description_for(&self, lang: &str) -> Cow<'_, str> {
        Cow::Borrowed(t::exit_description(lang))
    }

    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        ExitWorktreeInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        ExitWorktreeResult::data_schema()
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolExecutionContext,
    ) -> TypedToolResult<ExitWorktreeResult> {
        let args: ExitWorktreeInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(e) => {
                return TypedToolResult::error(t::invalid_input_error(&ctx.resources.lang, e))
            }
        };

        if let Some(path) = args.path {
            // 直接切到指定路径：校验存在性 + 同源，不污染上下文栈（不留多余栈帧）。
            match ctx.workspace_control().switch_to(PathBuf::from(&path)) {
                Ok(()) => {
                    let path_base = ctx.workspace_read().current_path_base();
                    let workspace_root = ctx.workspace_read().current_workspace_root();
                    let branch = current_branch(ctx);
                    let headline = if ctx.resources.lang == "zh" {
                        format!("已切换到：{}", path)
                    } else {
                        format!("Switched to: {}", path)
                    };
                    TypedToolResult::success(
                        headline.clone(),
                        ExitWorktreeResult {
                            branch: branch.clone(),
                            path_base: path_base.clone(),
                            workspace_root: workspace_root.clone(),
                            guidance: t::switch_guidance(&ctx.resources.lang, &path),
                        },
                    )
                }
                Err(e) => TypedToolResult::error(t::switch_error(&ctx.resources.lang, e)),
            }
        } else {
            // 恢复上一上下文
            match ctx.workspace_control().exit() {
                Ok(prev) => {
                    let path_base = ctx.workspace_read().current_path_base();
                    let workspace_root = ctx.workspace_read().current_workspace_root();
                    let branch = current_branch(ctx);
                    let headline = if ctx.resources.lang == "zh" {
                        format!("已退出 worktree，恢复到：{}", prev.path_base.display())
                    } else {
                        format!("Exited worktree, restored to: {}", prev.path_base.display())
                    };
                    TypedToolResult::success(
                        headline.clone(),
                        ExitWorktreeResult {
                            branch: branch.clone(),
                            path_base: path_base.clone(),
                            workspace_root: workspace_root.clone(),
                            guidance: t::exit_guidance(&ctx.resources.lang, &prev.path_base),
                        },
                    )
                }
                Err(e) => TypedToolResult::error(t::exit_error(&ctx.resources.lang, e)),
            }
        }
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use share::session_types::WorktreeKind;

    /// Build an isolated real-git command without mutating process-wide env/cwd.
    fn test_git_command(path: &Path) -> std::process::Command {
        let hooks = path.join(".aemeath-test-empty-hooks");
        std::fs::create_dir_all(&hooks).expect("创建空 hooks 目录失败");
        let mut command = std::process::Command::new("git");
        command
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                path.join(".aemeath-test-unavailable-global-config"),
            )
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "core.hooksPath")
            .env("GIT_CONFIG_VALUE_0", hooks);
        command
    }
    /// 初始化最小 main-branch git 仓库：
    /// `git init --initial-branch=main` + 本地 user + seed commit。
    ///
    /// 与 production `worktree_add` 协议对齐：默认 base = `main`，
    /// 且新分支必须从一个已存在的 commit 派生。
    fn init_main_repo(path: &Path) {
        let status = test_git_command(path)
            .args(["init", "--initial-branch=main"])
            .current_dir(path)
            .status()
            .expect("git init spawn 失败（git 是否已安装？）");
        assert!(status.success(), "git init 退出码非 0");

        let config_local = |key: &str, value: &str| {
            let status = test_git_command(path)
                .args(["config", "--local", key, value])
                .current_dir(path)
                .status()
                .unwrap_or_else(|e| panic!("git config {key} spawn 失败: {e}"));
            assert!(status.success(), "git config {key} 退出码非 0");
        };
        config_local("user.name", "Worktree Tool Test");
        config_local("user.email", "worktree-tool-test@example.invalid");
        config_local("commit.gpgsign", "false");
        config_local("tag.gpgsign", "false");
        config_local("core.hooksPath", ".aemeath-test-empty-hooks");

        std::fs::write(path.join("seed.txt"), "seed\n").unwrap();
        let status = test_git_command(path)
            .args(["add", "seed.txt"])
            .current_dir(path)
            .status()
            .expect("git add spawn 失败");
        assert!(status.success(), "git add 退出码非 0");

        let status = test_git_command(path)
            .args(["commit", "-m", "seed"])
            .current_dir(path)
            .status()
            .expect("git commit spawn 失败");
        assert!(status.success(), "git commit 退出码非 0");
    }

    #[test]
    fn test_enter_worktree_schema() {
        let tool = EnterWorktreeTool;
        let schema = tool.input_schema();

        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["path"]["type"], "string");
        assert_eq!(schema["properties"]["branch"]["type"], "string");
        assert!(schema["properties"].get("base").is_none());
        // 全 Option 字段：生成的 schema 不含 required 键（或为空数组）。
        assert!(schema
            .get("required")
            .and_then(|v| v.as_array())
            .is_none_or(|arr| arr.is_empty()));
    }

    #[test]
    fn test_exit_worktree_schema() {
        let tool = ExitWorktreeTool;
        let schema = tool.input_schema();

        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["path"]["type"], "string");
        // 全 Option 字段：生成的 schema 不含 required 键（或为空数组）。
        assert!(schema
            .get("required")
            .and_then(|v| v.as_array())
            .is_none_or(|arr| arr.is_empty()));
    }

    #[test]
    fn test_enter_worktree_name() {
        assert_eq!(EnterWorktreeTool.name(), "EnterWorktree");
    }

    #[test]
    fn test_exit_worktree_name() {
        assert_eq!(ExitWorktreeTool.name(), "ExitWorktree");
    }

    #[test]
    fn test_enter_worktree_not_read_only() {
        let tool = EnterWorktreeTool;
        assert!(!tool.is_read_only());
    }

    #[test]
    fn test_exit_worktree_not_read_only() {
        let tool = ExitWorktreeTool;
        assert!(!tool.is_read_only());
    }

    #[test]
    fn test_enter_worktree_not_concurrency_safe() {
        let tool = EnterWorktreeTool;
        assert!(!tool.is_concurrency_safe());
    }

    #[test]
    fn test_exit_worktree_not_concurrency_safe() {
        let tool = ExitWorktreeTool;
        assert!(!tool.is_concurrency_safe());
    }

    #[test]
    fn test_workspace_context_payload_includes_path_base_and_workspace_root() {
        let payload = workspace_context_payload(
            "已进入 worktree：/repo/.worktrees/feature",
            "feature",
            Path::new("/repo/.worktrees/feature/subdir"),
            Path::new("/repo/.worktrees/feature"),
            "zh",
        );

        assert_eq!(payload.branch, "feature");
        assert_eq!(
            payload.path_base,
            Path::new("/repo/.worktrees/feature/subdir")
        );
        assert_eq!(
            payload.workspace_root,
            Path::new("/repo/.worktrees/feature")
        );
        // guidance 区分 path_base/workspace_root 语义（#413）
        assert!(payload.guidance.contains("path_base"));
        assert!(payload.guidance.contains("workspace_root"));
    }

    #[test]
    fn test_workspace_context_payload_guidance_bilingual() {
        let zh = workspace_context_payload("headline", "b", Path::new("/p"), Path::new("/p"), "zh");
        let en = workspace_context_payload("headline", "b", Path::new("/p"), Path::new("/p"), "en");
        assert!(zh.guidance.contains("相对路径"));
        assert!(en.guidance.contains("relative paths"));
    }

    // ── Enter / Exit / Switch call() 失败行为集成测试 ────────────────
    //
    // 通过 `TypedTool::call()` 端到端验证失败路径，而非仅测 helper。
    // `PersistedWorkspaceContext` 已 derive PartialEq/Eq，可直接逐字段 assert_eq!。

    /// 构造最小 `ToolExecutionContext`，workspace 指向 `cwd`，使用默认 ToolResources。
    fn build_ctx(cwd: PathBuf) -> ToolExecutionContext {
        use crate::domain::ToolResources;
        use std::collections::HashSet;
        use std::sync::{Arc, Mutex};
        use tokio::sync::Semaphore;
        use tokio_util::sync::CancellationToken;

        ToolExecutionContext {
            workspace: project::wire_production_workspace(cwd)
                .expect("workspace 初始化成功")
                .into_views(),
            run_id: "01900000-0000-7000-8000-000000000001".to_string(),
            cancel: CancellationToken::new(),
            read_files: Arc::new(Mutex::new(HashSet::new())),
            resources: ToolResources {
                agent_runner: None,
                registry: None,
                memory: std::sync::Arc::new(memory::NoOpMemory),
                memory_config: share::config::MemoryConfig::default(),
                lang: "en".to_string(),
                allow_all: false,
            },
            session_reminders: None,
            plan_mode: None,
            max_tool_concurrency: 4,
            max_agent_concurrency: 4,
            agent_semaphore: Arc::new(Semaphore::new(4)),
            progress_tx: None,
            parent_session_id: None,
        }
    }

    /// 在 `path` 中执行 `git init`，建立最小 git 仓库。
    fn init_git(path: &Path) {
        let status = test_git_command(path)
            .args(["init", "--initial-branch=main"])
            .current_dir(path)
            .status()
            .expect("git init 失败（git 是否已安装？）");
        assert!(status.success(), "git init 退出码非 0");
        let status = test_git_command(path)
            .args([
                "config",
                "--local",
                "core.hooksPath",
                ".aemeath-test-empty-hooks",
            ])
            .current_dir(path)
            .status()
            .expect("git config core.hooksPath 失败");
        assert!(status.success(), "git config core.hooksPath 退出码非 0");
    }

    /// NonGit 目录调用 EnterWorktree 必须返回 error，
    /// 且 WorkspacePersist snapshot 完全不变。
    #[tokio::test]
    async fn enter_worktree_nongit_returns_error_and_snapshot_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = build_ctx(tmp.path().to_path_buf());

        let before = ctx.workspace.persist().snapshot();

        let result = EnterWorktreeTool
            .call(
                serde_json::json!({ "path": tmp.path().display().to_string() }),
                &ctx,
            )
            .await;

        assert!(result.is_error, "NonGit Enter 必须返回 error");
        assert!(
            result.data.is_none(),
            "失败时不应返回 data，got: {:?}",
            result.data
        );

        let after = ctx.workspace.persist().snapshot();
        assert_eq!(before, after, "Enter 失败后 snapshot 必须完全不变");
    }

    /// NonGit 目录调用 ExitWorktree（无 path）必须返回 Unsupported error，
    /// 且 snapshot 不变。
    #[tokio::test]
    async fn exit_worktree_nongit_returns_unsupported_and_snapshot_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = build_ctx(tmp.path().to_path_buf());

        let before = ctx.workspace.persist().snapshot();

        let result = ExitWorktreeTool.call(serde_json::json!({}), &ctx).await;

        assert!(result.is_error, "NonGit Exit 必须返回 error");
        assert!(
            result.data.is_none(),
            "失败时不应返回 data，got: {:?}",
            result.data
        );

        let after = ctx.workspace.persist().snapshot();
        assert_eq!(before, after, "Exit 失败后 snapshot 必须完全不变");
    }

    /// Git primary workspace 空栈调用 ExitWorktree 必须返回 EmptyStack error，
    /// 且 snapshot 不变。
    #[tokio::test]
    async fn exit_worktree_git_empty_stack_returns_error_and_snapshot_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        init_main_repo(tmp.path());
        let ctx = build_ctx(tmp.path().to_path_buf());
        let before = ctx.workspace.persist().snapshot();

        let result = ExitWorktreeTool.call(serde_json::json!({}), &ctx).await;

        assert!(result.is_error, "Git primary 空栈 Exit 必须返回 error");
        assert!(result.data.is_none(), "失败时不应返回 data");
        assert!(
            result.text.contains("上下文栈为空"),
            "错误应表达空栈语义: {}",
            result.text
        );
        assert_eq!(
            before,
            ctx.workspace.persist().snapshot(),
            "EmptyStack 失败后 snapshot 必须完全不变"
        );
    }

    /// Git workspace 中 Switch（ExitWorktree{path}）到另一 repo 必须返回 error，
    /// 且 snapshot 不变。
    #[tokio::test]
    async fn exit_worktree_switch_cross_repo_returns_error_and_snapshot_unchanged() {
        let repo_a = tempfile::tempdir().unwrap();
        let repo_b = tempfile::tempdir().unwrap();
        init_git(repo_a.path());
        init_git(repo_b.path());

        let ctx = build_ctx(repo_a.path().to_path_buf());

        let before = ctx.workspace.persist().snapshot();

        let result = ExitWorktreeTool
            .call(
                serde_json::json!({
                    "path": repo_b.path().display().to_string(),
                }),
                &ctx,
            )
            .await;

        assert!(result.is_error, "跨仓库 switch 必须返回 error");
        assert!(
            result.data.is_none(),
            "失败时不应返回 data，got: {:?}",
            result.data
        );

        let after = ctx.workspace.persist().snapshot();
        assert_eq!(before, after, "Switch 失败后 snapshot 必须完全不变");
    }

    // ── Enter / Exit / Switch call() 成功场景集成测试（真实 git） ────────────
    //
    // 通过 `TypedTool::call()` 端到端验证真实 git 成功路径：
    //   1) EnterWorktree{branch}：用真实 `git worktree add` 创建 linked worktree，
    //      断言 result data 字段与 mutation 后 `ctx.workspace.read()` 一致，
    //      `WorkspacePersist::snapshot()` 的 `context_stack.len() == 1` 且 `worktree_kind == Linked`。
    //   2) ExitWorktree{}：弹出栈帧回到 primary，断言 data/read/snapshot 回到原根、
    //      stack 空、kind == Primary。
    //   3) EnterWorktree{branch} + ExitWorktree{path=primary 子目录}：验证
    //      switch 不压栈、data/read/root/kind 切到新路径。
    //
    // 全部使用 `git init --initial-branch=main` + 本地 user + seed commit，保证
    // production `worktree_add` (`-b <new> <base=main>`) 能成功派生新分支。

    /// EnterWorktreeTool 用 branch 创建 linked worktree：
    /// data 字段与 read 一致，snapshot context_stack len=1、kind=Linked。
    #[tokio::test]
    async fn enter_worktree_with_branch_creates_linked_and_consistent_state() {
        let tmp = tempfile::tempdir().unwrap();
        init_main_repo(tmp.path());
        let ctx = build_ctx(tmp.path().to_path_buf());
        let main_canonical = tmp.path().canonicalize().unwrap();
        let expected_wt = main_canonical.join(".worktrees").join("feature-x");

        // Pre-state：primary、空栈
        let before = ctx.workspace.persist().snapshot();
        assert_eq!(before.worktree_kind, WorktreeKind::Primary);
        assert!(before.context_stack.is_empty());
        assert_eq!(before.path_base, main_canonical.display().to_string());
        assert_eq!(before.workspace_root, main_canonical.display().to_string());

        let result = EnterWorktreeTool
            .call(serde_json::json!({ "branch": "feature-x" }), &ctx)
            .await;

        assert!(!result.is_error, "EnterWorktree 必须成功: {}", result.text);
        let data = result.data.expect("成功必须返回 data");

        // data.path_base / workspace_root / branch 与 mutation 后 read 一致
        let read = ctx.workspace.read();
        assert_eq!(data.path_base, read.current_path_base());
        assert_eq!(data.workspace_root, read.current_workspace_root());
        let read_branch = read
            .current_branch()
            .expect("current_branch ok")
            .expect("linked worktree 必须报告新分支名");
        assert_eq!(data.branch, read_branch);

        // data 字段应为期望值
        assert_eq!(data.path_base, expected_wt);
        assert_eq!(data.workspace_root, expected_wt);
        assert_eq!(data.branch, "feature-x");

        // snapshot：context_stack len=1、kind=Linked，path 指向 linked worktree
        let snapshot = ctx.workspace.persist().snapshot();
        assert_eq!(
            snapshot.context_stack.len(),
            1,
            "Enter 后 context_stack 应含 1 帧（被压入的 primary）"
        );
        assert_eq!(
            snapshot.worktree_kind,
            WorktreeKind::Linked,
            "Enter 后顶层 kind 应为 Linked"
        );
        assert_eq!(snapshot.path_base, expected_wt.display().to_string());
        assert_eq!(snapshot.workspace_root, expected_wt.display().to_string());
        // 栈帧应是 primary，根指向原仓库根
        assert_eq!(
            snapshot.context_stack[0].worktree_kind,
            WorktreeKind::Primary
        );
        assert_eq!(
            snapshot.context_stack[0].path_base,
            main_canonical.display().to_string()
        );
        assert_eq!(
            snapshot.context_stack[0].workspace_root,
            main_canonical.display().to_string()
        );

        // 不应触碰初始 pre-state（防止 snapshot 不一致回归）。
        assert_eq!(before.path_base, main_canonical.display().to_string());
    }

    /// ExitWorktreeTool 空输入 pop 回 primary：data/read/snapshot 回到原根，
    /// stack 空、kind=Primary。
    #[tokio::test]
    async fn exit_worktree_empty_input_restores_primary_and_pops_stack() {
        let tmp = tempfile::tempdir().unwrap();
        init_main_repo(tmp.path());
        let ctx = build_ctx(tmp.path().to_path_buf());
        let main_canonical = tmp.path().canonicalize().unwrap();

        // 先 Enter 推 linked worktree
        let enter = EnterWorktreeTool
            .call(serde_json::json!({ "branch": "to-exit" }), &ctx)
            .await;
        assert!(
            !enter.is_error,
            "Enter 必须成功以建立 pop 场景: {}",
            enter.text
        );

        // 中间状态：linked、栈长=1（后续 pop 的回归点）
        let pre_exit = ctx.workspace.persist().snapshot();
        assert_eq!(pre_exit.worktree_kind, WorktreeKind::Linked);
        assert_eq!(pre_exit.context_stack.len(), 1);
        let linked_root = main_canonical.join(".worktrees").join("to-exit");
        assert_eq!(pre_exit.path_base, linked_root.display().to_string());
        assert_eq!(pre_exit.workspace_root, linked_root.display().to_string());

        // Exit 空输入 pop 回 primary
        let result = ExitWorktreeTool.call(serde_json::json!({}), &ctx).await;

        assert!(!result.is_error, "ExitWorktree 必须成功: {}", result.text);
        let data = result.data.expect("成功必须返回 data");

        // data.path_base / workspace_root / branch 与 read 一致且回到主根
        let read = ctx.workspace.read();
        assert_eq!(data.path_base, read.current_path_base());
        assert_eq!(data.workspace_root, read.current_workspace_root());
        assert_eq!(data.path_base, main_canonical);
        assert_eq!(data.workspace_root, main_canonical);
        let read_branch = read
            .current_branch()
            .expect("current_branch ok")
            .expect("pop 回 primary 后必须回到 main 分支");
        assert_eq!(data.branch, read_branch);
        assert_eq!(data.branch, "main");

        // snapshot 回原根、stack 空、kind=Primary
        let snapshot = ctx.workspace.persist().snapshot();
        assert_eq!(snapshot.path_base, main_canonical.display().to_string());
        assert_eq!(
            snapshot.workspace_root,
            main_canonical.display().to_string()
        );
        assert!(
            snapshot.context_stack.is_empty(),
            "pop 后 context_stack 必须为空"
        );
        assert_eq!(snapshot.worktree_kind, WorktreeKind::Primary);

        // 中间状态确实被改写过（防止 pop 是 no-op 回归）
        assert_ne!(pre_exit.workspace_root, snapshot.workspace_root);
    }

    /// 单独创建 linked worktree 后 ExitWorktreeTool path 输入 switch：
    /// 验证不压栈、data/read/root/kind 切到新路径。
    #[tokio::test]
    async fn exit_worktree_with_path_switches_without_pushing_stack() {
        let tmp = tempfile::tempdir().unwrap();
        init_main_repo(tmp.path());
        // 在主仓库内建另一个目录（primary 子目录），作为 switch 目标。
        // switch_to 不创建新 worktree，只换 path_base/workspace_root。
        let other = tmp.path().join("other-subdir");
        std::fs::create_dir_all(&other).unwrap();

        let ctx = build_ctx(tmp.path().to_path_buf());
        let main_canonical = tmp.path().canonicalize().unwrap();
        let other_canonical = other.canonicalize().unwrap();

        // 先 Enter 推 linked worktree（栈长=1）
        let enter = EnterWorktreeTool
            .call(serde_json::json!({ "branch": "switch-test" }), &ctx)
            .await;
        assert!(
            !enter.is_error,
            "Enter 必须成功以建立 switch 场景: {}",
            enter.text
        );
        let stack_after_enter = ctx.workspace.persist().snapshot().context_stack.len();
        assert_eq!(
            stack_after_enter, 1,
            "Enter 后栈长应=1（作为 switch 不压栈断言的基准）"
        );

        // ExitWorktree{path=other_subdir} 直接 switch 到 primary 子目录
        let result = ExitWorktreeTool
            .call(
                serde_json::json!({ "path": other.display().to_string() }),
                &ctx,
            )
            .await;

        assert!(
            !result.is_error,
            "ExitWorktree path switch 必须成功: {}",
            result.text
        );
        let data = result.data.expect("成功必须返回 data");

        // data.path_base / workspace_root / branch 与 read 一致
        let read = ctx.workspace.read();
        assert_eq!(data.path_base, read.current_path_base());
        assert_eq!(data.workspace_root, read.current_workspace_root());
        assert_eq!(data.path_base, other_canonical);
        assert_eq!(data.workspace_root, main_canonical);
        let read_branch = read
            .current_branch()
            .expect("current_branch ok")
            .expect("switch 到 primary 子目录后分支仍为 main");
        assert_eq!(data.branch, read_branch);
        assert_eq!(data.branch, "main");

        // 不压栈：栈长仍=1（Enter 推入的那一帧保留）
        let snapshot = ctx.workspace.persist().snapshot();
        assert_eq!(
            snapshot.context_stack.len(),
            1,
            "switch 不应压栈，栈长应保持 Enter 后的 1"
        );
        // 切回 primary，path_base 落到 other-subdir，workspace_root 仍是主仓库根
        assert_eq!(snapshot.worktree_kind, WorktreeKind::Primary);
        assert_eq!(snapshot.path_base, other_canonical.display().to_string());
        assert_eq!(
            snapshot.workspace_root,
            main_canonical.display().to_string()
        );
        // 栈帧依旧指向被 Enter 推入时的 primary 状态
        assert_eq!(
            snapshot.context_stack[0].worktree_kind,
            WorktreeKind::Primary
        );
        assert_eq!(
            snapshot.context_stack[0].workspace_root,
            main_canonical.display().to_string()
        );
    }

    /// 真实 Git 多 linked worktree 场景：
    /// 1) EnterWorktreeTool 创建 linked A（栈1）。
    /// 2) 通过测试 git command 直接从 main 创建 linked B（独立分支，真实 git worktree）。
    /// 3) ExitWorktreeTool { path: linked B } switch 到 B。
    /// 断言：成功、data/read/snapshot 指向 B、kind=Linked、
    /// context_stack 仍为1（switch 不压栈）。
    #[tokio::test]
    async fn exit_worktree_switch_to_another_linked_worktree_does_not_push_stack() {
        let tmp = tempfile::tempdir().unwrap();
        init_main_repo(tmp.path());
        let main_canonical = tmp.path().canonicalize().unwrap();

        let ctx = build_ctx(tmp.path().to_path_buf());

        // 1) EnterWorktreeTool 创建 linked A（栈1）
        let enter = EnterWorktreeTool
            .call(serde_json::json!({ "branch": "linked-a" }), &ctx)
            .await;
        assert!(!enter.is_error, "Enter linked-a 必须成功: {}", enter.text);
        let linked_a = main_canonical.join(".worktrees").join("linked-a");
        let snapshot_after_enter = ctx.workspace.persist().snapshot();
        assert_eq!(
            snapshot_after_enter.worktree_kind,
            WorktreeKind::Linked,
            "Enter 后顶层 kind 应为 Linked"
        );
        assert_eq!(
            snapshot_after_enter.context_stack.len(),
            1,
            "Enter 后 context_stack 应含 1 帧"
        );
        assert_eq!(
            snapshot_after_enter.path_base,
            linked_a.display().to_string()
        );

        // 2) 通过测试 git command 直接从 main 创建 linked B（独立分支）
        let linked_b_raw = tmp.path().join(".worktrees").join("linked-b");
        let status = test_git_command(tmp.path())
            .args([
                "worktree",
                "add",
                "-b",
                "linked-b",
                linked_b_raw.to_str().unwrap(),
            ])
            .current_dir(tmp.path())
            .status()
            .expect("git worktree add linked-b spawn 失败");
        assert!(status.success(), "git worktree add linked-b 退出码非 0");
        let linked_b = main_canonical.join(".worktrees").join("linked-b");

        // 3) ExitWorktreeTool { path: linked B } switch
        let result = ExitWorktreeTool
            .call(
                serde_json::json!({ "path": linked_b_raw.display().to_string() }),
                &ctx,
            )
            .await;

        assert!(
            !result.is_error,
            "ExitWorktree path switch 到 linked B 必须成功: {}",
            result.text
        );
        let data = result.data.expect("成功必须返回 data");

        // data.path_base / workspace_root / branch 与 read 一致且指向 B
        let read = ctx.workspace.read();
        assert_eq!(data.path_base, read.current_path_base());
        assert_eq!(data.workspace_root, read.current_workspace_root());
        assert_eq!(data.path_base, linked_b);
        assert_eq!(data.workspace_root, linked_b);
        let read_branch = read
            .current_branch()
            .expect("current_branch ok")
            .expect("linked worktree 必须报告新分支名");
        assert_eq!(data.branch, read_branch);
        assert_eq!(data.branch, "linked-b");

        // snapshot 指向 B、kind=Linked
        let snapshot = ctx.workspace.persist().snapshot();
        assert_eq!(
            snapshot.path_base,
            linked_b.display().to_string(),
            "snapshot path_base 应指向 linked B"
        );
        assert_eq!(
            snapshot.workspace_root,
            linked_b.display().to_string(),
            "snapshot workspace_root 应指向 linked B"
        );
        assert_eq!(
            snapshot.worktree_kind,
            WorktreeKind::Linked,
            "switch 到 linked worktree 后顶层 kind 应为 Linked"
        );

        // switch 不压栈：context_stack 仍为 1（Enter 推入的 primary 帧）
        assert_eq!(
            snapshot.context_stack.len(),
            1,
            "switch 不应压栈，栈长应保持 Enter 后的 1"
        );
        // 栈帧依旧是 Enter 推入的 primary 状态
        assert_eq!(
            snapshot.context_stack[0].worktree_kind,
            WorktreeKind::Primary
        );
        assert_eq!(
            snapshot.context_stack[0].workspace_root,
            main_canonical.display().to_string()
        );
    }
}
