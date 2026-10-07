//! 原生 llama.cpp worker：目标平台（macOS arm64）真实加载与数值稳定性验证。
//!
//! 真实模型测试标记 `#[ignore]`：缺资产时明确 skip 并提示执行
//! `aemeath systemone download`，NEVER 在测试内触发下载。

use std::path::PathBuf;

use crate::adapters::embedded::EmbeddedScoringAdapter;
use crate::adapters::kev_causal_row::KevCausalRowBuilder;
use crate::adapters::kev_hf_tokenizer::HfKevTokenizer;
use crate::adapters::llama_native_engine::{init_llama_row_engine, LlamaRowEngine};
use crate::adapters::llama_worker::{start_llama_worker, LlamaWorkerConfig, RowEmbeddingEngine};
use crate::domain::{ScoringQuestion, ScoringState};
use crate::ports::ScoringPort;

/// 本机已安装模型的位置（缺资产时由调用方 skip，测试 NEVER 下载）。
struct InstalledModelPaths {
    model_path: PathBuf,
    tokenizer_path: PathBuf,
    pointer_head_path: PathBuf,
    hidden_size: usize,
    pointer_dimension: usize,
    temperature: f32,
}

fn installed_model_paths() -> Option<InstalledModelPaths> {
    let models_dir = share::config::paths::systemone_models_dir();
    let mut revision_dirs: Vec<PathBuf> = std::fs::read_dir(&models_dir)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.is_dir()
                && path
                    .join(crate::constants::MODEL_MANIFEST_FILE_NAME)
                    .is_file()
        })
        .collect();
    revision_dirs.sort();
    let revision_dir = revision_dirs.pop()?;
    let manifest_source =
        std::fs::read_to_string(revision_dir.join(crate::constants::MODEL_MANIFEST_FILE_NAME))
            .ok()?;
    let manifest = crate::domain::ModelManifest::parse(&manifest_source).ok()?;
    let model_asset = manifest.model_asset().ok()?;
    let pointer_head_asset = manifest.pointer_head_asset().ok()?;
    Some(InstalledModelPaths {
        model_path: revision_dir.join(&model_asset.path),
        tokenizer_path: revision_dir.join(crate::constants::TOKENIZER_JSON_RELATIVE_PATH),
        pointer_head_path: revision_dir.join(&pointer_head_asset.path),
        hidden_size: manifest.hidden_size as usize,
        pointer_dimension: manifest.pointer_dimension as usize,
        temperature: manifest.temperature,
    })
}

fn worker_config(paths: &InstalledModelPaths) -> LlamaWorkerConfig {
    LlamaWorkerConfig {
        model_path: paths.model_path.clone(),
        hidden_size: paths.hidden_size,
        context_tokens: crate::constants::EMBEDDED_CONTEXT_TOKENS,
        ubatch_tokens: crate::constants::EMBEDDED_UBATCH_TOKENS,
    }
}

#[tokio::test]
async fn start_llama_worker_when_model_file_missing_reports_failure() {
    let config = LlamaWorkerConfig {
        model_path: PathBuf::from("/nonexistent/systemone/model.gguf"),
        hidden_size: 1024,
        context_tokens: crate::constants::EMBEDDED_CONTEXT_TOKENS,
        ubatch_tokens: crate::constants::EMBEDDED_UBATCH_TOKENS,
    };
    let error = start_llama_worker(config)
        .await
        .expect_err("模型文件缺失必须 fail-closed");
    assert!(
        matches!(
            error,
            crate::adapters::llama_worker::WorkerInitError::ModelLoadFailed { .. }
        ),
        "错误类型不符：{error:?}"
    );
    assert!(
        error.to_string().contains("不存在"),
        "错误消息为中文：{error}"
    );
}

