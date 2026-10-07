//! LocalModelAssetStore 行为测试：三态解析（Missing/Installed/Invalid）
//! 与原子安装（暂存 RAII、幂等、fail-closed、no-replace rename 冲突）。
//!
//! 每个用例使用独立唯一 TempDir；资产为小字节夹具，不依赖网络与真实大模型文件。

use super::*;
use crate::constants::{MODEL_MANIFEST_FILE_NAME, STAGING_DIR_PREFIX};
use crate::domain::{required_platform, ModelAsset, ModelManifest};
use crate::ports::{ModelAssetPort, ModelAssetState};

/// 测试用 engine_revision（canonical segment：字母数字开头、无 `..`）。
const TEST_ENGINE_REVISION: &str = "e83f5c1a9d2b4f6a7c0e1d2b3a4f5c6d7e8f9a0b";

/// 生产布局的三类小体积夹具资产（model.gguf / pointer_head / tokenizer）。
fn fixture_payloads() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("model.gguf", b"GGUF-TEST-WEIGHTS-CONTENT".to_vec()),
        ("pointer_head.safetensors", vec![7_u8; 64]),
        ("tokenizer/merges.txt", b"line-one\nline-two\n".to_vec()),
    ]
}

/// 按夹具字节内容构造契约 manifest（长度与 SHA-256 均来自真实字节，单一来源）。
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

fn store_at(root_dir: std::path::PathBuf) -> LocalModelAssetStore {
    LocalModelAssetStore::new(root_dir, fixture_manifest()).expect("契约 manifest 应通过校验")
}

/// 把夹具资产字节写入目标目录（自动创建 tokenizer/ 等父目录）。
fn write_payloads(directory: &Path, payloads: &[(&str, Vec<u8>)]) {
    for (path, payload) in payloads {
        let file_path = directory.join(path);
        std::fs::create_dir_all(file_path.parent().expect("资产应有父目录")).expect("父目录创建");
        std::fs::write(file_path, payload).expect("资产写入");
    }
}

/// 写入 canonical manifest.json（与生产安装相同的序列化形态）。
fn write_manifest(directory: &Path, manifest: &ModelManifest) {
    let serialized = serde_json::to_string_pretty(manifest).expect("manifest 序列化");
    std::fs::write(directory.join(MODEL_MANIFEST_FILE_NAME), serialized).expect("manifest 写入");
}

/// 在 root 下手工构造完整有效的 revision 安装树（覆盖读路径，不经过安装入口）。
fn write_installed_tree(root_dir: &Path, manifest: &ModelManifest) -> std::path::PathBuf {
    let revision_dir = root_dir.join(&manifest.engine_revision);
    std::fs::create_dir_all(&revision_dir).expect("revision 目录创建");
    write_payloads(&revision_dir, &fixture_payloads());
    write_manifest(&revision_dir, manifest);
    revision_dir
}

/// root 下所有 `.tmp-` 暂存子目录（断言无半成品残留）。
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

/// 组装一个完整可用的暂存目录（资产 + canonical manifest）。
fn stage_complete_assets(staging_path: &Path) {
    write_payloads(staging_path, &fixture_payloads());
    write_manifest(staging_path, &fixture_manifest());
}

#[tokio::test]
async fn absent_install_root_or_revision_resolves_missing() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let root_dir = temp.path().join("models").join("systemone");
    let store = store_at(root_dir.clone());

    // root 完全不存在 → Missing
    assert_eq!(store.installed_assets().await, ModelAssetState::Missing);

    // root 存在但 revision 目录不存在 → 仍是 Missing
    std::fs::create_dir_all(&root_dir).expect("root 创建");
    assert_eq!(store.installed_assets().await, ModelAssetState::Missing);
}

#[tokio::test]
async fn symlinked_install_root_resolves_invalid_fail_closed() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let real_root = temp.path().join("real-root");
    std::fs::create_dir_all(&real_root).expect("真实 root 创建");
    // root 是指向真实目录的符号链接：NEVER 穿过链接识别安装（即使背后一切有效）。
    let link_root = temp.path().join("link-root");
    std::os::unix::fs::symlink(&real_root, &link_root).expect("root 符号链接创建");
    let store = store_at(link_root);

    let state = store.installed_assets().await;
    let ModelAssetState::Invalid { kind, detail } = state else {
        panic!("root 符号链接应为 Invalid，实际：{state:?}");
    };
    assert_eq!(kind, InvalidAssetKind::Unsupported);
    assert!(
        detail.contains("符号链接"),
        "detail 应说明符号链接：{detail}"
    );
}

#[tokio::test]
async fn fully_valid_revision_resolves_installed_with_getters() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let root_dir = temp.path().join("models").join("systemone");
    let manifest = fixture_manifest();
    let revision_dir = write_installed_tree(&root_dir, &manifest);
    let store = store_at(root_dir);

    let state = store.installed_assets().await;
    let ModelAssetState::Installed(installed) = state else {
        panic!("有效安装应为 Installed，实际：{state:?}");
    };
    assert_eq!(installed.manifest(), &manifest);
    assert_eq!(installed.manifest().engine_revision, TEST_ENGINE_REVISION);
    assert_eq!(installed.manifest().assets.len(), 3);
    assert_eq!(installed.install_root(), revision_dir.as_path());
}

