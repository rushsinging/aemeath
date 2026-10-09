//! `aemeath systemone download` 组合根装配测试：真实生产装配链
//! （`wire_model_download_service` + `LocalModelAssetStore`），不注入 fake；
//! 覆盖 manifest 缺失 fail-closed、无效 manifest 契约失败与已安装幂等三条路径，
//! 全部零网络。

use std::path::Path;

use composition::systemone::{run_systemone_download_with, SystemoneDownloadExit};

/// 测试用 engine_revision（canonical segment）。
const TEST_ENGINE_REVISION: &str = "e83f5c1a9d2b4f6a7c0e1d2b3a4f5c6d7e8f9a0b";

/// 三类小体积夹具资产（与 systemone model_assets 测试同布局）。
fn fixture_payloads() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("model.gguf", b"GGUF-TEST-WEIGHTS-CONTENT".to_vec()),
        ("pointer_head.safetensors", vec![7_u8; 64]),
        ("tokenizer/merges.txt", b"line-one\nline-two\n".to_vec()),
    ]
}

/// 按夹具字节构造契约 manifest（长度与 SHA-256 来自真实字节）。
fn fixture_manifest() -> systemone::ModelManifest {
    let assets = fixture_payloads()
        .into_iter()
        .map(|(path, payload)| systemone::ModelAsset {
            url: format!("https://example.com/{path}"),
            byte_length: payload.len() as u64,
            sha256: utils::sha256_hex(&payload),
            path: path.to_owned(),
        })
        .collect();
    systemone::ModelManifest {
        schema_version: 1,
        engine_revision: TEST_ENGINE_REVISION.to_owned(),
        hidden_size: 1024,
        pointer_dimension: 256,
        temperature: 0.07,
        supported_platforms: vec![systemone::required_platform().to_owned()],
        assets,
    }
}

/// 在 models_dir 下手工构造完整有效安装树（覆盖读路径幂等命中）。
fn write_installed_tree(models_dir: &Path, manifest: &systemone::ModelManifest) {
    let revision_dir = models_dir.join(&manifest.engine_revision);
    for (path, payload) in fixture_payloads() {
        let file_path = revision_dir.join(path);
        std::fs::create_dir_all(file_path.parent().expect("资产应有父目录")).expect("父目录创建");
        std::fs::write(file_path, payload).expect("资产写入");
    }
    std::fs::write(
        revision_dir.join("manifest.json"),
        serde_json::to_string_pretty(manifest).expect("manifest 序列化"),
    )
    .expect("manifest 写入");
}

/// manifest 缺失（发行元数据未落地）→ typed 失败、退出码 1、fail-closed
/// 不构造下载链、不触碰网络与本地模型目录。
#[tokio::test(flavor = "current_thread")]
async fn download_without_release_manifest_fails_closed() {
    let temp = tempfile::tempdir().expect("唯一临时目录");
    let exit = run_systemone_download_with(
        None,
        temp.path().join("models").join("systemone"),
        "aemeath-test/1.0",
    )
    .await;
    let SystemoneDownloadExit::Failure { message, exit_code } = exit else {
        panic!("manifest 缺失应 fail-closed 失败，实际: {exit:?}");
    };
    assert_eq!(exit_code, 1);
    assert!(
        message.contains("manifest"),
        "失败消息应说明 manifest 未提供：{message}"
    );
    // fail-closed：不得创建任何目录（未触碰模型目录）。
    assert!(
        !temp.path().join("models").exists(),
        "manifest 缺失时不得创建模型目录"
    );
}

/// manifest 契约非法（资产 sha 长度错）→ 构造期拒绝、零 IO。
#[tokio::test(flavor = "current_thread")]
async fn download_with_invalid_manifest_contract_is_rejected() {
    let temp = tempfile::tempdir().expect("唯一临时目录");
    let mut manifest = fixture_manifest();
    manifest.assets[0].sha256 = "deadbeef".to_owned();
    let exit = run_systemone_download_with(
        Some(manifest),
        temp.path().join("models").join("systemone"),
        "aemeath-test/1.0",
    )
    .await;
    let SystemoneDownloadExit::Failure { message, exit_code } = exit else {
        panic!("manifest 非法应失败，实际: {exit:?}");
    };
    assert_eq!(exit_code, 1);
    assert!(
        message.contains("manifest"),
        "失败消息应指向 manifest 契约：{message}"
    );
}

/// 本地已有有效安装 → 幂等成功（already_installed，零网络），真实生产
/// 装配链（store 读路径 + sha 校验）全程参与。
#[tokio::test(flavor = "current_thread")]
async fn download_with_installed_model_is_idempotent() {
    let temp = tempfile::tempdir().expect("唯一临时目录");
    let models_dir = temp.path().join("models").join("systemone");
    let manifest = fixture_manifest();
    write_installed_tree(&models_dir, &manifest);

    let exit = run_systemone_download_with(Some(manifest), models_dir, "aemeath-test/1.0").await;
    let SystemoneDownloadExit::Success(report) = exit else {
        panic!("已安装应幂等成功，实际: {exit:?}");
    };
    assert!(report.already_installed);
    assert_eq!(report.revision, TEST_ENGINE_REVISION);
    assert!(report.install_root.join("model.gguf").exists());
    assert_eq!(report.exit_code(), 0);
}