/// 真实模型端到端（opt-in）：两阶段 MUST 串行——llama-cpp-2 的 backend 是进程级
/// 单例（并存即 `BackendAlreadyInitialized`），拆成两个并行测试会互相跳过、掩盖真实
/// 失败；故阶段 1 的 `LlamaRowEngine` 先析构（同步 `llama_backend_free`），阶段 2
/// 才重新初始化 backend 走完整 facade。资产缺失 → skip 提示；资产存在而初始化 /
/// 前向 / 评分失败 → 测试 MUST 失败（NEVER 以 skip 掩盖）。
#[tokio::test]
#[ignore = "需要本机已安装模型：先执行 aemeath systemone download"]
async fn real_model_scores_deterministically_with_full_width_finite_hidden() {
    let Some(paths) = installed_model_paths() else {
        println!("skip：未找到已安装模型，先执行 `aemeath systemone download`");
        return;
    };
    if !paths.model_path.is_file() {
        println!("skip：模型文件缺失 {}", paths.model_path.display());
        return;
    }

    // 阶段 1：直接驱动原生引擎——宽度与 manifest 对账、hidden 全有限、清 KV 后重复前向稳定。
    let question = ScoringQuestion::noul("Is the task fully complete?", None).unwrap();
    let plan = crate::adapters::kev_question_plan::plan_kev_question(&question);
    {
        let tokenizer = HfKevTokenizer::from_file(&paths.tokenizer_path)
            .unwrap_or_else(|error| panic!("tokenizer 加载失败：{error}"));
        let builder = KevCausalRowBuilder::new(tokenizer);
        let mut engine: LlamaRowEngine = init_llama_row_engine(&worker_config(&paths))
            .unwrap_or_else(|error| panic!("llama.cpp 初始化失败：{error}"));
        let rows = builder
            .build_rows(
                "The agent finished the requested work.",
                std::slice::from_ref(&plan),
            )
            .expect("row 构建成功");
        let row = rows.into_iter().next().expect("一题一行");

        let first = engine.embed_rows(vec![row.clone()]).expect("前向成功");
        assert_eq!(
            first[0].decide.len(),
            paths.hidden_size,
            "embeddings_ith 宽度与 manifest hidden_size 一致"
        );
        assert!(
            first[0].decide.iter().all(|value| value.is_finite()),
            "hidden 全部有限"
        );
        assert!(!first[0].options.is_empty());

        // 清 KV 后重复前向：结果必须稳定（设计「同一 row 清 KV 后重复评分结果稳定」）。
        let second = engine.embed_rows(vec![row]).expect("前向成功");
        assert_eq!(
            first[0].decide, second[0].decide,
            "KV 清空后同一 row 的 hidden 完全一致"
        );
    } // engine 析构 → context / 模型 / backend 同步释放，阶段 2 才能重新 init

    // 阶段 2：facade 端到端——published language → causal rows → 专用 worker → PointerHead → 答案。
    let adapter = EmbeddedScoringAdapter::start_from_paths(
        paths.model_path.clone(),
        paths.tokenizer_path.clone(),
        paths.pointer_head_path.clone(),
        paths.hidden_size,
        paths.pointer_dimension,
        paths.temperature,
    )
    .await
    .unwrap_or_else(|error| panic!("embedded adapter 初始化失败：{error}"));

    let state = ScoringState::new(
        "The agent reconfigured the deploy pipeline and reported the task complete.",
    )
    .expect("状态非空白");
    let questions = vec![ScoringQuestion::choice(
        "Which memory is most relevant to the next step?",
        vec![
            (
                "0".to_owned(),
                "User prefers short answers when reviewing changes.".to_owned(),
            ),
            (
                "1".to_owned(),
                "Deploy pipeline now builds on every push to main.".to_owned(),
            ),
        ],
    )
    .unwrap()];

    let first = adapter.answer(&state, &questions).await.expect("评分成功");
    let second = adapter.answer(&state, &questions).await.expect("评分成功");
    assert_eq!(first.len(), 1);
    match (&first[0], &second[0]) {
        (
            crate::domain::ScoringAnswer::Choice {
                probabilities: first_probabilities,
                ..
            },
            crate::domain::ScoringAnswer::Choice {
                probabilities: second_probabilities,
                ..
            },
        ) => {
            assert_eq!(
                first_probabilities, second_probabilities,
                "同一请求重复评分结果稳定"
            );
        }
        other => panic!("答案类型不符：{other:?}"),
    }
}
