//! 安装提交路径：安装根形态契约、暂存目录 RAII、canonical manifest 写入与原子 no-replace rename。
//!
//! 安装根本体契约（读/装两端同一判定，见 [`validate_install_root`] / [`ensure_root_directory`]）：
//! 只用 `symlink_metadata` 看 root 本体，NEVER 跟随链接；root 缺失时安装入口可安全
//! `create_dir_all`，但创建后必须复验最终 root 本体不是符号链接且是目录（`create_dir_all`
//! 会把既存的「指向目录的符号链接」当成功，且创建前后有竞态窗口）；root 已是符号链接/
//! 非目录时 typed 失败，**NEVER** 沿符号链接创建暂存或安装。
//!
//! 提交原语在可证明的 target（Linux / Android / macOS / iOS）上是
//! `rustix::fs::renameat_with(..., RenameFlags::NOREPLACE)`（safe API，无手工 syscall）：
//! 目标无论空目录还是非空目录一律返回冲突，**NEVER** 覆盖已存在的 final。
//! 其余 target 由 cfg 编译 fail-closed 分支（`ErrorKind::Unsupported`），
//! **NEVER** 回退会覆盖空目录的 `std::fs::rename`。
//!
//! 清理责任：正常失败/成功路径都在 `spawn_blocking` 内显式清理；
//! [`StagingGuard`] / [`StagingDirectory`] 的 `Drop` 只是**同步 best-effort 兜底**
//! （尽力同步清理元数据，失败仅记录日志、不重试），NEVER 作为常规清理手段——
//! 正常 application 错误路径应显式清理。进程中断（崩溃 / 取消）留下的 stale
//! 暂存目录本轮不清理，后续由启动 / 下载前 sweep 承接，`Drop` 只是同步兜底。
//!
//! unix 权限边界：生产 root 与暂存目录创建 mode `0700`（owner 专属），
//! 资产目标文件创建 mode `0600`（见同级 `fetch_http_destination.rs`）。

use std::path::{Path, PathBuf};

use crate::constants::{MODEL_MANIFEST_FILE_NAME, STAGING_ATTEMPT_LIMIT, STAGING_DIR_PREFIX};
use crate::domain::ModelManifest;
use crate::ports::{InstalledAssets, InvalidAssetKind};
use crate::state::STAGING_SEQUENCE;

use super::verify::{probe_final_revision, verify_installed_directory, FinalRevisionState};
use super::{ModelInstallError, PreparedStagedInstall, StagedInstallCommit};

/// 安装根本体形态（读路径与安装入口共享的同一契约判定）。
#[derive(Debug)]
pub(super) enum InstallRootState {
    /// root 不存在。
    Missing,
    /// root 是真实目录（本体非符号链接）。
    Directory,
    /// root 形态异常（符号链接 / 非目录 / 不可探测），带中文 detail。
    Invalid {
        /// 是否因探测即 IO 失败（→ 读路径归为 `Unreadable`）。
        unprobeable: bool,
        /// 中文失败原因（指明具体形态问题与 root 路径）。
        detail: String,
    },
}

/// 校验安装根本体形态：只用 `symlink_metadata` 看 root 本体，**NEVER** 跟随链接。
///
/// 读路径三态解析与安装入口（create / adopt / prepare）共用此判定，
/// 保证「安装时拒绝的 root」与「读取时拒绝的 root」是同一契约。
pub(super) fn validate_install_root(root_dir: &Path) -> InstallRootState {
    match std::fs::symlink_metadata(root_dir) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => InstallRootState::Missing,
        Err(error) => InstallRootState::Invalid {
            unprobeable: true,
            detail: format!("模型安装根路径不可探测：{error}"),
        },
        Ok(metadata) if metadata.file_type().is_symlink() => InstallRootState::Invalid {
            unprobeable: false,
            detail: format!("模型安装根路径是可疑符号链接：{}", root_dir.display()),
        },
        Ok(metadata) if !metadata.is_dir() => InstallRootState::Invalid {
            unprobeable: false,
            detail: format!("模型安装根路径不是目录：{}", root_dir.display()),
        },
        Ok(_) => InstallRootState::Directory,
    }
}

