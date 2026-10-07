//! `ModelInstallerPort` 实现测试：端口 staging token 的创建/显式清理、
//! 安装结果幂等分类与 typed 错误映射（真实 `LocalModelAssetStore` 落地行为）。

use super::*;

use std::path::{Path, PathBuf};

use crate::constants::{MODEL_MANIFEST_FILE_NAME, STAGING_DIR_PREFIX};
use crate::domain::{required_platform, ModelAsset, ModelManifest};
use crate::ports::{
    ModelInstallPortError, ModelInstallPortErrorKind, ModelInstallerPort, ModelStagingArea,
    StagedInstallOutcome,
};

/// 测试用 engine_revision（canonical segment）。
const TEST_ENGINE_REVISION: &str = "e83f5c1a9d2b4f6a7c0e1d2b3a4f5c6d7e8f9a0b";

/// 生产布局的三类小体积夹具资产。
fn fixture_payloads() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("model.gguf", b"GGUF-TEST-WEIGHTS-CONTENT".to_vec()),
        ("pointer_head.safetensors", vec![7_u8; 64]),
        ("tokenizer/merges.txt", b"line-one\nline-two\n".to_vec()),
    ]
}

/// 按夹具字节构造契约 manifest。
fn fixture_manifest() -> ModelManifest {
    let assets = fixture_payloads()
        .into_iter()
        .map(|(path, payload)| ModelAsset {
            url: format!("https://example.com/{path}"),
            byte_length: payload.len() as u64,
            sha256: utils::sha256_hex(&payload),
            path: path.to_owned(),
        })
        .collect();
    ModelManifest {
        schema_version: 1,
        engine_revision: TEST_ENGINE_REVISION.to_owned(),
        hidden_size: 1024,
        pointer_dimension: 256,
        temperature: 0.07,
        supported_platforms: vec![required_platform().to_owned()],
        assets,
    }
}

/// 构造临时安装根上的 store。
fn store_at(root_dir: &Path) -> LocalModelAssetStore {
    LocalModelAssetStore::new(root_dir.to_path_buf(), fixture_manifest()).expect("契约 manifest")
}

/// root 下全部暂存目录。
fn staging_directories(root_dir: &Path) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(root_dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(STAGING_DIR_PREFIX))
        })
        .collect()
}

/// 把全部夹具资产写入暂存目录。
fn stage_complete_assets(staging_root: &Path, manifest: &ModelManifest) {
    for (path, payload) in fixture_payloads() {
        let file_path = staging_root.join(path);
        std::fs::create_dir_all(file_path.parent().expect("资产应有父目录")).expect("父目录创建");
        std::fs::write(file_path, payload).expect("资产写入");
    }
    let serialized = serde_json::to_string_pretty(manifest).expect("manifest 序列化");
    std::fs::write(staging_root.join(MODEL_MANIFEST_FILE_NAME), serialized).expect("manifest 写入");
}

#[tokio::test]
async fn create_staging_yields_hidden_direct_child_and_discard_removes_it_async() {
    let temp_dir = tempfile::tempdir().expect("临时目录");
    let root_dir = temp_dir.path().join("systemone");
    let store = store_at(&root_dir);

    let staging: ModelStagingArea = store.create_staging().await.expect("创建暂存区");
    assert_eq!(
        staging.path().parent(),
        Some(root_dir.as_path()),
        "staging MUST 是安装根的直接子目录"
    );
    assert!(
        staging
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(STAGING_DIR_PREFIX)),
        "staging MUST 带隐藏前缀"
    );
    std::fs::write(staging.path().join("model.gguf"), b"bytes").expect("写入暂存内容");
    let staging_name = staging
        .path()
        .file_name()
        .and_then(|name| name.to_str())
        .expect("暂存目录名")
        .to_owned();
    assert!(
        install::staging_name_matches(&staging_name, TEST_ENGINE_REVISION),
        "暂存全名 MUST 精确匹配 .tmp-<revision>-<pid>-<seq>：{staging_name}"
    );

    store
        .discard_staging(staging)
        .await
        .expect("显式清理应成功");
    assert!(
        staging_directories(&root_dir).is_empty(),
        "显式异步清理 MUST 删除整个暂存目录"
    );
}

