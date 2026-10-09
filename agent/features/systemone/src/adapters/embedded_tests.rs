//! EmbeddedScoringAdapter：初始化 fail-closed、published language → worker → PointerHead 答案映射。

use crate::adapters::embedded::{EmbeddedInitError, EmbeddedScoringAdapter};
use crate::adapters::kev_causal_row::{
    CausalRowBuildError, KevCausalRowBuilder, KevDelimiter, KevRowTokenizer,
};
use crate::adapters::llama_worker::{
    spawn_worker_thread, CausalRow, EmbeddedWorkerClient, RowEmbeddingEngine, RowHiddenVectors,
    WorkerFailure, WorkerInitError,
};
use crate::domain::{
    CalibrationLevel, NoulCriteria, PointerHead, PointerHeadWeights, ScoringAnswer,
    ScoringQuestion, ScoringState, UnavailableKind,
};
use crate::ports::{InvalidAssetKind, ModelAssetState, ScoringPort};

/// 确定性 fake tokenizer：文本 → 逐字节 id，分隔 token → 负数哨兵。
struct FakeByteTokenizer;

impl KevRowTokenizer for FakeByteTokenizer {
    fn tokenize_user_text(&self, text: &str) -> Result<Vec<i32>, CausalRowBuildError> {
        Ok(text.bytes().map(i32::from).collect())
    }

    fn delimiter_id(&self, delimiter: KevDelimiter) -> Result<i32, CausalRowBuildError> {
        Ok(match delimiter {
            KevDelimiter::StateStart => -10,
            KevDelimiter::QuestionStart => -11,
            KevDelimiter::OptionStart => -12,
            KevDelimiter::OptionEnd => -13,
            KevDelimiter::Decide => -14,
        })
    }
}

/// 固定 hidden（[1,0] decide、交替 e0/e1 选项）的 fake engine。
struct FixedHiddenEngine;

impl RowEmbeddingEngine for FixedHiddenEngine {
    fn embed_rows(&mut self, rows: Vec<CausalRow>) -> Result<Vec<RowHiddenVectors>, WorkerFailure> {
        Ok(rows
            .iter()
            .map(|row| {
                let options = (0..row.option_offsets.len())
                    .map(|option_index| {
                        if option_index % 2 == 0 {
                            vec![1.0, 0.0]
                        } else {
                            vec![0.0, 1.0]
                        }
                    })
                    .collect();
                RowHiddenVectors::new(vec![1.0, 0.0], options)
            })
            .collect())
    }
}

/// 固定失败的 fake engine。
struct RejectingEngine {
    detail: String,
}

impl RowEmbeddingEngine for RejectingEngine {
    fn embed_rows(
        &mut self,
        _rows: Vec<CausalRow>,
    ) -> Result<Vec<RowHiddenVectors>, WorkerFailure> {
        Err(WorkerFailure::new(self.detail.clone()))
    }
}

/// 请求到达后崩溃的 fake engine。
struct PanickingEngine;

impl RowEmbeddingEngine for PanickingEngine {
    fn embed_rows(
        &mut self,
        _rows: Vec<CausalRow>,
    ) -> Result<Vec<RowHiddenVectors>, WorkerFailure> {
        panic!("模拟 worker 崩溃");
    }
}

/// 少回一行的 fake engine：行数与题数契约必须由 facade 校验。
struct MissingRowEngine;

impl RowEmbeddingEngine for MissingRowEngine {
    fn embed_rows(&mut self, rows: Vec<CausalRow>) -> Result<Vec<RowHiddenVectors>, WorkerFailure> {
        let mut vectors: Vec<RowHiddenVectors> = rows
            .iter()
            .map(|_| RowHiddenVectors::new(vec![1.0, 0.0], vec![vec![1.0, 0.0]]))
            .collect();
        vectors.pop();
        Ok(vectors)
    }
}

/// hidden 宽度错误的 fake engine。
struct WrongWidthEngine;

impl RowEmbeddingEngine for WrongWidthEngine {
    fn embed_rows(&mut self, rows: Vec<CausalRow>) -> Result<Vec<RowHiddenVectors>, WorkerFailure> {
        Ok(rows
            .iter()
            .map(|_| RowHiddenVectors::new(vec![1.0, 0.0, 0.0], vec![vec![1.0, 0.0, 0.0]]))
            .collect())
    }
}

/// 2 维恒等 PointerHead（与 pointer_head_loader 测试同一数学口径）。
fn tiny_pointer_head() -> PointerHead {
    PointerHead::new(
        PointerHeadWeights::new(
            2,
            2,
            1.0,
            &[1.0, 0.0, 0.0, 1.0],
            &[0.0, 0.0],
            &[1.0, 0.0, 0.0, 1.0],
            &[0.0, 0.0],
        )
        .expect("测试权重合法"),
    )
}

