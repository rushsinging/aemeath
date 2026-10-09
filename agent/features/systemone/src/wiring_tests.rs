//! `wire_embedded_scoring` 装配契约：typed 错误映射、fail-closed 零 IO 与审计外壳 revision。

use super::*;
use crate::domain::{CalibrationLevel, ScoringAnswer, ScoringQuestion, ScoringState};
use serde_json::json;

/// 测试 fixture manifest（URL / SHA-256 仅锁契约格式，**不**对应任何真实发行数据）。
fn fixture_manifest() -> ModelManifest {
    ModelManifest::parse(
        &json!({
            "schema_version": 1,
            "engine_revision": "wiring-fixture-r1",
            "hidden_size": 1024,
            "pointer_dimension": 256,
            "temperature": 0.07,
            "supported_platforms": ["macos-aarch64"],
            "assets": [
                {
                    "path": "model.gguf",
                    "url": "https://models.example.com/systemone/model.gguf",
                    "byte_length": 775000000u64,
                    "sha256": "0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0"
                },
                {
                    "path": "pointer_head.safetensors",
                    "url": "https://models.example.com/systemone/pointer_head.safetensors",
                    "byte_length": 8403456u64,
                    "sha256": "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90"
                },
                {
                    "path": "tokenizer/tokenizer.json",
                    "url": "https://models.example.com/systemone/tokenizer/tokenizer.json",
                    "byte_length": 263456u64,
                    "sha256": "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
                }
            ]
        })
        .to_string(),
    )
    .expect("fixture manifest 应满足全部契约")
}

/// manifest 契约非法：零 IO fail-closed（不创建目录、不读模型），
/// 任何 feature 组合下都先于装配拒绝。
#[tokio::test]
async fn wire_embedded_scoring_with_invalid_manifest_returns_manifest_invalid_before_io() {
    let mut manifest = fixture_manifest();
    manifest.hidden_size = 7;
    let models_dir = std::path::PathBuf::from("/nonexistent/aemeath-wiring-models");
    let scoring_dir = std::path::PathBuf::from("/nonexistent/aemeath-wiring-scoring");
    let error = match wire_embedded_scoring(models_dir.clone(), scoring_dir.clone(), manifest).await
    {
        Err(error) => error,
        Ok(_) => panic!("契约非法的 manifest 必须 fail-closed"),
    };
    assert!(
        matches!(error, EmbeddedScoringWiringError::ManifestInvalid { .. }),
        "错误类型不符：{error:?}"
    );
    assert!(
        error.to_string().contains("hidden size"),
        "中文 detail 应包含具体原因：{error}"
    );
    assert!(
        !models_dir.exists() && !scoring_dir.exists(),
        "零 IO：契约非法时不得创建或读取任何目录"
    );
}

/// feature `embedded` 关闭（默认构建）：typed `EmbeddedUnavailable`，
/// 绝不解析资产状态、绝不启动 worker、绝不回退 HTTP。
#[cfg(not(feature = "embedded"))]
#[tokio::test]
async fn wire_embedded_scoring_without_embedded_feature_returns_typed_unavailable() {
    let models_dir = std::path::PathBuf::from("/nonexistent/aemeath-wiring-models");
    let scoring_dir = std::path::PathBuf::from("/nonexistent/aemeath-wiring-scoring");
    let error =
        match wire_embedded_scoring(models_dir.clone(), scoring_dir, fixture_manifest()).await {
            Err(error) => error,
            Ok(_) => panic!("feature 关闭必须返回 typed 错误"),
        };
    assert!(
        matches!(error, EmbeddedScoringWiringError::EmbeddedUnavailable),
        "错误类型不符：{error:?}"
    );
    assert!(
        error.to_string().contains("embedded"),
        "中文消息应说明 embedded 缺失：{error}"
    );
    assert!(
        !models_dir.exists(),
        "零 IO：feature 关闭时不得读取或创建模型目录"
    );
}

/// feature `embedded` 开启 + 模型未安装：typed `ModelMissing`（fail-closed，
/// 在启动 worker 之前返回，测试绝不下载、绝不加载真实模型）。
#[cfg(feature = "embedded")]
#[tokio::test]
async fn wire_embedded_scoring_when_model_missing_returns_model_missing() {
    let temp = tempfile::TempDir::new().expect("临时目录");
    let error = match wire_embedded_scoring(
        temp.path().join("models"),
        temp.path().join("scoring"),
        fixture_manifest(),
    )
    .await
    {
        Err(error) => error,
        Ok(_) => panic!("资产缺失必须 fail-closed，NEVER 构造 port"),
    };
    assert!(
        matches!(error, EmbeddedScoringWiringError::ModelMissing),
        "错误类型不符：{error:?}"
    );
    assert!(
        error.to_string().contains("aemeath systemone download"),
        "消息应提示手动下载命令：{error}"
    );
}

