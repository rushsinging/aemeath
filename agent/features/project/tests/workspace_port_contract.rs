//! Workspace 三 port（Reader / Control / Writer）对外契约测试。
//!
//! 契约对象是 `project` crate-root 发布的窄 trait 行为，与内部实现解耦：
//! 真实 git 仓库 fixture（隔离配置的 `git init` + seed commit）驱动
//! `wire_production_workspace` 生产装配，锁定跨实例往返、栈协议与
//! fail-closed 恢复语义。内部聚合重构（字段私有化、阶段拆分）MUST
//! 在本套件保持全绿的前提下进行。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use project::{wire_production_workspace, Workspace};
use share::session_types::PersistedWorkspaceContext;

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

/// 每测试独立、Drop 自清理的临时目录（与生产配置隔离的 git 环境）。
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let base = std::env::temp_dir();
        for _ in 0..100 {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("系统时钟必须晚于 Unix epoch")
                .as_nanos();
            let sequence = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!(
                "aemeath-pj-contract-{prefix}-{}-{nonce}-{sequence}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => {
                    return Self {
                        path: path.canonicalize().unwrap(),
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("创建临时目录 {} 失败：{error}", path.display()),
            }
        }
        panic!("无法分配唯一临时目录");
    }

    fn path(&self) -> &Path {
        &self.path
    }

    /// 初始化隔离于用户与系统 git 配置的最小仓库。
    fn init_git(&self) {
        let hooks = self.path.join("empty-hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        let status = isolated_git()
            .args(["init", "--initial-branch=main"])
            .current_dir(&self.path)
            .status()
            .expect("git init 执行失败（git 是否已安装？）");
        assert!(status.success(), "git init 退出码非 0");
    }

    /// 创建 main 的初始 commit，使 `git worktree add ... main` 可用。
    fn commit_seed(&self) {
        std::fs::write(self.path.join("seed.txt"), "seed\n").unwrap();
        for args in [
            vec!["add", "seed.txt"],
            vec![
                "-c",
                "user.name=Project Contract",
                "-c",
                "user.email=contract@example.invalid",
                "commit",
                "-m",
                "seed",
            ],
        ] {
            let status = isolated_git()
                .args(&args)
                .current_dir(&self.path)
                .status()
                .expect("git 命令执行失败");
            assert!(status.success(), "git {args:?} 退出码非 0");
        }
    }
}