/// 安装入口的确保存在：root 缺失时安全 `create_dir_all`，随后（及既存形态下）
/// 一律用 `symlink_metadata` 复验最终 root 本体不是符号链接且是目录。
///
/// 不能只信 `create_dir_all` 的返回值：它会把「既存的指向目录的符号链接」当成功，
/// 且「缺失探测 → 创建 → 使用」之间存在竞态窗口；root 已是符号链接/非目录时
/// typed 失败，**NEVER** 沿链接创建暂存或安装。
pub(super) fn ensure_root_directory(root_dir: &Path) -> Result<(), ModelInstallError> {
    if let InstallRootState::Missing = validate_install_root(root_dir) {
        create_root_directory(root_dir).map_err(|error| {
            ModelInstallError::StagingCreateFailed {
                detail: format!("安装根目录创建失败：{error}"),
            }
        })?;
    }
    verify_root_directory(root_dir)
}

/// 创建缺失的安装根目录（unix：DirBuilder mode `0700`，owner 专属；
/// 非 unix：常规 `create_dir_all`）。仅作用于新建目录，NEVER chmod 既存目录。
#[cfg(unix)]
fn create_root_directory(root_dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(root_dir)
}

#[cfg(not(unix))]
fn create_root_directory(root_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(root_dir)
}

/// 要求 root 此刻已存在且形态合法（adopt / prepare / 端口绑定核对的 TOCTOU 复验：
/// 不创建、只校验）。
pub(super) fn require_existing_root(root_dir: &Path) -> Result<(), ModelInstallError> {
    match validate_install_root(root_dir) {
        InstallRootState::Directory => Ok(()),
        InstallRootState::Missing => Err(ModelInstallError::RootInvalid {
            detail: format!("模型安装根路径不存在：{}", root_dir.display()),
        }),
        InstallRootState::Invalid { detail, .. } => Err(ModelInstallError::RootInvalid { detail }),
    }
}

/// 复验 root 本体（`create_dir_all` 之后与暂存目录创建之后的竞态复验共用）。
fn verify_root_directory(root_dir: &Path) -> Result<(), ModelInstallError> {
    match validate_install_root(root_dir) {
        InstallRootState::Directory => Ok(()),
        InstallRootState::Missing => Err(ModelInstallError::RootInvalid {
            detail: format!(
                "模型安装根路径在创建后消失（可能被并发删除）：{}",
                root_dir.display()
            ),
        }),
        InstallRootState::Invalid { detail, .. } => Err(ModelInstallError::RootInvalid { detail }),
    }
}

/// 自有暂存目录守卫：持有 `PathBuf` + armed 标记。
///
/// armed 时 `Drop` 同步 best-effort 兜底清理（非正常路径的最后防线，
/// 正常 application 错误路径应显式清理，见文件头注释）；no-replace 提交成功后 disarm
/// （目录已原子搬迁到 final），准入复核失败时 disarm（路径非自有，**NEVER** 删除）。
#[derive(Debug)]
pub(super) struct StagingGuard {
    /// 自有暂存目录路径。
    path: PathBuf,
    /// 是否仍负有清理责任。
    armed: bool,
}

impl StagingGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    /// 暂存目录路径（只读）。
    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    /// 解除清理责任（目录已搬迁到 final，或已不再确认为自有路径）。
    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }

    /// 显式清理（消费守卫；正常路径在 `spawn_blocking` 内调用）。
    pub(super) fn cleanup(mut self) {
        discard_staging_directory(&self.path);
        self.disarm();
    }
}

impl Drop for StagingGuard {
    fn drop(&mut self) {
        if self.armed {
            discard_staging_directory(&self.path);
        }
    }
}

/// 下载/安装暂存目录句柄（RAII）：[`path`](StagingDirectory::path) 暴露
/// downloader 的逐资产写入落点。`Drop` 是**同步 best-effort 兜底**清理
/// （尽力同步清理、失败仅记录日志），正常失败/成功路径以显式清理为准；
/// 下载整合后的 application 错误路径由 Task 5 改为显式 await 清理。
///
/// 由 `LocalModelAssetStore::create_staging_directory` 创建，或经
/// `LocalModelAssetStore::adopt_staging_directory` 准入校验后接管。
#[derive(Debug)]
#[must_use]
pub struct StagingDirectory {
    /// 自有暂存目录守卫。
    guard: StagingGuard,
}

impl StagingDirectory {
    /// 武装一个已通过准入校验的暂存目录。
    fn armed(path: PathBuf) -> Self {
        Self {
            guard: StagingGuard::new(path),
        }
    }

    /// 暂存目录路径（downloader 写入资产文件的落点）。
    pub fn path(&self) -> &Path {
        self.guard.path()
    }

