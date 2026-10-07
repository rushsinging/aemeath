//! 本地模型资产存储：revision 目录解析、逐资产流式哈希校验与原子安装。
//!
//! - **根路径与期望 manifest 全部由构造器注入**（生产 composition 传
//!   `share::config::paths::systemone_models_dir()` 与固定发行 manifest），
//!   adapter **NEVER** 读取环境变量，也 **NEVER** 使用进程 cwd。
//! - 解析三态（fail-closed）：`Missing` = root/revision/manifest 均不存在；
//!   `Installed` = manifest 与逐资产长度/SHA-256 全部校验通过；
//!   `Invalid` = typed 原因类别 + 中文 detail（JSON 损坏、契约不符、资产缺失、
//!   可疑符号链接等），半成品 **NEVER** 被识别为已安装；root 本身是符号链接同样拒绝。
//! - 安装走 `<root>/.tmp-<revision>-<pid>-<seq>` 暂存目录（RAII [`StagingDirectory`]）：
//!   写 canonical manifest → 完整重新校验 → 原子 **no-replace** rename 提交到
//!   `<root>/<revision>`：目标目录无论空/非空一律不覆盖；失败清理自己的暂存目录，
//!   已有有效安装幂等返回，已有无效安装 **NEVER** 覆盖或删除，rename 冲突 fail closed。
//! - 文件 IO 在 `spawn_blocking` 内一次完成；SHA-256 经 `utils::sha256_reader_hex`
//!   流式读取，**NEVER** 把数百 MB 的模型文件整体读入内存。
//!
//! 职责拆分：本文件 = store / 公开 API / 三态解析；同级 `model_assets_verify.rs`
//! = 读校验（末段 NO-follow 打开 + 父组件逐组件探测，竞态由 SHA 锁定）；
//! `model_assets_install.rs` = 安装根形态契约（读/装共享 `validate_install_root`）、
//! 暂存 RAII、canonical manifest 写入与原子 no-replace 提交。

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::domain::{ModelManifest, ModelManifestError};
use crate::ports::{InstalledAssets, InvalidAssetKind, ModelAssetPort, ModelAssetState};

#[path = "model_assets_install.rs"]
mod install;
#[path = "model_assets_installer_port.rs"]
mod installer_port;
#[path = "model_assets_verify.rs"]
mod verify;

pub use install::StagingDirectory;
use install::{
    commit_prepared_sync, create_staging_directory_sync, prepare_staged_install_sync,
    validate_install_root, InstallRootState, StagingGuard,
};

/// 安装入口失败（fail-closed：任何失败都不产生可被运行时识别的半成品安装）。
#[derive(Debug)]
pub enum ModelInstallError {
    /// 暂存目录不被接受（不是安装根的直接子目录、缺隐藏前缀、非常规目录或可疑符号链接）。
    StagingRejected {
        /// 中文失败原因。
        detail: String,
    },
    /// 暂存目录创建失败（IO 错误或命名冲突超限）。
    StagingCreateFailed {
        /// 中文失败原因。
        detail: String,
    },
    /// 暂存目录写入失败（canonical manifest 创建/写入被拒绝：形态异常，NEVER 跟随链接）。
    StagingWriteFailed {
        /// 中文失败原因。
        detail: String,
    },
    /// 安装根本体形态异常（符号链接 / 非目录 / 缺失 / 创建后复验失败）：
    /// fail-closed，**NEVER** 沿符号链接创建暂存或安装。
    RootInvalid {
        /// 中文失败原因。
        detail: String,
    },
    /// final revision 已存在但校验无效：**NEVER** 覆盖或删除。
    FinalInvalid {
        /// 中文失败原因。
        detail: String,
    },
    /// final revision 已存在但探测即 IO 失败：既不判定无效，也 **NEVER** 覆盖。
    FinalUnprobeable {
        /// 中文失败原因。
        detail: String,
    },
    /// 暂存内容校验失败（typed 类别 + 中文原因）。
    VerificationFailed {
        /// 失败类别（损坏 / 不可读 / 不支持）。
        kind: InvalidAssetKind,
        /// 中文失败原因。
        detail: String,
    },
    /// 原子 no-replace rename 冲突：仅目标已存在且验证结果表明竞争者、
    /// 或平台缺该原语时使用（fail closed，NEVER 覆盖）。
    RenameConflict {
        /// 中文失败原因。
        detail: String,
    },
    /// 原子提交阶段的 IO 失败（EACCES / EXDEV / ENOENT 等无目标或不可探测的失败）：
    /// 不是竞争冲突，暂存已清理、final 未被触碰。
    CommitIoFailed {
        /// 中文失败原因。
        detail: String,
    },
    /// `spawn_blocking` 任务失败。
    TaskJoinFailed {
        /// 中文失败原因。
        detail: String,
    },
}