/// 用户/系统配置全隔离的 git 命令句柄（hook、签名均禁用）。
fn isolated_git() -> Command {
    let mut command = Command::new("git");
    command
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null");
    command
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// 在真实 git 仓库上建立生产 wiring；worktree 根配置在仓库内（`.wt`），
/// 与全局 `~/.agents/worktrees` 隔离。
fn wired_git_repo(prefix: &str) -> (TempDir, Workspace) {
    let tmp = TempDir::new(prefix);
    tmp.init_git();
    tmp.commit_seed();
    let wiring = wire_production_workspace(tmp.path().to_path_buf(), Some(PathBuf::from(".wt")))
        .expect("git 仓库装配应成功");
    (tmp, wiring)
}

// ─── Writer 面：snapshot → prepare_restore → commit_restore 往返 ───

#[test]
fn writer_snapshot_restores_position_across_instances() {
    let (tmp, source) = wired_git_repo("roundtrip");
    source
        .control()
        .enter(None, Some("feat/contract-roundtrip".to_string()), None)
        .expect("进入 linked worktree 应成功");
    let snapshot = source.persist().snapshot();

    let target = wire_production_workspace(tmp.path().to_path_buf(), Some(PathBuf::from(".wt")))
        .expect("同一仓库第二装配应成功");
    let prepared = target
        .persist()
        .prepare_restore(&snapshot)
        .expect("合法快照必须能构造恢复令牌");
    target.persist().commit_restore(prepared);

    assert_eq!(
        target.read().current_path_base(),
        source.read().current_path_base()
    );
    assert_eq!(
        target.read().current_workspace_root(),
        source.read().current_workspace_root()
    );
    assert_eq!(target.read().workspace_id(), source.read().workspace_id());
    assert_eq!(
        target.persist().snapshot(),
        snapshot,
        "恢复后再快照必须与源一致"
    );
}

#[test]
fn writer_rejected_snapshot_keeps_live_state_unchanged() {
    let (tmp, wiring) = wired_git_repo("reject");
    let inside = tmp.path().join("inside");
    std::fs::create_dir_all(&inside).unwrap();
    wiring
        .control()
        .change_directory(inside.canonicalize().unwrap())
        .expect("合法子目录切换应成功");
    let before = wiring.persist().snapshot();
    let before_base = wiring.read().current_path_base();

    let mut invalid = before.clone();
    invalid.workspace_root = tmp.path().join("missing_root").display().to_string();
    let result = wiring.persist().prepare_restore(&invalid);

    assert!(
        result
            .as_ref()
            .err()
            .is_some_and(|error| error.message().contains("路径不存在")),
        "缺失 workspace_root 应返回结构化路径错误，got {result:?}"
    );
    assert_eq!(
        wiring.read().current_path_base(),
        before_base,
        "拒绝后 live 位置不得变化"
    );
    assert_eq!(
        wiring.persist().snapshot(),
        before,
        "拒绝后 live 快照不得变化"
    );
}

// ─── Control 面：change_directory / enter / exit 栈协议 ───

#[test]
fn control_change_directory_follows_inside_and_rejects_outside() {
    let (tmp, wiring) = wired_git_repo("cd");
    let inside = tmp.path().join("inner");
    std::fs::create_dir_all(&inside).unwrap();

    wiring
        .control()
        .change_directory(inside.canonicalize().unwrap())
        .expect("workspace 内目录切换应成功");
    assert_eq!(
        wiring.read().current_path_base(),
        inside.canonicalize().unwrap()
    );

    let outside = TempDir::new("cd-outside");
    let error = wiring
        .control()
        .change_directory(outside.path().to_path_buf())
        .expect_err("workspace 外目录必须被拒绝");
    assert!(
        error.message().starts_with("路径 "),
        "越界错误应为结构化中文消息，got {}",
        error.message()
    );
    assert_eq!(
        wiring.read().current_path_base(),
        inside.canonicalize().unwrap(),
        "拒绝后位置不得漂移"
    );
}

#[test]
fn control_enter_exit_round_trips_stack_and_empty_exit_fails() {
    let (_tmp, wiring) = wired_git_repo("stack");
    let primary_base = wiring.read().current_path_base();
    let primary_root = wiring.read().current_workspace_root();
    assert!(!wiring.read().in_worktree(), "初始应为 Primary 非 worktree");

    let frame = wiring
        .control()
        .enter(None, Some("feat/contract-stack".to_string()), None)
        .expect("进入 linked worktree 应成功");
    assert!(
        wiring.read().in_worktree(),
        "enter 后必须处于 linked worktree"
    );
    assert_eq!(
        wiring.read().current_workspace_root(),
        wiring.read().current_path_base(),
        "linked worktree 的 root 与 base 应重合"
    );
    assert_eq!(frame.workspace_root, primary_root, "返回帧记录进入前 root");

    let exited = wiring.control().exit().expect("退栈应成功");
    assert_eq!(exited, frame, "exit 返回的帧应与 enter 一致");
    assert_eq!(wiring.read().current_path_base(), primary_base);
    assert_eq!(wiring.read().current_workspace_root(), primary_root);

    let error = wiring
        .control()
        .exit()
        .expect_err("空栈 exit 必须失败（fail-closed）");
    assert!(
        error.message().contains("栈"),
        "空栈错误应为结构化消息，got {}",
        error.message()
    );
}

// ─── Reader 面：身份稳定性与路径解析语义 ───

#[test]
fn reader_initial_cwd_and_identity_survive_worktree_switch() {
    let (_tmp, wiring) = wired_git_repo("identity");
    let initial_cwd = wiring.read().initial_cwd();

    wiring
        .control()
        .enter(None, Some("feat/contract-identity".to_string()), None)
        .expect("进入 linked worktree 应成功");

    assert_eq!(
        wiring.read().initial_cwd(),
        initial_cwd,
        "initial_cwd 是会话身份，enter 后不得变化"
    );
    assert!(
        wiring.read().project_identity().git_common_dir.is_some(),
        "git 仓库身份应包含 common dir"
    );
    assert!(!wiring.read().workspace_id().as_str().is_empty());

    let absolute = wiring.read().resolve(Path::new("/definitely/absolute"));
    assert_eq!(absolute, PathBuf::from("/definitely/absolute"));
    let relative = wiring.read().resolve(Path::new("rel/file.txt"));
    assert_eq!(
        relative,
        wiring.read().current_path_base().join("rel/file.txt")
    );
}

#[test]
fn reader_authorized_resolution_can_leave_workspace_root() {
    let (_tmp, wiring) = wired_git_repo("authorized");
    let outside = TempDir::new("authorized-outside");
    let target = outside.path().join("loose.txt");

    let error = wiring
        .read()
        .resolve_file_path(&target)
        .expect_err("未授权越界路径必须被拒绝");
    assert!(
        error.message().starts_with("路径 "),
        "越界错误应为结构化消息，got {}",
        error.message()
    );
    assert_eq!(
        wiring
            .read()
            .resolve_file_path_authorized(&target, true)
            .expect("显式授权必须放行 workspace 外路径"),
        target
    );
}

// ─── Writer 面：restore 令牌只读身份协议 ───

#[test]
fn writer_restore_token_exposes_identity_only() {
    let (_tmp, wiring) = wired_git_repo("token");
    let snapshot: PersistedWorkspaceContext = wiring.persist().snapshot();

    let prepared = wiring
        .persist()
        .prepare_restore(&snapshot)
        .expect("自洽快照应构造令牌");
    assert_eq!(
        prepared.project_identity(),
        &snapshot.project_identity,
        "令牌唯一只读 accessor 应暴露已校验身份"
    );
}
