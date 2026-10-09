//! `ModelInstallerPort` 实现：端口 staging token 绑定核对 ↔ 存储 RAII 句柄转换、
//! 幂等安装结果分类与 typed 错误映射。
//!
//! - `create_staging`：存储创建暂存目录后把路径连同 **owner 绑定**（owner root +
//!   owner revision + path）移交给端口句柄（RAII 责任随之转移；application 只
//!   可见 `ModelStagingArea::path`）。
//! - `install_staged`：先核对 token 绑定与当前存储一致（owner root / owner
//!   revision / 路径），再复用暂存全名校验（`adopt` 精确复核
//!   `.tmp-<expected_revision>-<pid>-<seq>`）——任何不匹配 typed
//!   [`ModelInstallError::StagingRejected`]（端口侧
//!   [`ModelInstallPortErrorKind::StagingRejected`]）且被拒路径 **NEVER** 删除；
//!   绑定核对通过后的 fs 准入失败才先显式异步清理再报错。
//!   prepare/commit 分类为 [`StagedInstallOutcome::Installed`] /
//!   [`StagedInstallOutcome::AlreadyValid`]（并发竞争者有效时幂等保留）。
//! - `discard_staging`：整个删除在 `spawn_blocking` 内完成，且闭包内**先复验
//!   root / path / 全名 / owner 绑定**，通过才删；不匹配 typed 拒绝、NEVER 删除。
//!   闭包持有端口句柄，任务未执行即结束时由 RAII `Drop` 兜底。
//! - [`ModelInstallError`] → [`ModelInstallPortError`] 的 typed kind 映射集中在此，
//!   application 只按 kind 决策，NEVER 字符串匹配。

use async_trait::async_trait;

use super::{
    install, LocalModelAssetStore, ModelInstallError, PreparedStagedInstall, StagingDirectory,
};
use crate::ports::{
    ModelInstallPortError, ModelInstallPortErrorKind, ModelInstallerPort, ModelStagingArea,
    StagedInstallOutcome,
};

impl From<ModelInstallError> for ModelInstallPortError {
    fn from(error: ModelInstallError) -> Self {
        let kind = match &error {
            ModelInstallError::StagingCreateFailed { .. }
            | ModelInstallError::RootInvalid { .. } => {
                ModelInstallPortErrorKind::StagingUnavailable
            }
            ModelInstallError::StagingRejected { .. } => ModelInstallPortErrorKind::StagingRejected,
            // 写入 / 探测类 IO 失败（暂存 manifest 写入、final 探测即 IO 失败、
            // 提交无目标的 IO 失败）→ IO，不是竞争冲突。
            ModelInstallError::StagingWriteFailed { .. }
            | ModelInstallError::FinalUnprobeable { .. }
            | ModelInstallError::CommitIoFailed { .. } => {
                ModelInstallPortErrorKind::InstallIoFailed
            }
            ModelInstallError::VerificationFailed { .. } => {
                ModelInstallPortErrorKind::VerificationFailed
            }
            // 只有「final 已存在但无效」与「原子提交冲突」才是安装目标冲突。
            ModelInstallError::FinalInvalid { .. } | ModelInstallError::RenameConflict { .. } => {
                ModelInstallPortErrorKind::InstallConflict
            }
            ModelInstallError::TaskJoinFailed { .. } => ModelInstallPortErrorKind::TaskFailed,
        };
        Self {
            kind,
            detail: error.to_string(),
        }
    }
}

/// 端口 staging token 的绑定 + 准入核对：owner 绑定与当前存储一致 → 暂存全名
/// 精确校验 → root 本体复验。任一失败 typed [`ModelInstallError::StagingRejected`]
/// （root 形态问题为 [`ModelInstallError::RootInvalid`]），且 token 在返回前解除
/// 清理责任（被拒路径 **NEVER** 被当前存储删除）；全部通过则原样交还 token。
fn verify_owned_staging(
    root_dir: &std::path::Path,
    engine_revision: &str,
    staging: ModelStagingArea,
) -> Result<ModelStagingArea, ModelInstallError> {
    // 1) owner 绑定核对（owner root / owner revision / 路径必须是 root 直接子目录）；
    //    不匹配时 `verify_owner_binding` 内部已解除清理责任。
    let staging = staging
        .verify_owner_binding(root_dir, engine_revision)
        .map_err(|detail| ModelInstallError::StagingRejected { detail })?;
    // 2) 复用暂存全名校验：名字必须精确 `.tmp-<expected_revision>-<pid>-<seq>`。
    let staging_name = staging
        .path()
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned);
    let Some(staging_name) = staging_name else {
        let detail = format!("暂存目录名无效：{}", staging.path().display());
        return Err(reject_owned_staging(staging, detail));
    };
    if !install::staging_name_matches(&staging_name, engine_revision) {
        return Err(reject_owned_staging(
            staging,
            format!("暂存目录名必须精确匹配 .tmp-{engine_revision}-<pid>-<seq>：{staging_name}"),
        ));
    }
    // 3) root 本体复验（存在、非符号链接、是目录）；同样 NEVER 删除。
    if let Err(root_error) = install::require_existing_root(root_dir) {
        staging.reject_without_cleanup();
        return Err(root_error);
    }
    Ok(staging)
}