impl std::fmt::Display for ModelInstallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StagingRejected { detail } => write!(formatter, "暂存目录被拒绝：{detail}"),
            Self::StagingCreateFailed { detail } => {
                write!(formatter, "创建暂存目录失败：{detail}")
            }
            Self::StagingWriteFailed { detail } => write!(formatter, "暂存目录写入失败：{detail}"),
            Self::RootInvalid { detail } => {
                write!(formatter, "模型安装根路径无效，拒绝安装：{detail}")
            }
            Self::FinalInvalid { detail } => {
                write!(formatter, "已存在的模型安装无效，拒绝覆盖：{detail}")
            }
            Self::FinalUnprobeable { detail } => {
                write!(
                    formatter,
                    "已存在的模型安装目标不可探测，拒绝覆盖：{detail}"
                )
            }
            Self::VerificationFailed { kind, detail } => write!(formatter, "{kind}：{detail}"),
            Self::RenameConflict { detail } => {
                write!(formatter, "模型安装原子提交冲突，已放弃：{detail}")
            }
            Self::CommitIoFailed { detail } => {
                write!(formatter, "模型安装原子提交 IO 失败：{detail}")
            }
            Self::TaskJoinFailed { detail } => {
                write!(formatter, "模型安装任务执行失败：{detail}")
            }
        }
    }
}

impl std::error::Error for ModelInstallError {}

/// 安装暂存已完整校验、等待原子 no-replace 提交的令牌。
///
/// 由 `LocalModelAssetStore::prepare_staged_install` 产出。内部 [`StagingGuard`]
/// 持有暂存目录清理责任（没有「令牌已消费」的可空态）：提交成功后 disarm，
/// 未提交即被丢弃或提交冲突时由 guard 的 `Drop` 清理暂存目录。
/// 该 `Drop` 只是**同步 best-effort 兜底**（失败仅记录日志），NEVER 作为常规清理手段：
/// 正常 application 错误路径应显式走清理点，下载整合后 Task 5 会改为显式 await 清理。
#[derive(Debug)]
pub struct StagedInstallCommit {
    /// 自有暂存目录守卫（armed = `Drop` 负责清理）。
    staging: StagingGuard,
    /// 目标 final revision 目录（no-replace rename 的目的地）。
    revision_dir: PathBuf,
    /// 校验通过的期望 manifest（落盘真相的唯一来源）。
    manifest: ModelManifest,
}

/// 安装准备结果：final 已有效（幂等返回现有安装）或暂存已就绪（待提交）。
#[derive(Debug)]
#[must_use]
pub enum PreparedStagedInstall {
    /// final revision 已存在且有效：暂存已被清理，直接返回现有安装。
    FinalAlreadyValid(InstalledAssets),
    /// final 不存在且暂存校验通过：等待 `commit_prepared_install` 原子提交。
    ReadyToCommit(StagedInstallCommit),
}

/// 本地模型资产存储 adapter：只读解析 + 原子安装两个职责，
/// 读路径经 `ModelAssetPort` 消费，安装入口为本类型具体方法（不混入读端口）。
#[derive(Debug)]
pub struct LocalModelAssetStore {
    /// 安装根目录（构造器注入；生产为 `systemone_models_dir()`）。
    root_dir: PathBuf,
    /// 期望的 final revision 目录 `<root>/<engine_revision>`。
    revision_dir: PathBuf,
    /// 当前发行的固定 manifest（落盘内容 MUST 与之完全一致的唯一期望真相）。
    expected_manifest: ModelManifest,
}

impl LocalModelAssetStore {
    /// 构造存储：校验期望 manifest 契约，并以 `manifest.engine_revision` 定位
    /// 唯一目标 revision 目录（不扫描其他 revision，杜绝模糊匹配）。
    pub fn new(
        root_dir: PathBuf,
        expected_manifest: ModelManifest,
    ) -> Result<Self, ModelManifestError> {
        expected_manifest.validate()?;
        let revision_dir = root_dir.join(&expected_manifest.engine_revision);
        Ok(Self {
            root_dir,
            revision_dir,
            expected_manifest,
        })
    }