#[tokio::test]
async fn invalid_manifest_json_resolves_invalid_as_corrupt() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let manifest = fixture_manifest();
    let revision_dir = write_installed_tree(temp.path(), &manifest);
    // 用非法 JSON 覆盖 manifest.json（资产保持在位）
    std::fs::write(
        revision_dir.join(MODEL_MANIFEST_FILE_NAME),
        "{ 这不是合法 JSON",
    )
    .expect("manifest 覆写");
    let store = store_at(temp.path().to_path_buf());

    let state = store.installed_assets().await;
    let ModelAssetState::Invalid { kind, detail } = state else {
        panic!("非法 manifest 应为 Invalid，实际：{state:?}");
    };
    assert_eq!(kind, InvalidAssetKind::Corrupt);
    assert!(
        detail.contains("manifest"),
        "detail 应指向 manifest：{detail}"
    );
}

#[tokio::test]
async fn installed_manifest_mismatching_expected_resolves_invalid() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let manifest = fixture_manifest();
    let revision_dir = write_installed_tree(temp.path(), &manifest);

    // 情形一：同 revision 但 manifest 内容与期望不一致（URL 变更）。
    let mut drifted_manifest = manifest.clone();
    drifted_manifest.assets[0].url = "https://example.com/other/model.gguf".to_owned();
    write_manifest(&revision_dir, &drifted_manifest);
    let store = store_at(temp.path().to_path_buf());
    let state = store.installed_assets().await;
    let ModelAssetState::Invalid { kind, detail } = state else {
        panic!("manifest 不一致应为 Invalid，实际：{state:?}");
    };
    assert_eq!(kind, InvalidAssetKind::Unsupported);
    assert!(detail.contains("不一致"), "detail 应说明不一致：{detail}");

    // 情形二：落盘 manifest 的 revision 与目录目标不符。
    let mut other_revision_manifest = manifest.clone();
    other_revision_manifest.engine_revision = "0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c".to_owned();
    write_manifest(&revision_dir, &other_revision_manifest);
    let state = store.installed_assets().await;
    let ModelAssetState::Invalid { kind, detail } = state else {
        panic!("revision 不符应为 Invalid，实际：{state:?}");
    };
    assert_eq!(kind, InvalidAssetKind::Unsupported);
    assert!(
        detail.contains("revision"),
        "detail 应说明 revision：{detail}"
    );
}

#[tokio::test]
async fn missing_asset_resolves_invalid_corrupt_naming_the_path() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let manifest = fixture_manifest();
    let revision_dir = write_installed_tree(temp.path(), &manifest);
    std::fs::remove_file(revision_dir.join("tokenizer/merges.txt")).expect("删除资产");
    let store = store_at(temp.path().to_path_buf());

    let state = store.installed_assets().await;
    let ModelAssetState::Invalid { kind, detail } = state else {
        panic!("资产缺失应为 Invalid，实际：{state:?}");
    };
    assert_eq!(kind, InvalidAssetKind::Corrupt);
    assert!(
        detail.contains("tokenizer/merges.txt"),
        "detail 应指向缺失资产：{detail}"
    );
}

#[tokio::test]
async fn symlinked_tokenizer_parent_resolves_invalid_fail_closed() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let manifest = fixture_manifest();
    let revision_dir = write_installed_tree(temp.path(), &manifest);

    // tokenizer/ 换成指向外部目录的符号链接——外部目录里甚至放着「有效」资产，
    // 校验器也必须逐组件拒绝，NEVER 穿过中间目录的链接。
    let outside_tokenizer = temp.path().join("outside-tokenizer");
    std::fs::create_dir_all(&outside_tokenizer).expect("外部 tokenizer 目录");
    std::fs::write(
        outside_tokenizer.join("merges.txt"),
        fixture_payloads()[2].1.clone(),
    )
    .expect("外部资产写入");
    std::fs::remove_dir_all(revision_dir.join("tokenizer")).expect("删除原 tokenizer 目录");
    std::os::unix::fs::symlink(&outside_tokenizer, revision_dir.join("tokenizer"))
        .expect("创建父目录符号链接");

    let store = store_at(temp.path().to_path_buf());
    let state = store.installed_assets().await;
    let ModelAssetState::Invalid { kind, detail } = state else {
        panic!("父目录符号链接应为 Invalid，实际：{state:?}");
    };
    assert_eq!(kind, InvalidAssetKind::Corrupt);
    assert!(
        detail.contains("符号链接"),
        "detail 应说明符号链接：{detail}"
    );
    assert!(
        detail.contains("tokenizer"),
        "detail 应指向父目录：{detail}"
    );
}