/// `EmbeddedInitError` → wiring 错误的 typed 映射：分类不丢失、消息保持中文。
#[cfg(feature = "embedded")]
#[test]
fn embedded_init_error_maps_to_typed_wiring_error() {
    use crate::adapters::embedded::EmbeddedInitError;
    use crate::adapters::llama_worker::WorkerInitError;
    use crate::ports::InvalidAssetKind;

    assert_eq!(
        EmbeddedScoringWiringError::from(EmbeddedInitError::ModelMissing),
        EmbeddedScoringWiringError::ModelMissing
    );
    assert_eq!(
        EmbeddedScoringWiringError::from(EmbeddedInitError::InvalidAssets {
            kind: InvalidAssetKind::Unsupported,
            detail: "schema 版本不符".to_owned(),
        }),
        EmbeddedScoringWiringError::InvalidAssets {
            kind: InvalidAssetKind::Unsupported,
            detail: "schema 版本不符".to_owned(),
        }
    );
    assert!(matches!(
        EmbeddedScoringWiringError::from(EmbeddedInitError::AssetContract {
            detail: "model.gguf 数量不是 1".to_owned(),
        }),
        EmbeddedScoringWiringError::AssetContract { .. }
    ));
    assert!(matches!(
        EmbeddedScoringWiringError::from(EmbeddedInitError::TokenizerLoadFailed {
            detail: "词表损坏".to_owned(),
        }),
        EmbeddedScoringWiringError::TokenizerLoadFailed { .. }
    ));
    assert!(matches!(
        EmbeddedScoringWiringError::from(EmbeddedInitError::PointerHeadLoadFailed {
            detail: "维度不符".to_owned(),
        }),
        EmbeddedScoringWiringError::PointerHeadLoadFailed { .. }
    ));
    let worker_error = EmbeddedScoringWiringError::from(EmbeddedInitError::WorkerStart {
        source: WorkerInitError::UnsupportedPlatform {
            platform: "x86_64-apple-darwin".to_owned(),
        },
    });
    assert!(matches!(
        worker_error,
        EmbeddedScoringWiringError::WorkerStart { .. }
    ));
    assert!(
        worker_error.to_string().contains("不支持"),
        "worker 错误消息保持中文：{worker_error}"
    );
}

/// 校准 + 审计外壳：fake 内层端口验证 wrapper 链生效，且审计事件的
/// `engine_revision` MUST 来自 manifest（不是模型文件名或环境推断）。
#[tokio::test]
async fn wrap_calibrated_audited_appends_audit_event_with_manifest_revision() {
    struct StubScoringPort;

    #[async_trait::async_trait]
    impl ScoringPort for StubScoringPort {
        async fn answer(
            &self,
            _state: &ScoringState,
            _questions: &[ScoringQuestion],
        ) -> Result<Vec<ScoringAnswer>, crate::domain::ScoringUnavailable> {
            Ok(vec![ScoringAnswer::choice(
                "a",
                vec![("a".to_owned(), 0.9), ("b".to_owned(), 0.1)],
                0.9,
                CalibrationLevel::Raw,
            )
            .expect("答案构造")])
        }
    }

    let temp = tempfile::TempDir::new().expect("临时目录");
    let scoring_dir = temp.path().join("scoring");
    std::fs::create_dir_all(&scoring_dir).expect("scoring 目录创建");
    let manifest = fixture_manifest();
    let port = wrap_calibrated_audited(
        Arc::new(StubScoringPort),
        &manifest,
        &scoring_dir,
        "memory_rerank",
    );

    let state = ScoringState::new("用户正在验证审计 revision。").expect("state 构造");
    let questions = vec![ScoringQuestion::choice(
        "哪个候选更相关？",
        vec![
            ("a".to_owned(), "候选甲。".to_owned()),
            ("b".to_owned(), "候选乙。".to_owned()),
        ],
    )
    .expect("question 构造")];
    let answers = port
        .answer(&state, &questions)
        .await
        .expect("外壳链应透传 fake 端口结果");
    assert_eq!(answers.len(), 1);

    let audit_path = scoring_dir.join(crate::constants::AUDIT_FILE);
    assert!(audit_path.is_file(), "审计事件应落在 scoring 目录");
    let events: Vec<serde_json::Value> = std::fs::read_to_string(&audit_path)
        .expect("审计文件可读")
        .lines()
        .map(|line| serde_json::from_str(line).expect("行应为合法 JSON"))
        .collect();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0]["engine_revision"].as_str(),
        Some(manifest.engine_revision.as_str()),
        "审计 revision MUST 来自 manifest.engine_revision"
    );
}
