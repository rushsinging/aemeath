use super::*;
use share::session_types::WorktreeKind;
use std::sync::Mutex;

type EnterArgs = (Option<PathBuf>, Option<String>, Option<String>);

#[derive(Default)]
struct RecordingWorkspaceControl {
    enter_args: Mutex<Option<EnterArgs>>,
}

impl WorkspaceControl for RecordingWorkspaceControl {
    fn change_directory(&self, _path: PathBuf) -> Result<(), share::error::DomainError> {
        Err(share::error::DomainError::invalid(
            "tools",
            "NonGit 环境不支持 worktree 操作",
        ))
    }

    fn enter(
        &self,
        path: Option<PathBuf>,
        branch: Option<String>,
        base: Option<String>,
    ) -> Result<project::WorkspaceData, share::error::DomainError> {
        *self.enter_args.lock().expect("recording control lock") = Some((path, branch, base));
        Err(share::error::DomainError::invalid(
            "tools",
            "NonGit 环境不支持 worktree 操作",
        ))
    }

    fn exit(&self) -> Result<project::WorkspaceData, share::error::DomainError> {
        Err(share::error::DomainError::invalid(
            "tools",
            "NonGit 环境不支持 worktree 操作",
        ))
    }
}

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

fn run_git(path: &Path, args: &[&str]) {
    let status = test_git_command(path)
        .args(args)
        .current_dir(path)
        .status()
        .unwrap_or_else(|error| panic!("git {args:?} spawn 失败: {error}"));
    assert!(status.success(), "git {args:?} 退出码非 0");
}

fn git_stdout(path: &Path, args: &[&str]) -> String {
    let output = test_git_command(path)
        .args(args)
        .current_dir(path)
        .output()
        .unwrap_or_else(|error| panic!("git {args:?} spawn 失败: {error}"));
    assert!(
        output.status.success(),
        "git {args:?} 退出码非 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git 输出必须是 UTF-8")
        .trim()
        .to_string()
}

fn enter_tool(ctx: &ToolExecutionContext) -> EnterWorktreeTool {
    EnterWorktreeTool {
        control: crate::adapters::test_support_tests::production_workspace_control(ctx),
    }
}
fn exit_tool(ctx: &ToolExecutionContext) -> ExitWorktreeTool {
    ExitWorktreeTool {
        control: crate::adapters::test_support_tests::production_workspace_control(ctx),
    }
}

fn metadata_tools() -> (
    tempfile::TempDir,
    ToolExecutionContext,
    EnterWorktreeTool,
    ExitWorktreeTool,
) {
    let temp = tempfile::tempdir().expect("temp workspace");
    let ctx = crate::adapters::test_support_tests::production_execution_context(
        temp.path().to_path_buf(),
    );
    let enter = enter_tool(&ctx);
    let exit = exit_tool(&ctx);
    (temp, ctx, enter, exit)
}

#[test]
fn test_enter_worktree_schema() {
    let (_temp, _ctx, tool, _) = metadata_tools();
    let schema = tool.input_schema();

    assert_eq!(schema["type"], "object");
    assert_eq!(schema["properties"]["path"]["type"], "string");
    assert_eq!(schema["properties"]["branch"]["type"], "string");
    assert_eq!(schema["properties"]["base"]["type"], "string");
    assert_eq!(schema["properties"]["base"]["nullable"], true);
    let path_description = schema["properties"]["path"]["description"]
        .as_str()
        .expect("path description");
    assert!(path_description.contains("必须省略"));
    assert!(path_description.contains("禁止传空字符串"));
    let base_description = schema["properties"]["base"]["description"]
        .as_str()
        .expect("base description");
    assert!(base_description.contains("默认 main"));
    // 全 Option 字段：生成的 schema 不含 required 键（或为空数组）。
    assert!(schema
        .get("required")
        .and_then(|v| v.as_array())
        .is_none_or(|arr| arr.is_empty()));
}

#[tokio::test]
async fn enter_worktree_normalizes_blank_path_and_preserves_base_for_project() {
    let temp = tempfile::tempdir().expect("temp workspace");
    let ctx = build_ctx(temp.path().to_path_buf());
    let control = Arc::new(RecordingWorkspaceControl::default());
    let tool = EnterWorktreeTool {
        control: control.clone(),
    };

    let _result = tool
        .call(
            serde_json::json!({
                "branch": "fix/example",
                "path": " \t\n",
                "base": "",
            }),
            &ctx,
        )
        .await;

    assert_eq!(
        *control.enter_args.lock().expect("recording control lock"),
        Some((None, Some("fix/example".into()), Some(String::new())))
    );
}