#[tokio::test]
async fn discard_staging_is_idempotent_when_directory_already_gone() {
    let temp_dir = tempfile::tempdir().expect("临时目录");
    let root_dir = temp_dir.path().join("systemone");
    let store = store_at(&root_dir);

    let staging: ModelStagingArea = store.create_staging().await.expect("创建暂存区");
    std::fs::remove_dir_all(staging.path()).expect("先行手动删除");
    store
        .discard_staging(staging)
        .await
        .expect("已不存在的暂存目录清理应幂等成功");
}

#[tokio::test]
async fn install_staged_with_complete_valid_assets_commits_final() {
    let temp_dir = tempfile::tempdir().expect("临时目录");
    let root_dir = temp_dir.path().join("systemone");
    let store = store_at(&root_dir);
    let manifest = fixture_manifest();

    let staging = store.create_staging().await.expect("创建暂存区");
    stage_complete_assets(staging.path(), &manifest);
    let outcome = store.install_staged(staging).await.expect("安装应成功");

    assert!(matches!(outcome, StagedInstallOutcome::Installed(_)));
    let StagedInstallOutcome::Installed(installed) = outcome else {
        panic!("应为 Installed");
    };
    assert_eq!(installed.manifest().engine_revision, TEST_ENGINE_REVISION);
    assert!(root_dir.join(TEST_ENGINE_REVISION).is_dir());
    assert!(
        staging_directories(&root_dir).is_empty(),
        "提交后 staging 应消失"
    );
}

#[tokio::test]
async fn install_staged_when_final_already_valid_returns_already_valid_without_overwrite() {
    let temp_dir = tempfile::tempdir().expect("临时目录");
    let root_dir = temp_dir.path().join("systemone");
    let store = store_at(&root_dir);
    let manifest = fixture_manifest();
    // 已存在的有效安装（读路径可识别）。
    let revision_dir = root_dir.join(TEST_ENGINE_REVISION);
    std::fs::create_dir_all(&revision_dir).expect("revision 创建");
    for (path, payload) in fixture_payloads() {
        let file_path = revision_dir.join(path);
        std::fs::create_dir_all(file_path.parent().expect("父目录")).expect("父目录创建");
        std::fs::write(file_path, payload).expect("资产写入");
    }
    let serialized = serde_json::to_string_pretty(&manifest).expect("序列化");
    std::fs::write(revision_dir.join(MODEL_MANIFEST_FILE_NAME), serialized).expect("manifest 写入");
    std::fs::write(revision_dir.join("competitor.sentinel"), b"keep").expect("哨兵写入");

    let staging = store.create_staging().await.expect("创建暂存区");
    stage_complete_assets(staging.path(), &manifest);
    let outcome = store.install_staged(staging).await.expect("幂等成功");

    assert!(matches!(outcome, StagedInstallOutcome::AlreadyValid(_)));
    assert!(
        revision_dir.join("competitor.sentinel").is_file(),
        "已有有效 revision MUST NOT 被覆盖"
    );
    assert!(
        staging_directories(&root_dir).is_empty(),
        "staging 应已清理"
    );
}