#[tokio::test]
async fn symlinked_manifest_or_asset_resolves_invalid_fail_closed() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let manifest = fixture_manifest();

    // 情形一：manifest.json 是指向真实合法文件的符号链接（依然拒绝）。
    let revision_dir = write_installed_tree(temp.path(), &manifest);
    let outside_manifest = temp.path().join("outside-manifest.json");
    let real_manifest =
        std::fs::read(revision_dir.join(MODEL_MANIFEST_FILE_NAME)).expect("读取原 manifest");
    std::fs::write(&outside_manifest, real_manifest).expect("写入外部 manifest");
    std::fs::remove_file(revision_dir.join(MODEL_MANIFEST_FILE_NAME)).expect("删除原 manifest");
    std::os::unix::fs::symlink(
        &outside_manifest,
        revision_dir.join(MODEL_MANIFEST_FILE_NAME),
    )
    .expect("创建 manifest 符号链接");
    let store = store_at(temp.path().to_path_buf());
    let state = store.installed_assets().await;
    let ModelAssetState::Invalid { kind, detail } = state else {
        panic!("manifest 符号链接应为 Invalid，实际：{state:?}");
    };
    assert!(
        matches!(
            kind,
            InvalidAssetKind::Corrupt | InvalidAssetKind::Unsupported
        ),
        "符号链接应为 Corrupt/Unsupported：{kind:?}"
    );
    assert!(
        detail.contains("符号链接"),
        "detail 应说明符号链接：{detail}"
    );

    // 情形二：资产文件被换成符号链接。
    let symlink_revision_dir = temp.path().join("symlink-case");
    let symlink_store = store_at(symlink_revision_dir.clone());
    let manifest = fixture_manifest();
    let asset_revision_dir = write_installed_tree(&symlink_revision_dir, &manifest);
    let sidecar_file = temp.path().join("sidecar.gguf");
    std::fs::write(&sidecar_file, fixture_payloads()[0].1.clone()).expect("写入边车文件");
    std::fs::remove_file(asset_revision_dir.join("model.gguf")).expect("删除原资产");
    std::os::unix::fs::symlink(&sidecar_file, asset_revision_dir.join("model.gguf"))
        .expect("创建资产符号链接");
    let state = symlink_store.installed_assets().await;
    let ModelAssetState::Invalid { kind, detail } = state else {
        panic!("资产符号链接应为 Invalid，实际：{state:?}");
    };
    assert_eq!(kind, InvalidAssetKind::Corrupt);
    assert!(
        detail.contains("符号链接"),
        "detail 应说明符号链接：{detail}"
    );

    // 情形三：资产路径是目录而非常规文件。
    let directory_asset_root = temp.path().join("directory-case");
    let directory_store = store_at(directory_asset_root.clone());
    let manifest = fixture_manifest();
    let asset_revision_dir = write_installed_tree(&directory_asset_root, &manifest);
    std::fs::remove_file(asset_revision_dir.join("pointer_head.safetensors")).expect("删除资产");
    std::fs::create_dir(asset_revision_dir.join("pointer_head.safetensors")).expect("创建同名目录");
    let state = directory_store.installed_assets().await;
    let ModelAssetState::Invalid { .. } = state else {
        panic!("非常规文件应为 Invalid，实际：{state:?}");
    };
}

#[tokio::test]
async fn asset_byte_length_mismatch_resolves_invalid_corrupt() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let manifest = fixture_manifest();
    let revision_dir = write_installed_tree(temp.path(), &manifest);
    let mut payload = fixture_payloads()[0].1.clone();
    payload.push(0xFF); // 长度 +1，SHA 必然也变——先按长度判定
    std::fs::write(revision_dir.join("model.gguf"), payload).expect("覆写资产");
    let store = store_at(temp.path().to_path_buf());

    let state = store.installed_assets().await;
    let ModelAssetState::Invalid { kind, detail } = state else {
        panic!("字节数不符应为 Invalid，实际：{state:?}");
    };
    assert_eq!(kind, InvalidAssetKind::Corrupt);
    assert!(detail.contains("字节数"), "detail 应说明字节数：{detail}");
    assert!(detail.contains("model.gguf"), "detail 应指向资产：{detail}");
}

#[tokio::test]
async fn asset_sha256_mismatch_resolves_invalid_corrupt() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let manifest = fixture_manifest();
    let revision_dir = write_installed_tree(temp.path(), &manifest);
    let mut payload = fixture_payloads()[0].1.clone();
    let flipped_index = 3;
    payload[flipped_index] ^= 0x01; // 长度不变，仅内容变化 → 只有 SHA 能发现
    std::fs::write(revision_dir.join("model.gguf"), payload).expect("覆写资产");
    let store = store_at(temp.path().to_path_buf());

    let state = store.installed_assets().await;
    let ModelAssetState::Invalid { kind, detail } = state else {
        panic!("SHA 不符应为 Invalid，实际：{state:?}");
    };
    assert_eq!(kind, InvalidAssetKind::Corrupt);
    assert!(detail.contains("SHA-256"), "detail 应说明 SHA：{detail}");
    assert!(detail.contains("model.gguf"), "detail 应指向资产：{detail}");
}

#[tokio::test]
async fn half_staged_assets_are_invisible_and_resolve_missing() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let store = store_at(temp.path().join("systemone-models"));

    // 暂存目录只写了一半资产（甚至没有 manifest）→ 运行时看不到任何安装。
    let staging = store.create_staging_directory().await.expect("暂存目录");
    write_payloads(staging.path(), &fixture_payloads()[..1]);
    assert_eq!(store.installed_assets().await, ModelAssetState::Missing);

    // 即使暂存目录已写入完整资产与 manifest，未 commit 前依然不可见。
    write_manifest(staging.path(), &fixture_manifest());
    assert_eq!(store.installed_assets().await, ModelAssetState::Missing);

    // 句柄丢弃（Drop 兜底）→ 暂存目录被清理。
    let staging_path = staging.path().to_path_buf();
    drop(staging);
    assert!(!staging_path.exists(), "句柄 Drop 应清理暂存目录");
}