#[test]
fn test_exit_worktree_schema() {
    let (_temp, _ctx, _, tool) = metadata_tools();
    let schema = tool.input_schema();

    assert_eq!(schema["type"], "object");
    // ExitWorktree 无输入字段：path 参数已移除，输入 schema 必须为空。
    assert!(
        schema
            .get("properties")
            .and_then(|v| v.as_object())
            .is_none_or(|props| props.is_empty()),
        "ExitWorktree 不应声明任何输入字段，got: {schema}"
    );
    // 无字段：不含 required 键（或为空数组）。
    assert!(schema
        .get("required")
        .and_then(|v| v.as_array())
        .is_none_or(|arr| arr.is_empty()));
}

#[test]
fn test_enter_worktree_name() {
    let (_temp, _ctx, tool, _) = metadata_tools();
    assert_eq!(tool.name(), "EnterWorktree");
}

#[test]
fn test_exit_worktree_name() {
    let (_temp, _ctx, _, tool) = metadata_tools();
    assert_eq!(tool.name(), "ExitWorktree");
}

#[test]
fn test_enter_worktree_not_read_only() {
    let (_temp, _ctx, tool, _) = metadata_tools();
    assert!(!tool.is_read_only());
}

#[test]
fn test_exit_worktree_not_read_only() {
    let (_temp, _ctx, _, tool) = metadata_tools();
    assert!(!tool.is_read_only());
}

#[test]
fn test_enter_worktree_not_concurrency_safe() {
    let (_temp, _ctx, tool, _) = metadata_tools();
    assert!(!tool.is_concurrency_safe());
}

#[test]
fn test_exit_worktree_not_concurrency_safe() {
    let (_temp, _ctx, _, tool) = metadata_tools();
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
    crate::adapters::test_support_tests::production_execution_context(cwd)
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
/// 且 WorkspaceWriter snapshot 完全不变。
#[tokio::test]
async fn enter_worktree_nongit_returns_error_and_snapshot_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = build_ctx(tmp.path().to_path_buf());

    let before = crate::adapters::test_support_tests::production_workspace_persist(&ctx).snapshot();

    let result = enter_tool(&ctx)
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

    let after = crate::adapters::test_support_tests::production_workspace_persist(&ctx).snapshot();
    assert_eq!(before, after, "Enter 失败后 snapshot 必须完全不变");
}

/// NonGit 目录调用 ExitWorktree（无 path）必须返回 Unsupported error，
/// 且 snapshot 不变。
#[tokio::test]
async fn exit_worktree_nongit_returns_unsupported_and_snapshot_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = build_ctx(tmp.path().to_path_buf());

    let before = crate::adapters::test_support_tests::production_workspace_persist(&ctx).snapshot();

    let result = exit_tool(&ctx).call(serde_json::json!({}), &ctx).await;

    assert!(result.is_error, "NonGit Exit 必须返回 error");
    assert!(
        result.data.is_none(),
        "失败时不应返回 data，got: {:?}",
        result.data
    );

    let after = crate::adapters::test_support_tests::production_workspace_persist(&ctx).snapshot();
    assert_eq!(before, after, "Exit 失败后 snapshot 必须完全不变");
}

/// Git primary workspace 空栈调用 ExitWorktree 必须返回 EmptyStack error，
/// 且 snapshot 不变。
#[tokio::test]
async fn exit_worktree_git_empty_stack_returns_error_and_snapshot_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    init_main_repo(tmp.path());
    let ctx = build_ctx(tmp.path().to_path_buf());
    let before = crate::adapters::test_support_tests::production_workspace_persist(&ctx).snapshot();

    let result = exit_tool(&ctx).call(serde_json::json!({}), &ctx).await;

    assert!(result.is_error, "Git primary 空栈 Exit 必须返回 error");
    assert!(result.data.is_none(), "失败时不应返回 data");
    assert!(
        result.text.contains("上下文栈为空"),
        "错误应表达空栈语义: {}",
        result.text
    );
    assert_eq!(
        before,
        crate::adapters::test_support_tests::production_workspace_persist(&ctx).snapshot(),
        "EmptyStack 失败后 snapshot 必须完全不变"
    );
}