#[tokio::test]
async fn install_staged_when_final_invalid_maps_to_install_conflict_and_keeps_final() {
    let temp_dir = tempfile::tempdir().expect("临时目录");
    let root_dir = temp_dir.path().join("systemone");
    let store = store_at(&root_dir);
    let manifest = fixture_manifest();
    // 已存在但无效的 final（缺 manifest.json）。
    let revision_dir = root_dir.join(TEST_ENGINE_REVISION);
    std::fs::create_dir_all(revision_dir.join("tokenizer")).expect("目录创建");
    std::fs::write(revision_dir.join("model.gguf"), b"junk").expect("无效内容写入");

    let staging = store.create_staging().await.expect("创建暂存区");
    stage_complete_assets(staging.path(), &manifest);
    let error = store
        .install_staged(staging)
        .await
        .expect_err("无效 final MUST 拒绝覆盖");

    assert_eq!(error.kind, ModelInstallPortErrorKind::InstallConflict);
    assert_eq!(
        std::fs::read(revision_dir.join("model.gguf")).expect("可读"),
        b"junk",
        "无效 final MUST NOT 被覆盖或删除"
    );
    assert!(
        !revision_dir.join(MODEL_MANIFEST_FILE_NAME).exists(),
        "final MUST 未被本次安装改写"
    );
    assert!(
        staging_directories(&root_dir).is_empty(),
        "staging 应已清理"
    );
}