#[tokio::test]
async fn created_staging_directories_are_unique_hidden_direct_children() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let store = store_at(temp.path().join("systemone-models"));

    let first_staging = store.create_staging_directory().await.expect("首个暂存");
    let second_staging = store.create_staging_directory().await.expect("次个暂存");
    assert_ne!(
        first_staging.path(),
        second_staging.path(),
        "暂存目录必须唯一"
    );
    for staging in [&first_staging, &second_staging] {
        let name = staging
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .expect("名称");
        assert!(
            name.starts_with(STAGING_DIR_PREFIX),
            "暂存名应隐藏前缀：{name}"
        );
        assert!(
            name.contains(TEST_ENGINE_REVISION),
            "暂存名应携带 revision：{name}"
        );
        assert_eq!(
            staging.path().parent(),
            Some(store.root_dir()),
            "暂存必须在根目录下"
        );
        assert!(staging.path().is_dir(), "暂存应为目录");
    }
}

#[tokio::test]
async fn staging_directory_drop_and_adopt_guard_paths() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let root_dir = temp.path().join("systemone-models");
    let store = store_at(root_dir.clone());

    // 创建即武装：句柄 Drop → 尽力清理自己的暂存目录。
    let staging = store.create_staging_directory().await.expect("暂存");
    let staging_path = staging.path().to_path_buf();
    assert!(staging_path.is_dir(), "暂存应为目录");
    drop(staging);
    assert!(!staging_path.exists(), "guard Drop 应清理暂存目录");

    // adopt：准入通过才武装，接管后 Drop 同样清理。
    std::fs::create_dir_all(&root_dir).expect("root 创建");
    let adopted_path = root_dir.join(".tmp-adopted-external-1");
    std::fs::create_dir(&adopted_path).expect("手工暂存目录");
    let adopted = store
        .adopt_staging_directory(adopted_path.clone())
        .expect("接管自有暂存目录");
    assert_eq!(adopted.path(), adopted_path.as_path());
    drop(adopted);
    assert!(!adopted_path.exists(), "接管后 Drop 应清理暂存目录");

    // adopt 拒绝 root 外路径，且 NEVER 删除外部目录。
    let foreign = temp.path().join("foreign-dir");
    std::fs::create_dir_all(&foreign).expect("外部目录");
    let error = store
        .adopt_staging_directory(foreign.clone())
        .expect_err("root 外路径必须拒绝");
    assert!(
        matches!(error, ModelInstallError::StagingRejected { .. }),
        "应为暂存准入拒绝：{error}"
    );
    assert!(foreign.is_dir(), "被拒外部目录 MUST 保持存在");

    // adopt 拒绝缺隐藏前缀的 root 子目录，且不删除。
    let plain = root_dir.join("not-staging");
    std::fs::create_dir(&plain).expect("root 子目录");
    let error = store
        .adopt_staging_directory(plain.clone())
        .expect_err("缺隐藏前缀必须拒绝");
    assert!(
        matches!(error, ModelInstallError::StagingRejected { .. }),
        "应为暂存准入拒绝：{error}"
    );
    assert!(plain.is_dir(), "被拒非暂存子目录 MUST 保持存在");
}

#[tokio::test]
async fn install_publishes_final_revision_only_after_commit() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let root_dir = temp.path().join("systemone-models");
    let store = store_at(root_dir.clone());
    let revision_dir = root_dir.join(TEST_ENGINE_REVISION);

    let staging = store.create_staging_directory().await.expect("暂存目录");
    let staging_path = staging.path().to_path_buf();
    stage_complete_assets(&staging_path);

    // prepare：校验通过但 final 尚未出现。
    let prepared = store
        .prepare_staged_install(staging)
        .await
        .expect("暂存校验通过");
    assert!(
        matches!(prepared, PreparedStagedInstall::ReadyToCommit(_)),
        "应进入待提交态"
    );
    assert!(
        !revision_dir.exists(),
        "commit 之前 final revision 目录不得出现"
    );
    assert!(staging_path.join(MODEL_MANIFEST_FILE_NAME).exists());

    // commit：no-replace rename 之后 final 才出现且可解析为 Installed。
    let installed = store
        .commit_prepared_install(prepared)
        .await
        .expect("原子安装提交");
    assert_eq!(installed.install_root(), revision_dir.as_path());
    assert!(revision_dir.join(MODEL_MANIFEST_FILE_NAME).exists());
    assert!(!staging_path.exists(), "commit 后暂存目录应已 rename 消失");
    let state = store.installed_assets().await;
    assert!(
        matches!(state, ModelAssetState::Installed(_)),
        "安装后应解析为 Installed，实际：{state:?}"
    );
}