/// 带 `path` 输入的 ExitWorktree 必须被拒绝（path 参数已移除，
/// 防止误用直接切换旁路），且 snapshot 不变。
#[tokio::test]
async fn exit_worktree_with_path_input_is_rejected_and_state_unchanged() {
    let repo = tempfile::tempdir().unwrap();
    init_git(repo.path());

    let ctx = build_ctx(repo.path().to_path_buf());

    let before = crate::adapters::test_support_tests::production_workspace_persist(&ctx).snapshot();

    let result = exit_tool(&ctx)
        .call(
            serde_json::json!({
                "path": repo.path().display().to_string(),
            }),
            &ctx,
        )
        .await;

    assert!(result.is_error, "带 path 的 ExitWorktree 必须返回 error");
    assert!(
        result.data.is_none(),
        "失败时不应返回 data，got: {:?}",
        result.data
    );

    let after = crate::adapters::test_support_tests::production_workspace_persist(&ctx).snapshot();
    assert_eq!(before, after, "拒绝后 snapshot 必须完全不变");
}

// ── Enter / Exit call() 成功场景集成测试（真实 git） ────────────
//
// 通过 `TypedTool::call()` 端到端验证真实 git 成功路径：
//   1) EnterWorktree{branch}：用真实 `git worktree add` 创建 linked worktree，
//      断言 result data 字段与 mutation 后 `ctx.workspace_read()` 一致，
//      `WorkspaceWriter::snapshot()` 的 `context_stack.len() == 1` 且 `worktree_kind == Linked`。
//   2) ExitWorktree{}：弹出栈帧回到 primary，断言 data/read/snapshot 回到原根、
//      stack 空、kind == Primary。
//
// 全部使用 `git init --initial-branch=main` + 本地 user + seed commit，保证
// production `worktree_add` (`-b <new> <base=main>`) 能成功派生新分支。