/// 绑定后准入失败：解除清理责任并组装 typed 拒绝（NEVER 删除被拒路径）。
fn reject_owned_staging(staging: ModelStagingArea, detail: String) -> ModelInstallError {
    staging.reject_without_cleanup();
    ModelInstallError::StagingRejected { detail }
}

#[async_trait]
impl ModelInstallerPort for LocalModelAssetStore {
    async fn create_staging(&self) -> Result<ModelStagingArea, ModelInstallPortError> {
        let staging: StagingDirectory = self.create_staging_directory().await?;
        // RAII 责任从存储句柄转移到端口句柄（路径不变，owner 绑定写入，armed 状态守恒）。
        Ok(ModelStagingArea::armed(
            staging.into_path(),
            self.root_dir.to_path_buf(),
            self.expected_manifest.engine_revision.clone(),
        ))
    }

    async fn install_staged(
        &self,
        staging: ModelStagingArea,
    ) -> Result<StagedInstallOutcome, ModelInstallPortError> {
        // 绑定 + 全名 + root 复验（不匹配 typed 拒绝且 NEVER 删除）。
        let staging = verify_owned_staging(
            &self.root_dir,
            &self.expected_manifest.engine_revision,
            staging,
        )?;
        let staging_path = staging.path().to_path_buf();
        let staging_directory = match self.adopt_staging_directory(staging_path) {
            Ok(adopted) => {
                let _transferred = staging.into_path(); // adopt 已接管 RAII 责任
                adopted
            }
            Err(rejection) => {
                // 绑定已核对、暂存确认为自有：fs 准入复核（TOCTOU）失败时
                // 先显式异步清理再报错（不依赖 Drop）。
                let mut port_error = ModelInstallPortError::from(rejection);
                if let Err(cleanup_error) = self.discard_staging(staging).await {
                    port_error
                        .detail
                        .push_str(&format!("；暂存目录清理失败：{cleanup_error}"));
                }
                return Err(port_error);
            }
        };
        let prepared = self.prepare_staged_install(staging_directory).await?;
        match prepared {
            PreparedStagedInstall::FinalAlreadyValid(installed) => {
                Ok(StagedInstallOutcome::AlreadyValid(installed))
            }
            ready @ PreparedStagedInstall::ReadyToCommit(_) => {
                let installed = self.commit_prepared_install(ready).await?;
                Ok(StagedInstallOutcome::Installed(installed))
            }
        }
    }

    async fn discard_staging(
        &self,
        staging: ModelStagingArea,
    ) -> Result<(), ModelInstallPortError> {
        let root_dir = self.root_dir.clone();
        let engine_revision = self.expected_manifest.engine_revision.clone();
        let task = tokio::task::spawn_blocking(move || {
            // 闭包内先复验 root / path / 全名 / owner 绑定，通过才删；
            // 任一不匹配 typed 拒绝且 NEVER 删除（token 已解除清理责任）。
            let verified = verify_owned_staging(&root_dir, &engine_revision, staging)
                .map_err(ModelInstallPortError::from)?;
            // 通过：移交路径（解除 RAII），删除由本闭包显式完成；
            // 闭包未执行即结束时端口句柄 `Drop` 兜底清理。
            let staging_path = verified.into_path();
            crate::ports::remove_staged_directory(&staging_path).map_err(|error| {
                ModelInstallPortError {
                    kind: ModelInstallPortErrorKind::StagingDiscardFailed,
                    detail: format!("暂存目录清理失败：{error}"),
                }
            })
        });
        match task.await {
            Ok(outcome) => outcome,
            Err(error) => Err(ModelInstallPortError {
                kind: ModelInstallPortErrorKind::TaskFailed,
                detail: format!("暂存清理任务执行失败：{error}"),
            }),
        }
    }
}

#[cfg(all(test, unix))]
#[path = "model_assets_installer_port_tests.rs"]
mod tests;