#[tokio::test]
async fn install_staged_verification_failure_maps_typed_kind_and_cleans_staging() {
    let temp_dir = tempfile::tempdir().expect("临时目录");
    let root_dir = temp_dir.path().join("systemone");
    let store = store_at(&root_dir);

    // 暂存只写了一个资产（结构不完整）→ 最终校验失败。
    let staging = store.create_staging().await.expect("创建暂存区");
    std::fs::create_dir_all(staging.path().join("tokenizer")).expect("父目录创建");
    std::fs::write(staging.path().join("model.gguf"), b"only-one").expect("写入");
    let error = store
        .install_staged(staging)
        .await
        .expect_err("不完整暂存 MUST 校验失败");

    assert_eq!(error.kind, ModelInstallPortErrorKind::VerificationFailed);
    assert!(
        !root_dir.join(TEST_ENGINE_REVISION).exists(),
        "失败 MUST 不出现 final"
    );
    assert!(
        staging_directories(&root_dir).is_empty(),
        "失败 MUST 清理 staging"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn create_staging_on_symlinked_root_maps_to_staging_unavailable() {
    let temp_dir = tempfile::tempdir().expect("临时目录");
    let external_dir = tempfile::tempdir().expect("外部目录");
    let root_dir = temp_dir.path().join("systemone");
    std::os::unix::fs::symlink(external_dir.path(), &root_dir).expect("创建 root 符号链接");
    let store = store_at(&root_dir);

    let error = store
        .create_staging()
        .await
        .expect_err("符号链接安装根 MUST 拒绝创建暂存");

    assert_eq!(error.kind, ModelInstallPortErrorKind::StagingUnavailable);
    assert!(
        std::fs::read_dir(external_dir.path())
            .expect("外部目录可读")
            .next()
            .is_none(),
        "MUST NOT 沿符号链接创建任何目录"
    );
}

/// crate 测试辅助：伪装配一个端口 staging token（模拟跨 store / 跨 revision /
/// 越界路径伪造）。生产形态该构造只有安装 adapter 可达。
fn forge_staging_token(
    path: PathBuf,
    owner_root: PathBuf,
    owner_revision: &str,
) -> ModelStagingArea {
    ModelStagingArea::armed(path, owner_root, owner_revision.to_owned())
}

#[tokio::test]
async fn cross_store_forged_token_is_rejected_without_deleting_foreign_staging() {
    let temp_dir = tempfile::tempdir().expect("临时目录");
    let root_a = temp_dir.path().join("systemone-a");
    let root_b = temp_dir.path().join("systemone-b");
    let store_a = store_at(&root_a);
    let store_b = store_at(&root_b);

    // store B 创建的真实暂存（移交路径但不删除，保留 B 的目录作为「外国」路径）。
    let staging_b: ModelStagingArea = store_b.create_staging().await.expect("B 创建暂存");
    let path_b = staging_b.into_path();

    // 伪造：owner 绑定谎称为 store A（路径实际在 store B 下），分别过 install / discard。
    let forged_install = forge_staging_token(path_b.clone(), root_a.clone(), TEST_ENGINE_REVISION);
    let error = store_a
        .install_staged(forged_install)
        .await
        .expect_err("跨 store token MUST 被拒绝");
    assert_eq!(error.kind, ModelInstallPortErrorKind::StagingRejected);
    assert!(path_b.is_dir(), "被拒跨 store 路径 MUST 未被删除");

    let forged_discard = forge_staging_token(path_b.clone(), root_a.clone(), TEST_ENGINE_REVISION);
    let error = store_a
        .discard_staging(forged_discard)
        .await
        .expect_err("跨 store token MUST 被拒绝");
    assert_eq!(error.kind, ModelInstallPortErrorKind::StagingRejected);
    assert!(path_b.is_dir(), "被拒跨 store 路径 MUST 未被删除");
}

#[tokio::test]
async fn cross_revision_forged_tokens_are_rejected_without_deleting_staging() {
    let temp_dir = tempfile::tempdir().expect("临时目录");
    let root_dir = temp_dir.path().join("systemone");
    let store = store_at(&root_dir);

    // 真实暂存（保证 root 已存在；移交路径保留目录）。
    let staging: ModelStagingArea = store.create_staging().await.expect("创建暂存区");
    let path = staging.into_path();

    // 伪造 1：owner revision 与当前期望不符。
    let forged_owner = forge_staging_token(path.clone(), root_dir.clone(), "ffffffffffffffff");
    let error = store
        .install_staged(forged_owner)
        .await
        .expect_err("跨 revision owner MUST 被拒绝");
    assert_eq!(error.kind, ModelInstallPortErrorKind::StagingRejected);
    assert!(path.is_dir(), "被拒跨 revision 路径 MUST 未被删除");

    // 伪造 2：owner 绑定谎称为当前 revision，但目录名携带其他 revision
    // （owner 核对通过后由全名精确校验拦截）。
    let forged_name = root_dir.join(format!(
        "{STAGING_DIR_PREFIX}other-revision-{}-1",
        std::process::id()
    ));
    std::fs::create_dir(&forged_name).expect("伪暂存目录创建");
    let forged_path = forged_name.clone();
    let forged_name_token =
        forge_staging_token(forged_name, root_dir.clone(), TEST_ENGINE_REVISION);
    let error = store
        .discard_staging(forged_name_token)
        .await
        .expect_err("全名不符的伪暂存 MUST 被拒绝");
    assert_eq!(error.kind, ModelInstallPortErrorKind::StagingRejected);
    assert!(forged_path.is_dir(), "全名不符的被拒路径 MUST 未被删除");
    assert!(path.is_dir(), "真实暂存 MUST 未被删除");
}

#[tokio::test]
async fn out_of_bounds_token_path_is_rejected_and_never_deleted() {
    let temp_dir = tempfile::tempdir().expect("临时目录");
    let root_dir = temp_dir.path().join("systemone");
    let store = store_at(&root_dir);
    // 先建 root（真实暂存触发创建后立即释放路径，只保留目录形态检查）。
    let staging: ModelStagingArea = store.create_staging().await.expect("创建暂存区");
    let owned_path = staging.into_path();

    // 越界路径：root 之外的目录，owner 绑定谎称为当前 store。
    let external = temp_dir.path().join("outside-root");
    std::fs::create_dir(&external).expect("越界目录创建");
    std::fs::write(external.join("precious.txt"), b"keep").expect("越界内容写入");

    let forged_install =
        forge_staging_token(external.clone(), root_dir.clone(), TEST_ENGINE_REVISION);
    let error = store
        .install_staged(forged_install)
        .await
        .expect_err("越界路径 MUST 被拒绝");
    assert_eq!(error.kind, ModelInstallPortErrorKind::StagingRejected);
    assert!(
        external.join("precious.txt").is_file(),
        "越界路径 MUST 未被删除"
    );

    let forged_discard =
        forge_staging_token(external.clone(), root_dir.clone(), TEST_ENGINE_REVISION);
    let error = store
        .discard_staging(forged_discard)
        .await
        .expect_err("越界路径 MUST 被拒绝");
    assert_eq!(error.kind, ModelInstallPortErrorKind::StagingRejected);
    assert!(
        external.join("precious.txt").is_file(),
        "越界路径 MUST 未被删除"
    );
    assert!(owned_path.is_dir(), "真实暂存 MUST 未被删除");
}

#[test]
fn model_install_errors_map_precisely_to_port_kinds() {
    use crate::ports::InvalidAssetKind;
    let cases: Vec<(ModelInstallError, ModelInstallPortErrorKind)> = vec![
        // IO 类：暂存写入、final 探测即失败、提交 IO 失败 → InstallIoFailed。
        (
            ModelInstallError::StagingWriteFailed {
                detail: "manifest 写入失败".to_owned(),
            },
            ModelInstallPortErrorKind::InstallIoFailed,
        ),
        (
            ModelInstallError::FinalUnprobeable {
                detail: "探测失败".to_owned(),
            },
            ModelInstallPortErrorKind::InstallIoFailed,
        ),
        (
            ModelInstallError::CommitIoFailed {
                detail: "rename 失败".to_owned(),
            },
            ModelInstallPortErrorKind::InstallIoFailed,
        ),
        // 冲突类：只有 final 已存在但无效 / 原子提交冲突 → InstallConflict。
        (
            ModelInstallError::FinalInvalid {
                detail: "已存在但无效".to_owned(),
            },
            ModelInstallPortErrorKind::InstallConflict,
        ),
        (
            ModelInstallError::RenameConflict {
                detail: "提交冲突".to_owned(),
            },
            ModelInstallPortErrorKind::InstallConflict,
        ),
        (
            ModelInstallError::VerificationFailed {
                kind: InvalidAssetKind::Corrupt,
                detail: "SHA 不符".to_owned(),
            },
            ModelInstallPortErrorKind::VerificationFailed,
        ),
        (
            ModelInstallError::StagingRejected {
                detail: "绑定不符".to_owned(),
            },
            ModelInstallPortErrorKind::StagingRejected,
        ),
        (
            ModelInstallError::StagingCreateFailed {
                detail: "创建失败".to_owned(),
            },
            ModelInstallPortErrorKind::StagingUnavailable,
        ),
        (
            ModelInstallError::RootInvalid {
                detail: "root 无效".to_owned(),
            },
            ModelInstallPortErrorKind::StagingUnavailable,
        ),
        (
            ModelInstallError::TaskJoinFailed {
                detail: "任务失败".to_owned(),
            },
            ModelInstallPortErrorKind::TaskFailed,
        ),
    ];
    for (error, expected) in cases {
        let mapped = ModelInstallPortError::from(error);
        assert_eq!(
            mapped.kind, expected,
            "typed 错误映射不符：detail={}",
            mapped.detail
        );
        assert!(!mapped.detail.is_empty(), "映射后 detail 不得丢失");
    }
}

#[tokio::test]
async fn root_and_staging_directories_are_created_owner_only_on_unix() {
    use std::os::unix::fs::PermissionsExt;
    let temp_dir = tempfile::tempdir().expect("临时目录");
    let root_dir = temp_dir.path().join("systemone");
    let store = store_at(&root_dir);

    let staging: ModelStagingArea = store.create_staging().await.expect("创建暂存区");

    let root_mode = std::fs::metadata(&root_dir)
        .expect("root 元数据")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(root_mode, 0o700, "生产 root MUST 以 mode 0700 创建");

    let staging_mode = std::fs::metadata(staging.path())
        .expect("暂存元数据")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(staging_mode, 0o700, "生产暂存目录 MUST 以 mode 0700 创建");

    store
        .discard_staging(staging)
        .await
        .expect("显式清理应成功");
}