    /// 移交暂存目录路径（消费句柄并解除 RAII 清理责任）：
    /// 供 [`ModelInstallerPort`](crate::ports::ModelInstallerPort) 实现把
    /// 清理责任转移给端口层 [`ModelStagingArea`](crate::ports::ModelStagingArea)。
    pub(crate) fn into_path(mut self) -> PathBuf {
        self.guard.disarm();
        self.guard.path().to_path_buf()
    }

    /// 交出守卫（prepare / commit 消费；保持 armed）。
    fn into_guard(self) -> StagingGuard {
        self.guard
    }
}

impl AsRef<Path> for StagingDirectory {
    fn as_ref(&self) -> &Path {
        self.guard.path()
    }
}

/// 创建暂存目录（blocking）：先 [`ensure_root_directory`]（root 缺失时安全创建，
/// 创建后复验 root 本体非链接、是目录），再以 pid + 原子序号生成唯一名
/// （unix 下创建 mode `0700`），冲突则换下一个序号重试，超限失败。
///
/// 每次 `create_dir` 成功后**再复验一次 root 本体**（防「root 被并发换成符号链接」
/// 的竞态）：复验不过立刻清理刚创建的目录并 typed 失败，
/// **NEVER** 沿符号链接把暂存留在外部目标。
pub(super) fn create_staging_directory_sync(
    root_dir: &Path,
    engine_revision: &str,
) -> Result<StagingDirectory, ModelInstallError> {
    ensure_root_directory(root_dir)?;
    for _attempt in 0..STAGING_ATTEMPT_LIMIT {
        let sequence = STAGING_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let staging_name = format!(
            "{STAGING_DIR_PREFIX}{engine_revision}-{}-{sequence}",
            std::process::id()
        );
        let staging_path = root_dir.join(staging_name);
        match create_staging_dir(&staging_path) {
            Ok(()) => {
                if let Err(root_error) = verify_root_directory(root_dir) {
                    discard_staging_directory(&staging_path);
                    return Err(root_error);
                }
                return Ok(StagingDirectory::armed(staging_path));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(ModelInstallError::StagingCreateFailed {
                    detail: format!("暂存目录创建失败：{error}"),
                });
            }
        }
    }
    Err(ModelInstallError::StagingCreateFailed {
        detail: format!("暂存目录命名冲突超过 {STAGING_ATTEMPT_LIMIT} 次"),
    })
}

/// 创建单个暂存目录（unix：DirBuilder mode `0700`，owner 专属；
/// 非 unix：常规 `create_dir`）。
#[cfg(unix)]
fn create_staging_dir(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o700).create(path)
}

#[cfg(not(unix))]
fn create_staging_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir(path)
}

/// 暂存目录全名精确契约：`.tmp-<engine_revision>-<pid>-<seq>`。
///
/// **不做 `starts_with` 前缀匹配**：暂存前缀、revision 段与 pid/seq 段都必须
/// 逐字符精确（pid / seq 仅 ASCII 数字且非空，revision 段取自期望 manifest
/// 的精确 engine revision），杜绝 `.tmp-<其他 revision>-…` 或
/// `.tmp-<revision>-<garbage>` 之类的伪名混入安装 / 清理入口。
pub(super) fn staging_name_matches(staging_name: &str, engine_revision: &str) -> bool {
    let Some(after_prefix) = staging_name.strip_prefix(STAGING_DIR_PREFIX) else {
        return false;
    };
    let Some(after_revision) = after_prefix
        .strip_prefix(engine_revision)
        .and_then(|rest| rest.strip_prefix('-'))
    else {
        return false;
    };
    let mut segments = after_revision.split('-');
    match (segments.next(), segments.next(), segments.next()) {
        (Some(pid), Some(sequence), None) => {
            !pid.is_empty()
                && pid.bytes().all(|byte| byte.is_ascii_digit())
                && !sequence.is_empty()
                && sequence.bytes().all(|byte| byte.is_ascii_digit())
        }
        _ => false,
    }
}

/// 接管外部创建的暂存目录（blocking）：root 本体与暂存准入（含全名精确契约）
/// 都通过才武装守卫。
///
/// 被拒路径从不武装守卫，**NEVER** 被删除。
pub(super) fn adopt_staging_directory(
    root_dir: &Path,
    engine_revision: &str,
    staging_dir: PathBuf,
) -> Result<StagingDirectory, ModelInstallError> {
    // TOCTOU 复验：接管时 root 必须仍是真实目录，NEVER 沿符号链接接管暂存。
    require_existing_root(root_dir)?;
    validate_staging_directory(root_dir, engine_revision, &staging_dir)?;
    Ok(StagingDirectory::armed(staging_dir))
}