#[tokio::test]
async fn failed_install_leaves_no_final_and_no_staging() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let root_dir = temp.path().join("systemone-models");
    let store = store_at(root_dir.clone());
    let revision_dir = root_dir.join(TEST_ENGINE_REVISION);

    // 暂存目录缺 tokenizer 资产 → 校验失败。
    let staging = store.create_staging_directory().await.expect("暂存目录");
    let staging_path = staging.path().to_path_buf();
    write_payloads(&staging_path, &fixture_payloads()[..2]);
    write_manifest(&staging_path, &fixture_manifest());

    let error = store
        .install_staged_assets(staging)
        .await
        .expect_err("缺失资产的安装必须失败");
    assert!(
        matches!(error, ModelInstallError::VerificationFailed { .. }),
        "应为校验失败：{error}"
    );
    assert!(!revision_dir.exists(), "失败安装不得出现 final 目录");
    assert!(!staging_path.exists(), "失败安装必须清理暂存目录");
    assert!(
        staging_directories(&root_dir).is_empty(),
        "root 下不得残留任何暂存目录"
    );
}

#[tokio::test]
async fn prepare_rejects_symlinked_staging_manifest_without_following() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let root_dir = temp.path().join("systemone-models");
    let store = store_at(root_dir.clone());

    // 外部真实文件：若写入跟随了符号链接，它的内容会被覆盖。
    let outside_manifest = temp.path().join("outside-manifest.json");
    std::fs::write(&outside_manifest, b"OUTSIDE-MANIFEST-CONTENT").expect("外部文件写入");

    let staging = store.create_staging_directory().await.expect("暂存");
    let staging_path = staging.path().to_path_buf();
    write_payloads(&staging_path, &fixture_payloads());
    std::os::unix::fs::symlink(
        &outside_manifest,
        staging_path.join(MODEL_MANIFEST_FILE_NAME),
    )
    .expect("manifest 符号链接创建");

    let error = store
        .prepare_staged_install(staging)
        .await
        .expect_err("暂存内符号链接 manifest 必须被拒绝");
    assert!(
        matches!(error, ModelInstallError::StagingWriteFailed { .. }),
        "应为暂存写入失败：{error}"
    );
    let outside = std::fs::read(&outside_manifest).expect("外部文件读回");
    assert_eq!(
        outside,
        b"OUTSIDE-MANIFEST-CONTENT".to_vec(),
        "NEVER 跟随符号链接覆盖外部文件"
    );
    assert!(
        staging_directories(&root_dir).is_empty(),
        "失败路径必须清理自己的暂存目录"
    );
}

#[tokio::test]
async fn install_when_final_already_valid_is_idempotent_and_keeps_existing() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let root_dir = temp.path().join("systemone-models");
    let store = store_at(root_dir.clone());

    // 首次安装成功。
    let first_staging = store.create_staging_directory().await.expect("暂存");
    stage_complete_assets(first_staging.path());
    let installed = store
        .install_staged_assets(first_staging)
        .await
        .expect("首次安装成功");
    let revision_dir = root_dir.join(TEST_ENGINE_REVISION);
    assert_eq!(installed.install_root(), revision_dir.as_path());

    // 在 final 中放入哨兵文件：任何覆盖/重命名都会使其消失。
    let sentinel_path = revision_dir.join("sentinel.marker");
    std::fs::write(&sentinel_path, b"keep-me").expect("哨兵写入");

    // 第二次携带同样有效资产安装 → 幂等返回现有 Installed，不覆盖。
    let second_staging = store.create_staging_directory().await.expect("暂存");
    stage_complete_assets(second_staging.path());
    let again_installed = store
        .install_staged_assets(second_staging)
        .await
        .expect("幂等安装成功");
    assert_eq!(again_installed.install_root(), revision_dir.as_path());
    assert_eq!(again_installed.manifest(), installed.manifest());
    assert!(
        sentinel_path.exists(),
        "已存在的有效安装不得被覆盖或重命名替换"
    );
    assert!(
        staging_directories(&root_dir).is_empty(),
        "幂等路径必须清理自己的暂存目录"
    );
}

#[tokio::test]
async fn install_when_final_already_invalid_fails_without_touching_final() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let root_dir = temp.path().join("systemone-models");
    let store = store_at(root_dir.clone());
    let revision_dir = root_dir.join(TEST_ENGINE_REVISION);

    // final 已存在但已损坏（manifest 非法 + 标记文件）。
    std::fs::create_dir_all(&revision_dir).expect("final 目录创建");
    std::fs::write(revision_dir.join(MODEL_MANIFEST_FILE_NAME), "broken{{").expect("损坏 manifest");
    std::fs::write(revision_dir.join("marker.keep"), b"untouched").expect("标记文件");

    let staging = store.create_staging_directory().await.expect("暂存");
    let staging_path = staging.path().to_path_buf();
    stage_complete_assets(&staging_path);
    let error = store
        .install_staged_assets(staging)
        .await
        .expect_err("final 已无效时安装必须失败");
    assert!(
        matches!(error, ModelInstallError::FinalInvalid { .. }),
        "应为 final 无效错误：{error}"
    );
    assert!(
        revision_dir.join(MODEL_MANIFEST_FILE_NAME).exists(),
        "NEVER 删除已存在的 final 目录"
    );
    let final_manifest =
        std::fs::read_to_string(revision_dir.join(MODEL_MANIFEST_FILE_NAME)).expect("读回");
    assert_eq!(final_manifest, "broken{{", "NEVER 覆写已存在的 final 内容");
    assert!(
        revision_dir.join("marker.keep").exists(),
        "final 内容原样保留"
    );
    assert!(
        staging_directories(&root_dir).is_empty(),
        "失败路径必须清理暂存目录"
    );
}