/// EnterWorktreeTool 用 branch 创建 linked worktree：
/// data 字段与 read 一致，snapshot context_stack len=1、kind=Linked。
#[tokio::test]
async fn enter_worktree_with_branch_creates_linked_and_consistent_state() {
    let tmp = tempfile::tempdir().unwrap();
    init_main_repo(tmp.path());
    let main_commit = git_stdout(tmp.path(), &["rev-parse", "main"]);
    run_git(tmp.path(), &["switch", "-c", "caller/divergent"]);
    std::fs::write(tmp.path().join("caller-only.txt"), "caller-only\n").unwrap();
    run_git(tmp.path(), &["add", "caller-only.txt"]);
    run_git(tmp.path(), &["commit", "-m", "caller-only"]);
    let caller_commit = git_stdout(tmp.path(), &["rev-parse", "HEAD"]);
    assert_ne!(caller_commit, main_commit, "caller branch 必须领先 main");

    let ctx = build_ctx(tmp.path().to_path_buf());
    let main_canonical = tmp.path().canonicalize().unwrap();
    let expected_wt = main_canonical
        .join(".worktrees")
        .join(
            main_canonical
                .file_name()
                .unwrap()
                .to_string_lossy()
                .trim_matches(|c| matches!(c, '.' | '_' | '-')),
        )
        .join("default-from-main");

    // Pre-state：primary、空栈
    let before = crate::adapters::test_support_tests::production_workspace_persist(&ctx).snapshot();
    assert_eq!(before.worktree_kind, WorktreeKind::Primary);
    assert!(before.context_stack.is_empty());
    assert_eq!(before.path_base, main_canonical.display().to_string());
    assert_eq!(before.workspace_root, main_canonical.display().to_string());

    let result = enter_tool(&ctx)
        .call(serde_json::json!({ "branch": "default/from-main" }), &ctx)
        .await;

    assert!(!result.is_error, "EnterWorktree 必须成功: {}", result.text);
    let data = result.data.expect("成功必须返回 data");

    // data.path_base / workspace_root / branch 与 mutation 后 read 一致
    let read = ctx.workspace_read();
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
    assert_eq!(data.branch, "default/from-main");
    let worktree_head = git_stdout(&expected_wt, &["rev-parse", "HEAD"]);
    assert_eq!(
        worktree_head, main_commit,
        "base 省略时新 worktree 必须从 main 创建"
    );
    assert_ne!(
        worktree_head, caller_commit,
        "base 省略时不得继承 divergent caller HEAD"
    );

    // snapshot：context_stack len=1、kind=Linked，path 指向 linked worktree
    let snapshot =
        crate::adapters::test_support_tests::production_workspace_persist(&ctx).snapshot();
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

#[tokio::test]
async fn enter_worktree_with_blank_path_and_base_derives_path_from_branch() {
    let tmp = tempfile::tempdir().unwrap();
    init_main_repo(tmp.path());
    let ctx = build_ctx(tmp.path().to_path_buf());
    let main_canonical = tmp.path().canonicalize().unwrap();
    let expected_wt = main_canonical
        .join(".worktrees")
        .join(
            main_canonical
                .file_name()
                .unwrap()
                .to_string_lossy()
                .trim_matches(|c| matches!(c, '.' | '_' | '-')),
        )
        .join("fix-example");

    let result = enter_tool(&ctx)
        .call(
            serde_json::json!({
                "branch": "fix/example",
                "path": "",
                "base": "",
            }),
            &ctx,
        )
        .await;

    assert!(!result.is_error, "EnterWorktree 必须成功: {}", result.text);
    let data = result.data.expect("成功必须返回 data");
    assert_eq!(data.path_base, expected_wt);
    assert_eq!(data.workspace_root, expected_wt);
    assert_eq!(data.branch, "fix/example");
}

#[tokio::test]
async fn enter_worktree_with_explicit_base_starts_at_base_commit() {
    let tmp = tempfile::tempdir().unwrap();
    init_main_repo(tmp.path());
    run_git(tmp.path(), &["switch", "-c", "base/unique"]);
    std::fs::write(tmp.path().join("base-only.txt"), "base-only\n").unwrap();
    run_git(tmp.path(), &["add", "base-only.txt"]);
    run_git(tmp.path(), &["commit", "-m", "base-only"]);
    let base_commit = git_stdout(tmp.path(), &["rev-parse", "HEAD"]);
    run_git(tmp.path(), &["switch", "main"]);
    assert_ne!(
        git_stdout(tmp.path(), &["rev-parse", "HEAD"]),
        base_commit,
        "测试前置条件：base ref 必须领先 main"
    );

    let ctx = build_ctx(tmp.path().to_path_buf());
    let main_canonical = tmp.path().canonicalize().unwrap();
    let expected_wt = main_canonical
        .join(".worktrees")
        .join(
            main_canonical
                .file_name()
                .unwrap()
                .to_string_lossy()
                .trim_matches(|c| matches!(c, '.' | '_' | '-')),
        )
        .join("from-explicit-base");
    let result = enter_tool(&ctx)
        .call(
            serde_json::json!({
                "branch": "from-explicit-base",
                "base": "base/unique",
            }),
            &ctx,
        )
        .await;

    assert!(!result.is_error, "EnterWorktree 必须成功: {}", result.text);
    let data = result.data.expect("成功必须返回 data");
    assert_eq!(data.path_base, expected_wt);
    assert_eq!(data.branch, "from-explicit-base");
    assert_eq!(
        git_stdout(&expected_wt, &["rev-parse", "HEAD"]),
        base_commit,
        "显式 base 必须跨 Tool → Project → Git 到达 worktree HEAD"
    );
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
    let enter = enter_tool(&ctx)
        .call(serde_json::json!({ "branch": "to-exit" }), &ctx)
        .await;
    assert!(
        !enter.is_error,
        "Enter 必须成功以建立 pop 场景: {}",
        enter.text
    );

    // 中间状态：linked、栈长=1（后续 pop 的回归点）
    let pre_exit =
        crate::adapters::test_support_tests::production_workspace_persist(&ctx).snapshot();
    assert_eq!(pre_exit.worktree_kind, WorktreeKind::Linked);
    assert_eq!(pre_exit.context_stack.len(), 1);
    let linked_root = main_canonical
        .join(".worktrees")
        .join(
            main_canonical
                .file_name()
                .unwrap()
                .to_string_lossy()
                .trim_matches(|c| matches!(c, '.' | '_' | '-')),
        )
        .join("to-exit");
    assert_eq!(pre_exit.path_base, linked_root.display().to_string());
    assert_eq!(pre_exit.workspace_root, linked_root.display().to_string());

    // Exit 空输入 pop 回 primary
    let result = exit_tool(&ctx).call(serde_json::json!({}), &ctx).await;

    assert!(!result.is_error, "ExitWorktree 必须成功: {}", result.text);
    let data = result.data.expect("成功必须返回 data");

    // data.path_base / workspace_root / branch 与 read 一致且回到主根
    let read = ctx.workspace_read();
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
    let snapshot =
        crate::adapters::test_support_tests::production_workspace_persist(&ctx).snapshot();
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