/// 暂存目录准入（blocking）：必须是安装根的直接子目录（同文件系统 rename 的前提）、
/// 全名精确匹配 `.tmp-<expected_revision>-<pid>-<seq>`、真实存在的常规目录；
/// 否则拒绝且 **NEVER** 清理非自有路径。
fn validate_staging_directory(
    root_dir: &Path,
    engine_revision: &str,
    staging_dir: &Path,
) -> Result<(), ModelInstallError> {
    if staging_dir.parent() != Some(root_dir) {
        return Err(ModelInstallError::StagingRejected {
            detail: format!(
                "暂存目录必须是安装根目录的直接子目录：{}",
                staging_dir.display()
            ),
        });
    }
    let Some(staging_name) = staging_dir.file_name().and_then(|name| name.to_str()) else {
        return Err(ModelInstallError::StagingRejected {
            detail: format!("暂存目录名无效：{}", staging_dir.display()),
        });
    };
    if !staging_name_matches(staging_name, engine_revision) {
        return Err(ModelInstallError::StagingRejected {
            detail: format!(
                "暂存目录名必须精确匹配 {STAGING_DIR_PREFIX}{engine_revision}-<pid>-<seq>（pid/seq 为 ASCII 数字）：{staging_name}"
            ),
        });
    }
    match std::fs::symlink_metadata(staging_dir) {
        Err(error) => Err(ModelInstallError::StagingRejected {
            detail: format!("暂存目录不可探测：{error}"),
        }),
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(ModelInstallError::StagingRejected {
                detail: format!("暂存目录是可疑符号链接：{staging_name}"),
            })
        }
        Ok(metadata) if !metadata.is_dir() => Err(ModelInstallError::StagingRejected {
            detail: format!("暂存目录不是目录：{staging_name}"),
        }),
        Ok(_) => Ok(()),
    }
}

/// 安装准备（blocking）：复验 root 本体 → 复核暂存准入 → 预判 final → 写 canonical manifest → 完整校验。
///
/// 消费 [`StagingDirectory`]：任一失败分支直接返回，由句柄 `Drop` 在本线程
/// 清理自有暂存目录（root/准入复核失败除外：先 disarm 再丢弃，非自有路径 **NEVER** 触碰）。
pub(super) fn prepare_staged_install_sync(
    root_dir: &Path,
    revision_dir: &Path,
    expected_manifest: &ModelManifest,
    mut staging: StagingDirectory,
) -> Result<PreparedStagedInstall, ModelInstallError> {
    let staging_path = staging.path().to_path_buf();
    // TOCTOU 复验：创建暂存到 prepare 之间 root 可能被换成符号链接，重新校验。
    if let Err(root_error) = require_existing_root(root_dir) {
        staging.guard.disarm(); // root 形态异常 → 暂存路径不可确认为自有，NEVER 删除
        return Err(root_error);
    }
    if let Err(rejected) =
        validate_staging_directory(root_dir, &expected_manifest.engine_revision, &staging_path)
    {
        staging.guard.disarm(); // 准入复核失败 → 路径非自有，NEVER 删除
        return Err(rejected);
    }
    match probe_final_revision(revision_dir, expected_manifest) {
        FinalRevisionState::Valid(installed) => {
            return Ok(PreparedStagedInstall::FinalAlreadyValid(installed));
        }
        FinalRevisionState::Unprobeable { detail } => {
            return Err(ModelInstallError::FinalUnprobeable { detail });
        }
        FinalRevisionState::Invalid { detail } => {
            return Err(ModelInstallError::FinalInvalid { detail });
        }
        FinalRevisionState::Absent => {}
    }
    if let Err(detail) = write_canonical_manifest(&staging_path, expected_manifest) {
        return Err(ModelInstallError::StagingWriteFailed { detail });
    }
    if let Err(failure) = verify_installed_directory(&staging_path, expected_manifest) {
        return Err(ModelInstallError::VerificationFailed {
            kind: failure.kind,
            detail: failure.detail,
        });
    }
    Ok(PreparedStagedInstall::ReadyToCommit(StagedInstallCommit {
        staging: staging.into_guard(), // 保持 armed：commit 成功 no-replace 提交后才 disarm
        revision_dir: revision_dir.to_path_buf(),
        manifest: expected_manifest.clone(),
    }))
}