#[tokio::test]
async fn rename_conflict_with_concurrent_final_fails_closed_and_keeps_existing() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let root_dir = temp.path().join("systemone-models");
    let store = store_at(root_dir.clone());
    let revision_dir = root_dir.join(TEST_ENGINE_REVISION);

    // prepare 阶段 final 不存在（校验通过，进入待提交态）。
    let staging = store.create_staging_directory().await.expect("暂存");
    let staging_path = staging.path().to_path_buf();
    stage_complete_assets(&staging_path);
    let prepared = store
        .prepare_staged_install(staging)
        .await
        .expect("暂存校验通过");
    assert!(
        matches!(prepared, PreparedStagedInstall::ReadyToCommit(_)),
        "final 尚未存在时应进入待提交态"
    );

    // 模拟并发竞争者在 rename 之前抢先落了非空 final 目录。
    std::fs::create_dir_all(&revision_dir).expect("并发 final 创建");
    std::fs::write(revision_dir.join("intruder.marker"), b"intruder").expect("并发内容");

    // commit → no-replace 冲突 → fail closed，绝不删除/覆盖对方内容。
    let error = store
        .commit_prepared_install(prepared)
        .await
        .expect_err("no-replace rename 冲突必须失败");
    assert!(
        matches!(error, ModelInstallError::RenameConflict { .. }),
        "应为 rename 冲突：{error}"
    );
    assert!(
        revision_dir.join("intruder.marker").exists(),
        "NEVER 删除并发出现的 final 内容"
    );
    assert!(
        staging_directories(&root_dir).is_empty(),
        "冲突路径必须清理自己的暂存目录"
    );
}

#[tokio::test]
async fn rename_conflict_with_concurrent_empty_final_fails_closed_and_keeps_empty_dir() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let root_dir = temp.path().join("systemone-models");
    let store = store_at(root_dir.clone());
    let revision_dir = root_dir.join(TEST_ENGINE_REVISION);

    // prepare 阶段 final 不存在。
    let staging = store.create_staging_directory().await.expect("暂存");
    let staging_path = staging.path().to_path_buf();
    stage_complete_assets(&staging_path);
    let prepared = store
        .prepare_staged_install(staging)
        .await
        .expect("暂存校验通过");
    assert!(
        matches!(prepared, PreparedStagedInstall::ReadyToCommit(_)),
        "final 尚未存在时应进入待提交态"
    );

    // 竞争者只抢先创建了**空** final 目录（std::fs::rename 会成功覆盖它）。
    std::fs::create_dir(&revision_dir).expect("并发空 final 目录创建");

    // commit → no-replace 一律不覆盖（空目录也不行）→ fail closed。
    let error = store
        .commit_prepared_install(prepared)
        .await
        .expect_err("空目录目标同样必须阻止提交");
    assert!(
        matches!(error, ModelInstallError::RenameConflict { .. }),
        "应为 rename 冲突：{error}"
    );
    assert!(
        revision_dir.is_dir(),
        "并发出现的空 final 目录 MUST 原样存在"
    );
    let remaining = std::fs::read_dir(&revision_dir)
        .expect("读 final 目录")
        .count();
    assert_eq!(remaining, 0, "空 final 目录内容 MUST 不变");
    assert!(
        staging_directories(&root_dir).is_empty(),
        "冲突路径必须清理自己的暂存目录"
    );
}

#[tokio::test]
async fn commit_when_competitor_lands_valid_final_is_idempotent_and_cleans_staging() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let root_dir = temp.path().join("systemone-models");
    let store = store_at(root_dir.clone());
    let manifest = fixture_manifest();
    let revision_dir = root_dir.join(TEST_ENGINE_REVISION);

    // prepare 阶段 final 不存在。
    let staging = store.create_staging_directory().await.expect("暂存");
    stage_complete_assets(staging.path());
    let prepared = store
        .prepare_staged_install(staging)
        .await
        .expect("暂存校验通过");
    assert!(
        matches!(prepared, PreparedStagedInstall::ReadyToCommit(_)),
        "final 尚未存在时应进入待提交态"
    );

    // 竞争者在 commit 之前落地了**完整有效**的 final 安装。
    write_installed_tree(&root_dir, &manifest);
    std::fs::write(revision_dir.join("competitor.marker"), b"mine").expect("竞争者标记");

    // commit → no-replace 冲突 → 探测为有效 → 幂等返回竞争者的安装。
    let installed = store
        .commit_prepared_install(prepared)
        .await
        .expect("竞争者已有效时幂等返回");
    assert_eq!(installed.install_root(), revision_dir.as_path());
    assert_eq!(installed.manifest(), &manifest);
    assert!(
        revision_dir.join("competitor.marker").exists(),
        "NEVER 删除竞争者的 final 内容"
    );
    assert!(
        staging_directories(&root_dir).is_empty(),
        "幂等路径必须清理自己的暂存目录"
    );
    let state = store.installed_assets().await;
    assert!(
        matches!(state, ModelAssetState::Installed(_)),
        "竞争者安装应解析为 Installed，实际：{state:?}"
    );
}