async fn client_for<E, F>(init_engine: F) -> EmbeddedWorkerClient
where
    E: RowEmbeddingEngine + 'static,
    F: FnOnce() -> Result<E, WorkerInitError> + Send + 'static,
{
    let init_receiver = spawn_worker_thread(init_engine);
    init_receiver
        .await
        .expect("worker 初始化通道不被丢弃")
        .expect("fake engine 初始化成功")
}

async fn adapter_with<E, F>(
    init_engine: F,
    row_builder: KevCausalRowBuilder,
) -> EmbeddedScoringAdapter
where
    E: RowEmbeddingEngine + 'static,
    F: FnOnce() -> Result<E, WorkerInitError> + Send + 'static,
{
    let client = client_for(init_engine).await;
    EmbeddedScoringAdapter::from_parts(client, row_builder, tiny_pointer_head(), 2)
}

fn sample_state(text: &str) -> ScoringState {
    ScoringState::new(text).expect("状态非空白")
}

fn sample_questions() -> Vec<ScoringQuestion> {
    let criteria =
        NoulCriteria::new("All requirements are met.", "A requirement is unmet.").unwrap();
    vec![
        ScoringQuestion::noul("Is the task fully complete?", Some(criteria)).unwrap(),
        ScoringQuestion::choice(
            "Which memory is most relevant?",
            vec![
                ("0".to_owned(), "First memory.".to_owned()),
                ("1".to_owned(), "Second memory.".to_owned()),
            ],
        )
        .unwrap(),
        ScoringQuestion::score(
            "How risky is the operation?",
            vec!["Reversible.".to_owned(), "Destructive.".to_owned()],
        )
        .unwrap(),
    ]
}

#[tokio::test]
async fn answer_maps_worker_vectors_to_pointer_head_answers_in_question_order() {
    let adapter = adapter_with(
        || Ok(FixedHiddenEngine),
        KevCausalRowBuilder::new(FakeByteTokenizer),
    )
    .await;
    let questions = sample_questions();
    let answers = adapter
        .answer(&sample_state("the session state"), &questions)
        .await
        .expect("评分成功");
    assert_eq!(answers.len(), 3, "答案与题目一一对应且保序");

    // 独立复算期望概率：decide=[1,0]、option_j 交替 e0/e1 → PointerHead 数学由 domain 测试锁定。
    let head = tiny_pointer_head();
    let noul_expected = head
        .score_options(&[1.0, 0.0], &[1.0, 0.0, 0.0, 1.0])
        .expect("复算成功");
    match &answers[0] {
        ScoringAnswer::Noul {
            p_true,
            calibration,
        } => {
            assert!(
                (p_true - f64::from(noul_expected[1])).abs() < 1e-6,
                "Noul p_true = probs[1]，实际 {p_true}"
            );
            assert_eq!(*calibration, CalibrationLevel::Temperature);
        }
        other => panic!("题型不符：{other:?}"),
    }

    let choice_expected = head
        .score_options(&[1.0, 0.0], &[1.0, 0.0, 0.0, 1.0])
        .expect("复算成功");
    match &answers[1] {
        ScoringAnswer::Choice {
            choice,
            probabilities,
            confidence,
            calibration,
        } => {
            assert_eq!(choice, "0", "首选项概率更高 → argmax 为 key 0");
            assert_eq!(
                probabilities
                    .iter()
                    .map(|(key, _)| key.clone())
                    .collect::<Vec<_>>(),
                vec!["0".to_owned(), "1".to_owned()],
                "probabilities 保 criteria 顺序"
            );
            assert!((probabilities[0].1 - f64::from(choice_expected[0])).abs() < 1e-6);
            assert!((probabilities[1].1 - f64::from(choice_expected[1])).abs() < 1e-6);
            assert!((confidence - (f64::from(choice_expected[0]) - 0.5) / 0.5).abs() < 1e-6);
            assert_eq!(*calibration, CalibrationLevel::Temperature);
        }
        other => panic!("题型不符：{other:?}"),
    }

    let score_expected = head
        .score_options(&[1.0, 0.0], &[1.0, 0.0, 0.0, 1.0])
        .expect("复算成功");
    match &answers[2] {
        ScoringAnswer::Score {
            score,
            probabilities,
            calibration,
            ..
        } => {
            let expected_score = f64::from(score_expected[1]);
            assert!(
                (score - expected_score).abs() < 1e-6,
                "score = Σ i·p_i，实际 {score}"
            );
            assert_eq!(probabilities.len(), 2, "两档概率与等级保序");
            assert_eq!(*calibration, CalibrationLevel::Temperature);
        }
        other => panic!("题型不符：{other:?}"),
    }
}