/// 把期望 manifest 序列化为 canonical JSON 写入暂存目录（落盘唯一形态）。
///
/// `create_new`（`O_CREAT | O_EXCL`，unix 另加 `O_NOFOLLOW`）保证绝不跟随链接或
/// 覆盖既有文件：暂存内已存在 regular manifest（下载方预写）先安全删除
/// （unlink 只摘链接/文件本身，不跟随）再新建；符号链接或目录形态明确拒绝。
fn write_canonical_manifest(
    staging_dir: &Path,
    expected_manifest: &ModelManifest,
) -> Result<(), String> {
    let serialized = serde_json::to_string_pretty(expected_manifest)
        .map_err(|error| format!("manifest 序列化失败：{error}"))?;
    let manifest_path = staging_dir.join(MODEL_MANIFEST_FILE_NAME);
    match std::fs::symlink_metadata(&manifest_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!(
                "暂存目录中的 {MODEL_MANIFEST_FILE_NAME} 是可疑符号链接，拒绝写入（不跟随链接）"
            ));
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(format!(
                "暂存目录中的 {MODEL_MANIFEST_FILE_NAME} 不是常规文件，拒绝覆盖"
            ));
        }
        Ok(_) => std::fs::remove_file(&manifest_path)
            .map_err(|error| format!("既有 {MODEL_MANIFEST_FILE_NAME} 清理失败：{error}"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!("{MODEL_MANIFEST_FILE_NAME} 形态探测失败：{error}"));
        }
    }
    #[cfg(unix)]
    let created = {
        use std::os::unix::fs::OpenOptionsExt;
        // O_NOFOLLOW 不由 std 暴露；值取自 rustix 的平台常量（与 libc 同一 ABI 定义）。
        let no_follow = rustix::fs::OFlags::NOFOLLOW.bits() as i32;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(no_follow)
            .open(&manifest_path)
    };
    #[cfg(not(unix))]
    let created = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&manifest_path);
    let mut manifest_file =
        created.map_err(|error| format!("{MODEL_MANIFEST_FILE_NAME} 创建失败：{error}"))?;
    std::io::Write::write_all(&mut manifest_file, serialized.as_bytes())
        .map_err(|error| format!("{MODEL_MANIFEST_FILE_NAME} 写入失败：{error}"))
}

/// 安装提交（blocking）：原子 **no-replace** rename 暂存 → final。
///
/// 成功后 disarm 守卫（暂存目录已搬迁为 final）；失败按 [`classify_rename_failure`]
/// 分类：竞争者已落地有效安装则幂等返回，目标已存在且验证表明竞争者（或平台缺原语）
/// 归 `RenameConflict`，无目标/不可探测的 IO 失败归 `CommitIoFailed`；所有路径均在本
/// 线程清理自己的暂存目录且不触碰 final 内容。
pub(super) fn commit_prepared_sync(
    prepared: StagedInstallCommit,
) -> Result<InstalledAssets, ModelInstallError> {
    let StagedInstallCommit {
        mut staging,
        revision_dir,
        manifest,
    } = prepared;
    match rename_without_replace(staging.path(), &revision_dir) {
        Ok(()) => {
            staging.disarm(); // 已原子搬迁到 final，暂存路径不复存在
            InstalledAssets::new(manifest, revision_dir).map_err(|error| {
                ModelInstallError::VerificationFailed {
                    kind: InvalidAssetKind::Unsupported,
                    detail: error.to_string(),
                }
            })
        }
        Err(rename_error) => {
            let target = probe_final_target_after_failure(&rename_error, &revision_dir);
            let competitor_verification_detail = match target {
                // 目标已存在：先完整校验，竞争者是有效安装则幂等返回现有结果。
                FinalTargetProbe::Present => {
                    match verify_installed_directory(&revision_dir, &manifest) {
                        Ok(installed) => {
                            staging.cleanup();
                            return Ok(installed);
                        }
                        Err(failure) => Some(failure.detail),
                    }
                }
                FinalTargetProbe::Unsupported
                | FinalTargetProbe::Absent
                | FinalTargetProbe::Unprobeable(_) => None,
            };
            let classified =
                classify_rename_failure(&rename_error, target, competitor_verification_detail);
            staging.cleanup();
            Err(classified)
        }
    }
}