#[tokio::test]
async fn symlinked_install_root_rejects_staging_and_install_without_touching_target() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    // 外部真实目标：若安装入口沿链接创建暂存/安装，内容会出现在这里。
    let external_root = temp.path().join("external-root");
    std::fs::create_dir_all(&external_root).expect("外部 root 创建");
    let link_root = temp.path().join("link-root");
    std::os::unix::fs::symlink(&external_root, &link_root).expect("root 符号链接创建");
    let store = store_at(link_root.clone());

    // create staging：root 本体是符号链接 → typed 失败，安装入口因此不可达。
    let error = store
        .create_staging_directory()
        .await
        .expect_err("符号链接 root 必须拒绝创建暂存");
    assert!(
        matches!(error, ModelInstallError::RootInvalid { .. }),
        "应为安装根无效：{error}"
    );
    assert!(
        error.to_string().contains("符号链接"),
        "detail 应说明符号链接：{error}"
    );

    // adopt：同样 typed 失败；外部目标中的候选目录 NEVER 被接管或删除。
    let adopted_candidate = external_root.join(".tmp-adopted-external-1");
    std::fs::create_dir(&adopted_candidate).expect("外部候选暂存创建");
    let error = store
        .adopt_staging_directory(link_root.join(".tmp-adopted-external-1"))
        .expect_err("符号链接 root 必须拒绝接管暂存");
    assert!(
        matches!(error, ModelInstallError::RootInvalid { .. }),
        "应为安装根无效：{error}"
    );
    assert!(adopted_candidate.is_dir(), "被拒路径 MUST 保持存在");

    // 外部目标未被安装入口写：没有创建出任何暂存目录，也没有 revision 安装。
    assert_eq!(
        staging_directories(&external_root),
        vec![adopted_candidate.clone()],
        "安装入口 NEVER 沿符号链接创建暂存"
    );
    assert!(
        !external_root.join(TEST_ENGINE_REVISION).exists(),
        "外部目标不得出现安装目录"
    );

    // 读路径同一契约：fail-closed 拒绝该 root。
    let state = store.installed_assets().await;
    let ModelAssetState::Invalid { detail, .. } = state else {
        panic!("符号链接 root 应为 Invalid，实际：{state:?}");
    };
    assert!(
        detail.contains("符号链接"),
        "detail 应说明符号链接：{detail}"
    );
}

#[tokio::test]
async fn prepare_revalidates_root_after_swap_to_symlink_without_touching_target() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let root_dir = temp.path().join("systemone-models");
    let store = store_at(root_dir.clone());

    let staging = store.create_staging_directory().await.expect("暂存");
    let staging_path = staging.path().to_path_buf();
    stage_complete_assets(&staging_path);

    // TOCTOU：prepare 之前把真实 root 移走，用指向外部目录的符号链接占据原路径。
    let moved_root = temp.path().join("systemone-models.real");
    std::fs::rename(&root_dir, &moved_root).expect("移走真实 root");
    let external = temp.path().join("external-target");
    std::fs::create_dir_all(&external).expect("外部目录创建");
    std::os::unix::fs::symlink(&external, &root_dir).expect("符号链接占据原 root 路径");

    let error = store
        .prepare_staged_install(staging)
        .await
        .expect_err("root 被换成符号链接时 prepare 必须重新校验并拒绝");
    assert!(
        matches!(error, ModelInstallError::RootInvalid { .. }),
        "应为安装根无效：{error}"
    );
    assert!(
        error.to_string().contains("符号链接"),
        "detail 应说明符号链接：{error}"
    );
    // 外部目标从未被写（含 canonical manifest 写入）。
    assert_eq!(
        std::fs::read_dir(&external).expect("读外部目录").count(),
        0,
        "NEVER 沿符号链接写入外部目标"
    );
    assert!(
        !external.join(TEST_ENGINE_REVISION).exists(),
        "外部目标不得出现安装目录"
    );
}

#[tokio::test]
async fn regular_file_install_root_is_rejected_by_install_and_read() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let root_dir = temp.path().join("systemone-models");
    std::fs::write(&root_dir, b"not-a-directory").expect("普通文件 root 写入");
    let store = store_at(root_dir.clone());

    let error = store
        .create_staging_directory()
        .await
        .expect_err("普通文件 root 必须拒绝创建暂存");
    assert!(
        matches!(error, ModelInstallError::RootInvalid { .. }),
        "应为安装根无效：{error}"
    );
    assert!(
        error.to_string().contains("不是目录"),
        "detail 应说明不是目录：{error}"
    );
    // 普通文件 root 原样保留，NEVER 被覆盖成目录。
    assert_eq!(
        std::fs::read(&root_dir).expect("读回 root 文件"),
        b"not-a-directory".to_vec(),
        "普通文件 root 内容 MUST 不变"
    );

    // 读路径同一契约拒绝。
    let state = store.installed_assets().await;
    let ModelAssetState::Invalid { detail, .. } = state else {
        panic!("普通文件 root 应为 Invalid，实际：{state:?}");
    };
    assert!(
        detail.contains("不是目录"),
        "detail 应说明不是目录：{detail}"
    );
}

