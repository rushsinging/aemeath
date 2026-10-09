//! 79-case embedded 真机门禁（#1833 批次 2，`--ignored` 显式运行）：
//! fixture token ids → llama.cpp Q8_0 前向 → Rust PointerHead → argmax 对拍
//! golden（批次 1 已证 Q8_0 vs MLX golden = 79/79；本门禁锁定 Rust 内嵌链）。
//!
//! 前置（模型缺失时明确提示并 skip，NEVER 下载）：
//! - GGUF：按序探测 `AEMEATH_SYSTEMONE_PARITY_MODEL` →
//!   `~/.agents/models/systemone/*/model.gguf` →
//!   `~/.cache/system-one-eval/kev-merged-q8_0.gguf`
//! - fixture：`eval/system-one/fixtures/parity_q8/`（fixture_parity 门禁同源）
//!
//! 运行：cargo test -p systemone --features embedded embedded_parity -- --ignored --nocapture

use std::path::PathBuf;

use serde::Deserialize;

use super::llama_worker::{start_llama_worker, CausalRow, LlamaWorkerConfig, RowHiddenVectors};
use crate::constants::{EMBEDDED_CONTEXT_TOKENS, EMBEDDED_UBATCH_TOKENS};
use crate::domain::{PointerHead, PointerHeadWeights};

/// fixture 单行（本门禁只消费 ids 与 golden argmax，hidden 不需要）。
#[derive(Deserialize)]
struct ParityFixtureCase {
    dataset: String,
    id: String,
    gold: usize,
    ids: Vec<u32>,
    decide: usize,
    opts: Vec<usize>,
    golden_probs: Vec<f64>,
}

/// fixture 根目录。
fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("eval/system-one/fixtures/parity_q8")
}

/// 按序探测模型 GGUF：显式 env → 标准安装目录 → eval 缓存。
fn locate_model_gguf() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("AEMEATH_SYSTEMONE_PARITY_MODEL") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Some(path);
        }
    }
    let installed_root = share::config::paths::systemone_models_dir();
    if let Ok(entries) = std::fs::read_dir(&installed_root) {
        for entry in entries.flatten() {
            let candidate = entry.path().join("model.gguf");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    let eval_cache = dirs_home().join(".cache/system-one-eval/kev-merged-q8_0.gguf");
    eval_cache.is_file().then_some(eval_cache)
}

/// 用户 home（测试环境探测用，非生产路径）。
fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"))
}

/// fixture manifest（只取本门禁需要的口径字段）。
#[derive(Deserialize)]
struct ParityFixtureManifest {
    temperature: f32,
}

fn read_fixture_manifest() -> ParityFixtureManifest {
    let source = std::fs::read_to_string(fixture_dir().join("manifest.json"))
        .expect("fixture manifest.json 应存在（先运行 export_parity_fixture.py）");
    serde_json::from_str(&source).expect("fixture manifest 解析")
}

/// head.f32 → PointerHead 权重（与 tests/fixture_parity.rs 的 reader 同构，
/// 权重真相源相同：fixture head/head.f32）。
fn fixture_pointer_head() -> PointerHead {
    let bytes = std::fs::read(fixture_dir().join("head/head.f32"))
        .expect("fixture head.f32 应存在（先运行 export_parity_fixture.py）");
    const HIDDEN_SIZE: usize = 1024;
    const POINTER_DIMENSION: usize = 256;
    let matrix_len = POINTER_DIMENSION * HIDDEN_SIZE;
    let floats: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect();
    let weights = PointerHeadWeights::new(
        HIDDEN_SIZE,
        POINTER_DIMENSION,
        read_fixture_manifest().temperature,
        &floats[0..matrix_len],
        &floats[matrix_len..matrix_len + POINTER_DIMENSION],
        &floats[matrix_len + POINTER_DIMENSION..2 * matrix_len + POINTER_DIMENSION],
        &floats[2 * matrix_len + POINTER_DIMENSION..],
    )
    .expect("fixture head 权重构造");
    PointerHead::new(weights)
}

fn read_fixture_cases() -> Vec<ParityFixtureCase> {
    let source = std::fs::read_to_string(fixture_dir().join("79_cases.jsonl"))
        .expect("fixture 79_cases.jsonl 应存在（先运行 export_parity_fixture.py）");
    source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("fixture 行解析"))
        .collect()
}

fn golden_argmax(case: &ParityFixtureCase) -> usize {
    case.golden_probs
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(index, _)| index)
        .expect("golden_probs 非空")
}

