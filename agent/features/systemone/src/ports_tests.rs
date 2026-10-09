//! 模型资产端口数据：`InstalledAssets` 的校验构造与只读访问，以及 `Invalid` 的 typed 原因分类。

use super::{InstalledAssets, InvalidAssetKind, ModelAssetState};
use crate::domain::{required_platform, ModelAsset, ModelManifest, ModelManifestError};

/// 构造一份完整合法的 manifest（生产布局三类资产齐备，字段均为契约值）。
fn manifest_stub() -> ModelManifest {
    let sha256 = "0123456789abcdef".repeat(4);
    ModelManifest {
        schema_version: 1,
        engine_revision: "e83f5c1a9d2b4f6a7c0e1d2b3a4f5c6d7e8f9a0b".to_owned(),
        hidden_size: 1024,
        pointer_dimension: 256,
        temperature: 0.07,
        supported_platforms: vec![required_platform().to_owned()],
        assets: vec![
            ModelAsset {
                path: "model.gguf".to_owned(),
                url: "https://example.com/model.gguf".to_owned(),
                byte_length: 4096,
                sha256: sha256.clone(),
            },
            ModelAsset {
                path: "pointer_head.safetensors".to_owned(),
                url: "https://example.com/pointer_head.safetensors".to_owned(),
                byte_length: 2048,
                sha256: sha256.clone(),
            },
            ModelAsset {
                path: "tokenizer/merges.txt".to_owned(),
                url: "https://example.com/tokenizer/merges.txt".to_owned(),
                byte_length: 1024,
                sha256,
            },
        ],
    }
}

#[test]
fn installed_assets_accepts_valid_manifest_and_exposes_getters() {
    let install_root = std::path::PathBuf::from("/tmp/systemone/e83f5c1a");
    let assets = InstalledAssets::new(manifest_stub(), install_root.clone())
        .expect("完整合法 manifest 应通过校验");
    assert_eq!(
        assets.manifest().engine_revision,
        manifest_stub().engine_revision
    );
    assert_eq!(assets.manifest().hidden_size, 1024);
    assert_eq!(assets.install_root(), install_root.as_path());
}

#[test]
fn installed_assets_rejects_manifest_with_empty_assets_with_typed_error() {
    let mut manifest = manifest_stub();
    manifest.assets.clear();
    let error = InstalledAssets::new(
        manifest,
        std::path::PathBuf::from("/tmp/systemone/e83f5c1a"),
    )
    .expect_err("空资产 manifest 必须被拒绝");
    assert_eq!(error, ModelManifestError::ModelGgufCount { found: 0 });
}

#[test]
fn installed_assets_rejects_invalid_manifest_with_typed_error() {
    let mut manifest = manifest_stub();
    manifest.hidden_size = 512;
    let error = InstalledAssets::new(
        manifest,
        std::path::PathBuf::from("/tmp/systemone/e83f5c1a"),
    )
    .expect_err("非契约维度的 manifest 必须被拒绝");
    assert_eq!(
        error,
        ModelManifestError::UnsupportedHiddenSize {
            found: 512,
            expected: 1024,
        }
    );
}

#[test]
fn invalid_asset_kind_display_messages_contain_chinese_text() {
    for kind in [
        InvalidAssetKind::Corrupt,
        InvalidAssetKind::Unreadable,
        InvalidAssetKind::Unsupported,
    ] {
        let message = kind.to_string();
        assert!(
            message
                .chars()
                .any(|character| ('\u{4e00}'..='\u{9fff}').contains(&character)),
            "原因类别消息应包含中文：{message}"
        );
    }
}

#[test]
fn invalid_state_carries_typed_kind_and_detail() {
    let state = ModelAssetState::Invalid {
        kind: InvalidAssetKind::Corrupt,
        detail: "model.gguf 的 SHA-256 与 manifest 不符".to_owned(),
    };
    match state {
        ModelAssetState::Invalid { kind, detail } => {
            assert_eq!(kind, InvalidAssetKind::Corrupt);
            assert!(
                detail.contains("SHA-256"),
                "detail 应携带中文原因：{detail}"
            );
        }
        other => panic!("应为 Invalid 状态：{other:?}"),
    }
}