#[tokio::test]
async fn answer_when_worker_rejects_request_returns_server_unavailable() {
    let adapter = adapter_with(
        || {
            Ok(RejectingEngine {
                detail: "causal row 解码失败：测试".to_owned(),
            })
        },
        KevCausalRowBuilder::new(FakeByteTokenizer),
    )
    .await;
    let error = adapter
        .answer(&sample_state("state"), &sample_questions())
        .await
        .expect_err("worker 失败必须降级");
    assert_eq!(error.kind(), UnavailableKind::Server, "{error}");
    assert!(error.to_string().contains("解码失败"), "{error}");
}

#[tokio::test]
async fn answer_when_worker_thread_dies_returns_connect_unavailable() {
    let adapter = adapter_with(
        || Ok(PanickingEngine),
        KevCausalRowBuilder::new(FakeByteTokenizer),
    )
    .await;
    let error = adapter
        .answer(&sample_state("state"), &sample_questions())
        .await
        .expect_err("worker 崩溃必须降级");
    assert_eq!(error.kind(), UnavailableKind::Connect, "{error}");
}

#[tokio::test]
async fn answer_with_no_questions_returns_empty_without_worker_roundtrip() {
    let adapter = adapter_with(
        || Ok(PanickingEngine),
        KevCausalRowBuilder::new(FakeByteTokenizer),
    )
    .await;
    let answers = adapter
        .answer(&sample_state("state"), &[])
        .await
        .expect("空题目直接返回空答案");
    assert!(
        answers.is_empty(),
        "engine 一旦被触达即崩溃，空题目必须绕过 worker"
    );
}

#[tokio::test]
async fn answer_when_row_build_fails_returns_schema_unavailable() {
    // 分支限额压到 16：任何题目分支都超出 row 限额（kev serve 的 422 语义）。
    let builder = KevCausalRowBuilder::with_limits(FakeByteTokenizer, 64, 16);
    let adapter = adapter_with(|| Ok(FixedHiddenEngine), builder).await;
    let error = adapter
        .answer(&sample_state("state"), &sample_questions())
        .await
        .expect_err("row 构建失败必须降级");
    assert_eq!(error.kind(), UnavailableKind::Schema, "{error}");
    assert!(error.to_string().contains("超出"), "{error}");
}

#[tokio::test]
async fn answer_when_worker_returns_wrong_row_count_returns_server_unavailable() {
    let adapter = adapter_with(
        || Ok(MissingRowEngine),
        KevCausalRowBuilder::new(FakeByteTokenizer),
    )
    .await;
    let error = adapter
        .answer(&sample_state("state"), &sample_questions())
        .await
        .expect_err("行数不符必须降级");
    assert_eq!(error.kind(), UnavailableKind::Server, "{error}");
    assert!(
        error.to_string().contains("hidden 数量与题目数不符"),
        "命中 facade 行数契约检查本身（而非下游间接失败）：{error}"
    );
}

#[tokio::test]
async fn answer_when_hidden_width_mismatch_returns_server_unavailable() {
    let adapter = adapter_with(
        || Ok(WrongWidthEngine),
        KevCausalRowBuilder::new(FakeByteTokenizer),
    )
    .await;
    let error = adapter
        .answer(&sample_state("state"), &sample_questions())
        .await
        .expect_err("hidden 宽度不符必须降级");
    assert_eq!(error.kind(), UnavailableKind::Server, "{error}");
    assert!(error.to_string().contains("维度"), "{error}");
}

#[tokio::test]
async fn start_when_model_assets_missing_reports_model_missing_without_constructing_port() {
    let error = EmbeddedScoringAdapter::start(&ModelAssetState::Missing)
        .await
        .expect_err("模型缺失必须 fail-closed，不构造 port");
    assert!(
        matches!(error, EmbeddedInitError::ModelMissing),
        "错误类型不符：{error:?}"
    );
    assert!(
        error.to_string().contains("未安装"),
        "错误消息为中文：{error}"
    );
}

#[tokio::test]
async fn start_when_model_assets_invalid_reports_invalid_without_constructing_port() {
    let assets = ModelAssetState::Invalid {
        kind: InvalidAssetKind::Corrupt,
        detail: "pointer_head.safetensors 的 sha256 与 manifest 不符".to_owned(),
    };
    let error = EmbeddedScoringAdapter::start(&assets)
        .await
        .expect_err("校验失败资产必须 fail-closed，不构造 port");
    assert!(
        matches!(error, EmbeddedInitError::InvalidAssets { .. }),
        "错误类型不符：{error:?}"
    );
    assert!(
        error.to_string().contains("sha256 与 manifest 不符"),
        "明细透传资产校验原因：{error}"
    );
}