    /// 安装根目录（只读）。
    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    /// 在安装根下创建唯一暂存目录 `.tmp-<revision>-<pid>-<seq>`（RAII）。
    ///
    /// root 本体形态不合格（符号链接 / 非目录 / 创建后复验失败）时 typed 失败
    /// （[`ModelInstallError::RootInvalid`]），**NEVER** 沿符号链接创建暂存；
    /// root 缺失时安全创建后再复验本体。
    /// 暂存名带隐藏前缀，运行时解析（canonical revision 目录）永远无法识别它；
    /// downloader 经 [`StagingDirectory::path`] 拿到逐资产写入落点，
    /// 未提交（或未走到显式清理点）即丢弃句柄时由 `Drop` 兜底清理；
    /// 进程中断留下的 stale 暂存由后续启动 / 下载前 sweep 承接（Drop 只是同步兜底）。
    pub async fn create_staging_directory(&self) -> Result<StagingDirectory, ModelInstallError> {
        let root_dir = self.root_dir.clone();
        let engine_revision = self.expected_manifest.engine_revision.clone();
        let task = tokio::task::spawn_blocking(move || {
            create_staging_directory_sync(&root_dir, &engine_revision)
        });
        map_blocking_task_result(task.await)
    }

    /// 接管一个外部创建的暂存目录：root 本体与暂存准入（全名精确匹配
    /// `.tmp-<expected_revision>-<pid>-<seq>`）都通过才返回武装的 RAII 守卫。
    ///
    /// root 不是真实目录（缺失 / 符号链接 / 非目录）→ [`ModelInstallError::RootInvalid`]；
    /// 被拒路径（root 外、全名不符、非常规目录或可疑符号链接）从不被武装，
    /// **NEVER** 删除任何被拒路径。
    pub fn adopt_staging_directory(
        &self,
        staging_dir: PathBuf,
    ) -> Result<StagingDirectory, ModelInstallError> {
        install::adopt_staging_directory(
            &self.root_dir,
            &self.expected_manifest.engine_revision,
            staging_dir,
        )
    }

    /// 一步安装：复核暂存 → 写 canonical manifest → 完整重新校验 →
    /// 幂等/fail-closed 判断 → 原子 no-replace rename。
    ///
    /// `staging` MUST 是本 store 创建（[`Self::create_staging_directory`]）或经
    /// [`Self::adopt_staging_directory`] 接管的暂存目录；安装输入只接受暂存内的
    /// 资产文件，**NEVER** 接受任意绝对目标路径。
    pub async fn install_staged_assets(
        &self,
        staging: StagingDirectory,
    ) -> Result<InstalledAssets, ModelInstallError> {
        let prepared = self.prepare_staged_install(staging).await?;
        self.commit_prepared_install(prepared).await
    }

    /// 安装 step 1：复验 root 本体（防 TOCTOU 换成符号链接）、复核暂存准入、
    /// 预判 final（已有效 → 幂等；已无效或不可探测 → 拒绝覆盖；不存在 → 继续），
    /// 写 canonical manifest、完整重新校验。
    ///
    /// 消费 [`StagingDirectory`]：任一失败分支都会清理自有暂存目录
    /// （root/准入复核失败先 disarm，路径非自有 NEVER 删除）；
    /// caller 提前丢弃句柄时 `Drop` 同样兜底清理。
    pub async fn prepare_staged_install(
        &self,
        staging: StagingDirectory,
    ) -> Result<PreparedStagedInstall, ModelInstallError> {
        let root_dir = self.root_dir.clone();
        let revision_dir = self.revision_dir.clone();
        let expected_manifest = self.expected_manifest.clone();
        let task = tokio::task::spawn_blocking(move || {
            prepare_staged_install_sync(&root_dir, &revision_dir, &expected_manifest, staging)
        });
        map_blocking_task_result(task.await)
    }