/// rename 失败后对 final 目标的探测结果（错误分类的输入）。
#[derive(Debug)]
pub(super) enum FinalTargetProbe {
    /// 平台缺 no-replace 原语（fail closed）。
    Unsupported,
    /// 目标已存在（rename 冲突，可能的竞争者落地物）。
    Present,
    /// 目标不存在（rename 因别的原因失败）。
    Absent,
    /// 目标不可探测（权限/IO 失败，无从判断竞争者）。
    Unprobeable(String),
}

/// rename 失败后探测 final 目标（Unsupported 不做 fs 探测，直接 fail closed 分类）。
fn probe_final_target_after_failure(
    rename_error: &std::io::Error,
    revision_dir: &Path,
) -> FinalTargetProbe {
    if rename_error.kind() == std::io::ErrorKind::Unsupported {
        return FinalTargetProbe::Unsupported;
    }
    match std::fs::symlink_metadata(revision_dir) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => FinalTargetProbe::Absent,
        Err(error) => FinalTargetProbe::Unprobeable(error.to_string()),
        Ok(_) => FinalTargetProbe::Present,
    }
}

/// rename 失败的错误分类（单一决策点，确定性可单测）：
/// - 平台缺 no-replace 原语（`Unsupported`）→ `RenameConflict`（fail closed，NEVER 回退覆盖性 rename）；
/// - 目标已存在且验证结果表明竞争者（`Present` + 校验失败详情）→ `RenameConflict`（NEVER 覆盖）；
/// - 目标不存在或不可探测（EACCES / EXDEV / ENOENT 等无目标失败）→ `CommitIoFailed`
///   （这不是竞争冲突，是提交 IO 失败；暂存已由调用方清理）。
pub(super) fn classify_rename_failure(
    rename_error: &std::io::Error,
    target: FinalTargetProbe,
    competitor_verification_detail: Option<String>,
) -> ModelInstallError {
    match target {
        FinalTargetProbe::Unsupported => ModelInstallError::RenameConflict {
            detail: format!(
                "平台不支持原子 no-replace rename，拒绝回退覆盖性 rename：{rename_error}"
            ),
        },
        FinalTargetProbe::Present => ModelInstallError::RenameConflict {
            detail: format!(
                "原子 no-replace rename 失败：{rename_error}；安装目标已存在且校验未通过（竞争者落地）：{}",
                competitor_verification_detail.unwrap_or_else(|| "校验详情缺失".to_owned())
            ),
        },
        FinalTargetProbe::Absent => ModelInstallError::CommitIoFailed {
            detail: format!(
                "原子 no-replace rename 失败且安装目标不存在（非竞争冲突的提交 IO 失败）：{rename_error}"
            ),
        },
        FinalTargetProbe::Unprobeable(probe_detail) => ModelInstallError::CommitIoFailed {
            detail: format!(
                "原子 no-replace rename 失败：{rename_error}；安装目标不可探测：{probe_detail}"
            ),
        },
    }
}

/// 原子 no-replace rename：目标无论空/非空一律不覆盖。
///
/// Linux/Android 走 `renameat2(RENAME_NOREPLACE)`，macOS/iOS 走
/// `renameatx_np(RENAME_EXCL)`（均由 rustix safe API 封装）；运行时缺该原语
/// （如 macOS < 10.12 返回 NOSYS）同样归为 `Unsupported`，fail closed。
#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
))]
fn rename_without_replace(source: &Path, target: &Path) -> std::io::Result<()> {
    use rustix::fs::{renameat_with, RenameFlags, CWD};
    renameat_with(CWD, source, CWD, target, RenameFlags::NOREPLACE).map_err(|error| {
        if error == rustix::io::Errno::NOSYS || error == rustix::io::Errno::OPNOTSUPP {
            std::io::Error::from(std::io::ErrorKind::Unsupported)
        } else {
            std::io::Error::from(error)
        }
    })
}

/// fail-closed：本 target 没有可证明不覆盖的 rename 原语，
/// **NEVER** 回退 `std::fs::rename`（它会覆盖空目录）。
#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
)))]
fn rename_without_replace(_source: &Path, _target: &Path) -> std::io::Result<()> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

/// 尽力同步清理自有暂存目录（blocking；共享实现在端口层，
/// 清理失败仅记录日志、不掩盖原始错误）。
fn discard_staging_directory(staging_dir: &Path) {
    crate::ports::discard_staged_directory(staging_dir);
}