#[tokio::test]
async fn ready_to_commit_token_drop_cleans_staging_without_committing() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let root_dir = temp.path().join("systemone-models");
    let store = store_at(root_dir.clone());
    let revision_dir = root_dir.join(TEST_ENGINE_REVISION);

    let staging = store.create_staging_directory().await.expect("暂存");
    let staging_path = staging.path().to_path_buf();
    stage_complete_assets(&staging_path);
    let prepared = store
        .prepare_staged_install(staging)
        .await
        .expect("暂存校验通过");
    let PreparedStagedInstall::ReadyToCommit(_token) = &prepared else {
        panic!("应进入待提交态");
    };
    assert!(staging_path.is_dir(), "待提交令牌持有期间暂存目录必须在位");
    assert!(!revision_dir.exists(), "提交前 final 不得出现");

    // 未提交即丢弃令牌 → guard 的 Drop 同步 best-effort 清理自有暂存目录。
    drop(prepared);
    assert!(
        !staging_path.exists(),
        "ReadyToCommit 令牌丢弃必须清理暂存目录"
    );
    assert!(
        staging_directories(&root_dir).is_empty(),
        "root 下不得残留暂存目录"
    );
    assert!(!revision_dir.exists(), "丢弃令牌 NEVER 产生 final 安装");
}

#[tokio::test]
async fn commit_after_staging_removed_returns_commit_io_failed() {
    let temp = tempfile::TempDir::new().expect("唯一临时目录");
    let root_dir = temp.path().join("systemone-models");
    let store = store_at(root_dir.clone());
    let revision_dir = root_dir.join(TEST_ENGINE_REVISION);

    let staging = store.create_staging_directory().await.expect("暂存");
    let staging_path = staging.path().to_path_buf();
    stage_complete_assets(&staging_path);
    let prepared = store
        .prepare_staged_install(staging)
        .await
        .expect("暂存校验通过");
    assert!(
        matches!(prepared, PreparedStagedInstall::ReadyToCommit(_)),
        "final 尚未存在时应进入待提交态"
    );

    // prepare 之后暂存目录被外部删除：rename 源缺失 → 目标也不存在 → 非竞争 IO 失败。
    std::fs::remove_dir_all(&staging_path).expect("删除暂存目录");
    let error = store
        .commit_prepared_install(prepared)
        .await
        .expect_err("源不存在的提交必须失败");
    assert!(
        matches!(error, ModelInstallError::CommitIoFailed { .. }),
        "应为提交 IO 失败而非 rename 冲突：{error}"
    );
    assert!(
        error.to_string().contains("安装目标不存在"),
        "detail 应说明目标不存在：{error}"
    );
    assert!(!revision_dir.exists(), "失败提交不得出现 final 目录");
    assert!(
        staging_directories(&root_dir).is_empty(),
        "失败提交路径不得残留暂存目录"
    );
}

#[test]
fn rename_failure_classification_is_deterministic() {
    // 平台缺 no-replace 原语 → RenameConflict（fail closed，NEVER 回退覆盖性 rename）。
    let unsupported = std::io::Error::from(std::io::ErrorKind::Unsupported);
    let classified = install::classify_rename_failure(
        &unsupported,
        install::FinalTargetProbe::Unsupported,
        None,
    );
    assert!(
        matches!(classified, ModelInstallError::RenameConflict { .. }),
        "Unsupported 应为 RenameConflict：{classified}"
    );

    // 目标已存在且验证结果表明竞争者 → RenameConflict。
    let already_exists = std::io::Error::from(std::io::ErrorKind::AlreadyExists);
    let classified = install::classify_rename_failure(
        &already_exists,
        install::FinalTargetProbe::Present,
        Some("暂存缺少 manifest.json".to_owned()),
    );
    assert!(
        matches!(classified, ModelInstallError::RenameConflict { .. }),
        "竞争者应为 RenameConflict：{classified}"
    );
    assert!(
        classified.to_string().contains("竞争者"),
        "detail 应说明竞争者：{classified}"
    );

    // EACCES（无目标的权限失败）→ CommitIoFailed。
    let permission_denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
    let classified = install::classify_rename_failure(
        &permission_denied,
        install::FinalTargetProbe::Absent,
        None,
    );
    assert!(
        matches!(classified, ModelInstallError::CommitIoFailed { .. }),
        "EACCES 无目标应为 CommitIoFailed：{classified}"
    );

    // ENOENT（无目标）→ CommitIoFailed。
    let not_found = std::io::Error::from(std::io::ErrorKind::NotFound);
    let classified =
        install::classify_rename_failure(&not_found, install::FinalTargetProbe::Absent, None);
    assert!(
        matches!(classified, ModelInstallError::CommitIoFailed { .. }),
        "ENOENT 应为 CommitIoFailed：{classified}"
    );

    // 目标不可探测（无从判断竞争者）→ CommitIoFailed。
    let classified = install::classify_rename_failure(
        &permission_denied,
        install::FinalTargetProbe::Unprobeable("权限不足".to_owned()),
        None,
    );
    assert!(
        matches!(classified, ModelInstallError::CommitIoFailed { .. }),
        "目标不可探测应为 CommitIoFailed：{classified}"
    );
    assert!(
        classified.to_string().contains("不可探测"),
        "detail 应说明目标不可探测：{classified}"
    );
}