    /// 安装 step 2：把已校验暂存目录原子 **no-replace** rename 到 final revision。
    ///
    /// 提交 **NEVER** 覆盖已存在的 final（空目录也不覆盖）：no-replace rename 失败时
    /// 探测 final——竞争者为有效安装则幂等返回；目标已存在且验证表明竞争者
    /// （或平台缺原语）→ `RenameConflict`；目标不存在/不可探测的 IO 失败
    /// （EACCES / EXDEV / ENOENT 等）→ `CommitIoFailed`。各路径都
    /// **NEVER** 删除或覆盖已存在的 final，且都会清理自己的暂存目录。
    pub async fn commit_prepared_install(
        &self,
        prepared: PreparedStagedInstall,
    ) -> Result<InstalledAssets, ModelInstallError> {
        match prepared {
            PreparedStagedInstall::FinalAlreadyValid(installed) => Ok(installed),
            PreparedStagedInstall::ReadyToCommit(commit_token) => {
                let task = tokio::task::spawn_blocking(move || commit_prepared_sync(commit_token));
                map_blocking_task_result(task.await)
            }
        }
    }
}

#[async_trait]
impl ModelAssetPort for LocalModelAssetStore {
    async fn installed_assets(&self) -> ModelAssetState {
        let root_dir = self.root_dir.clone();
        let revision_dir = self.revision_dir.clone();
        let expected_manifest = self.expected_manifest.clone();
        let task = tokio::task::spawn_blocking(move || {
            resolve_installed_state(&root_dir, &revision_dir, &expected_manifest)
        });
        match task.await {
            Ok(state) => state,
            Err(error) => ModelAssetState::Invalid {
                kind: InvalidAssetKind::Unreadable,
                detail: format!("模型资产状态解析任务执行失败：{error}"),
            },
        }
    }
}

/// 映射 `spawn_blocking` 结果：join 失败（panic/取消）转 typed 错误，并解包任务载荷。
fn map_blocking_task_result<T>(
    task_result: Result<Result<T, ModelInstallError>, tokio::task::JoinError>,
) -> Result<T, ModelInstallError> {
    task_result
        .map_err(|error| ModelInstallError::TaskJoinFailed {
            detail: format!("阻塞任务执行失败：{error}"),
        })
        .and_then(|task_outcome| task_outcome)
}

/// 解析安装状态三态（blocking，供 `spawn_blocking` 调用）。
///
/// root 形态复用安装入口同一判定 [`validate_install_root`]：缺失 → `Missing`；
/// 符号链接 / 非目录 / 不可探测 → `Invalid`（fail-closed，NEVER 穿过链接把外部目录
/// 识别为安装根），保证读路径与安装入口对 root 契约完全一致。
fn resolve_installed_state(
    root_dir: &Path,
    revision_dir: &Path,
    expected_manifest: &ModelManifest,
) -> ModelAssetState {
    match validate_install_root(root_dir) {
        InstallRootState::Missing => return ModelAssetState::Missing,
        InstallRootState::Directory => {}
        InstallRootState::Invalid {
            unprobeable,
            detail,
        } => {
            return ModelAssetState::Invalid {
                kind: if unprobeable {
                    InvalidAssetKind::Unreadable
                } else {
                    InvalidAssetKind::Unsupported
                },
                detail,
            };
        }
    }
    // revision 目录缺失 → Missing；存在但可疑 → Invalid；存在则完整校验。
    match std::fs::symlink_metadata(revision_dir) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ModelAssetState::Missing,
        Err(error) => ModelAssetState::Invalid {
            kind: InvalidAssetKind::Unreadable,
            detail: format!("模型 revision 目录不可探测：{error}"),
        },
        Ok(metadata) if metadata.file_type().is_symlink() => ModelAssetState::Invalid {
            kind: InvalidAssetKind::Corrupt,
            detail: format!(
                "模型 revision 目录是可疑符号链接：{}",
                revision_dir.display()
            ),
        },
        Ok(metadata) if !metadata.is_dir() => ModelAssetState::Invalid {
            kind: InvalidAssetKind::Unsupported,
            detail: format!("模型 revision 安装路径不是目录：{}", revision_dir.display()),
        },
        Ok(_) => match verify::verify_installed_directory(revision_dir, expected_manifest) {
            Ok(installed) => ModelAssetState::Installed(installed),
            Err(failure) => ModelAssetState::Invalid {
                kind: failure.kind,
                detail: failure.detail,
            },
        },
    }
}

/// 测试夹具依赖 unix 能力（`std::os::unix::fs::symlink`）与可证明的 no-replace
/// rename 原语（非 unix 的安装成功路径本就是 fail-closed `Unsupported`），
/// 故仅在 unix 目标编译；production（含非 unix 交叉检查）不受影响。
#[cfg(all(test, unix))]
#[path = "model_assets_tests.rs"]
mod tests;