/// 79-case argmax 门禁：Q8_0 前向 + Rust PointerHead 对拍 golden，
/// 命中率 ≥99%（历史基线 100%）。
#[tokio::test]
#[ignore = "需要本机 Q8_0 GGUF 模型（探测顺序见模块注释），显式运行"]
async fn embedded_q8_argmax_matches_fixture_golden() {
    let Some(model_path) = locate_model_gguf() else {
        eprintln!(
            "SKIP：未找到 System One Q8_0 GGUF 模型。探测顺序：\
             AEMEATH_SYSTEMONE_PARITY_MODEL → ~/.agents/models/systemone/*/model.gguf →\
             ~/.cache/system-one-eval/kev-merged-q8_0.gguf（本门禁 NEVER 自动下载）"
        );
        return;
    };
    println!("model: {}", model_path.display());
    let cases = read_fixture_cases();
    assert_eq!(cases.len(), 79, "fixture 行数");

    // llama.cpp 0.1.159 已知缺陷（ggml-metal-device.m:1025 GGML_ASSERT
    // rsets count == 0）：residency sets 未释放即 device_free → SIGABRT。
    // 生产规避位于 `init_llama_row_engine`（engine init 期统一设置）；测试
    // 侧再显式设置一次以保持自持（不依赖生产实现细节）。
    std::env::set_var("GGML_METAL_NO_RESIDENCY", "1");
    let client = start_llama_worker(LlamaWorkerConfig {
        model_path,
        hidden_size: 1024,
        context_tokens: EMBEDDED_CONTEXT_TOKENS,
        ubatch_tokens: EMBEDDED_UBATCH_TOKENS,
    })
    .await
    .expect("llama worker 启动");
    let head = fixture_pointer_head();

    let mut hits = 0_usize;
    let mut max_delta = 0.0_f32;
    let mut flipped_results: Vec<usize> = Vec::with_capacity(cases.len());
    // 验收包输出：逐 case 结果（含 order-flip 反转轮与批延迟），落
    // eval/system-one/results/ 供 acceptance 脚本计算场景指标。
    let mut report_rows: Vec<serde_json::Value> = Vec::with_capacity(cases.len());
    let mut flipped_rows: Vec<CausalRow> = Vec::with_capacity(cases.len());
    // 分批提交：batch 容量 16k，row 最长 ~1.5k，8 行/批留足余量。
    for batch in cases.chunks(8) {
        let rows: Vec<CausalRow> = batch
            .iter()
            .map(|case| {
                CausalRow::new(
                    case.ids.iter().map(|token| *token as i32).collect(),
                    case.decide,
                    case.opts.clone(),
                )
            })
            .collect();
        let batch_started = std::time::Instant::now();
        let vectors: Vec<RowHiddenVectors> = client.run_rows(rows).await.expect("llama 前向应成功");
        let batch_latency_ms = batch_started.elapsed().as_millis();
        assert_eq!(vectors.len(), batch.len(), "返回 hidden 行数应与提交一致");
        // order-flip 轮：选项顺序整体反转（rank 题型翻转稳定性验收）。
        for case in batch {
            flipped_rows.push(CausalRow::new(
                case.ids.iter().map(|token| *token as i32).collect(),
                case.decide,
                case.opts.iter().rev().copied().collect(),
            ));
        }
        for (case_index, (case, vector)) in batch.iter().zip(vectors).enumerate() {
            let mut options_flat = Vec::with_capacity(vector.options.len() * vector.decide.len());
            for option in &vector.options {
                options_flat.extend_from_slice(option);
            }
            let probs = head
                .score_options(&vector.decide, &options_flat)
                .expect("Rust PointerHead 评分");
            let rust_argmax = probs
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map(|(index, _)| index)
                .expect("probs 非空");
            let expected = golden_argmax(case);
            hits += usize::from(rust_argmax == expected);
            let case_delta = case
                .golden_probs
                .iter()
                .zip(probs.iter())
                .map(|(golden, actual)| (*golden - f64::from(*actual)).abs() as f32)
                .fold(0.0_f32, f32::max);
            max_delta = max_delta.max(case_delta);
            report_rows.push(serde_json::json!({
                "dataset": case.dataset,
                "id": case.id,
                "gold": case.gold,
                "n_options": case.opts.len(),
                "argmax": rust_argmax,
                "argmax_golden": expected,
                "p_true": if case.opts.len() == 2 { probs[1] } else { 0.0 },
                "probs": probs,
                "batch_index": case_index,
                "batch_latency_ms": batch_latency_ms,
            }));
            if rust_argmax != expected {
                println!(
                    "MISS [{}]: golden={expected} rust={rust_argmax} probs={probs:?}",
                    case.id
                );
            }
        }
    }
    // order-flip 轮前向（反转后的 rows 已收集，逐批推理后记录翻转 argmax）。
    for batch in flipped_rows.chunks(8) {
        let vectors: Vec<RowHiddenVectors> = client
            .run_rows(batch.to_vec())
            .await
            .expect("flip 前向应成功");
        for (row, vector) in batch.iter().zip(vectors) {
            let mut options_flat = Vec::with_capacity(vector.options.len() * vector.decide.len());
            for option in &vector.options {
                options_flat.extend_from_slice(option);
            }
            let probs = head
                .score_options(&vector.decide, &options_flat)
                .expect("flip 评分");
            let flipped_argmax = probs
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map(|(index, _)| index)
                .expect("probs 非空");
            flipped_results.push(flipped_argmax);
        }
    }
    // 合并：argmax_flipped 记录「反转序中的 argmax」（原序位置）。
    for (row, flipped_argmax) in report_rows.iter_mut().zip(flipped_results) {
        let n_options = row["n_options"].as_u64().unwrap_or(0) as usize;
        row["argmax_flipped"] = serde_json::json!(n_options - 1 - flipped_argmax);
    }
    let report_path = fixture_dir().join("../../results/embedded_rust_parity.json");
    if let Some(parent) = report_path.parent() {
        std::fs::create_dir_all(parent).expect("results 目录创建");
    }
    std::fs::write(
        &report_path,
        serde_json::to_string_pretty(&report_rows).expect("报告序列化") + "\n",
    )
    .expect("验收报告写入");
    println!("report: {}", report_path.display());
    println!("argmax 命中 {hits}/79，max|Δp|={max_delta:.4}");
    assert!(
        hits * 100 >= 79 * 99,
        "Q8_0 argmax 命中率 {hits}/79 低于 99% 门禁"
    );
    // llama.cpp 0.1.158 在进程 exit 期析构全局 Metal device 注册表时会
    // double-free（SIGABRT，判定完成后发生）：门禁 MUST 单测过滤运行
    //（`--lib embedded_parity -- --ignored`），此处显式短路退出避免
    // shell 侧拿到 101 误判失败。
    std::process::exit(0);
}
