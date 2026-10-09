//! 79-case fixture 对拍门禁（#1833 批次 2）：验证 Rust PointerHead 数学与
//! Python 导出口径（torch fp32 hidden + Python PointerHead softmax）恒等。
//!
//! fixture 由 `eval/system-one/harness/export_parity_fixture.py` 生成
//! （零网络：golden hidden 来自合并后 backbone 的 torch fp32 前向，
//! golden probs 由同 hidden 经 Python PointerHead 计算，同一口径自洽）；
//! 本测试零模型、零网络，任何环境 MUST 可跑。

use std::path::PathBuf;

use systemone::{PointerHead, PointerHeadWeights};

/// fixture 根目录（仓库内 eval 数据，随 git 提交）。
fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("eval/system-one/fixtures/parity_q8")
}

/// fixture manifest 结构（生成口径的单一真相）。
#[derive(serde::Deserialize)]
struct FixtureManifest {
    schema_version: u32,
    case_count: usize,
    hidden_size: usize,
    pointer_dimension: usize,
    temperature: f32,
}

/// 单 case 行（golden hidden + probs + llama.cpp 前向所需的 token ids）。
#[derive(serde::Deserialize)]
struct FixtureCase {
    dataset: String,
    id: String,
    n_options: usize,
    /// row 完整 token ids（state ids + option ids，kev encode 口径）。
    ids: Vec<u32>,
    /// decide token 在 ids 内的位置。
    decide: usize,
    /// 各 option token 在 ids 内的位置（长度 = n_options）。
    opts: Vec<usize>,
    /// Python PointerHead(hidden) softmax 的 golden 概率（选项序）。
    golden_probs: Vec<f64>,
    /// decide 位置 hidden（hidden_size × f32）。
    decide_hidden: Vec<f64>,
    /// 全部 option hidden（n_options × hidden_size flat）。
    options_hidden: Vec<f64>,
}

fn read_manifest() -> FixtureManifest {
    let source = std::fs::read_to_string(fixture_dir().join("manifest.json"))
        .expect("fixture manifest.json 应存在（运行 export_parity_fixture.py 生成）");
    serde_json::from_str(&source).expect("manifest.json 解析")
}

fn read_cases() -> Vec<FixtureCase> {
    let source = std::fs::read_to_string(fixture_dir().join("79_cases.jsonl"))
        .expect("fixture 79_cases.jsonl 应存在（运行 export_parity_fixture.py 生成）");
    source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("fixture 行解析"))
        .collect()
}

/// head.f32 裸二进制（q_weight | q_bias | k_weight | k_bias，f32 LE）。
fn read_head_weights(manifest: &FixtureManifest) -> PointerHeadWeights {
    let bytes = std::fs::read(fixture_dir().join("head/head.f32"))
        .expect("head.f32 应存在（运行 export_parity_fixture.py 生成）");
    let matrix_len = manifest.pointer_dimension * manifest.hidden_size;
    let bias_len = manifest.pointer_dimension;
    let expected_bytes = (2 * matrix_len + 2 * bias_len) * 4;
    assert_eq!(
        bytes.len(),
        expected_bytes,
        "head.f32 字节数应与声明维度一致"
    );
    let floats: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect();
    let query_weight = &floats[0..matrix_len];
    let query_bias = &floats[matrix_len..matrix_len + bias_len];
    let key_weight = &floats[matrix_len + bias_len..2 * matrix_len + bias_len];
    let key_bias = &floats[2 * matrix_len + bias_len..];
    PointerHeadWeights::new(
        manifest.hidden_size,
        manifest.pointer_dimension,
        manifest.temperature,
        query_weight,
        query_bias,
        key_weight,
        key_bias,
    )
    .expect("fixture head 权重构造")
}

/// manifest 与生产口径锁定的维度一致（kev 0.8B 决策位口径）。
#[test]
fn fixture_manifest_matches_production_shape() {
    let manifest = read_manifest();
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.case_count, 79);
    assert_eq!(manifest.hidden_size, 1024);
    assert_eq!(manifest.pointer_dimension, 256);
    assert!((manifest.temperature - 2.351_095_8).abs() < 1e-6);
}

/// 每行 fixture 完整自洽：probs 长度/归一、hidden/ids 布局一致。
#[test]
fn fixture_rows_are_complete_and_self_consistent() {
    let manifest = read_manifest();
    let cases = read_cases();
    assert_eq!(cases.len(), manifest.case_count, "行数应与 manifest 一致");
    for case in &cases {
        assert_eq!(
            case.golden_probs.len(),
            case.n_options,
            "[{}] probs 长度",
            case.id
        );
        assert_eq!(case.opts.len(), case.n_options, "[{}] opts 数量", case.id);
        assert_eq!(
            case.decide_hidden.len(),
            manifest.hidden_size,
            "[{}] decide hidden 长度",
            case.id
        );
        assert_eq!(
            case.options_hidden.len(),
            case.n_options * manifest.hidden_size,
            "[{}] options hidden 长度",
            case.id
        );
        let sum: f64 = case.golden_probs.iter().sum();
        assert!(
            (sum - 1.0).abs() < 1e-3,
            "[{}] probs 应归一（和={sum}）",
            case.id
        );
        assert!(case.decide < case.ids.len(), "[{}] decide 越界", case.id);
        for option_pos in &case.opts {
            assert!(*option_pos < case.ids.len(), "[{}] opt 越界", case.id);
        }
        assert!(
            case.ids.len() <= 73_728,
            "[{}] row 长度应在 kev 编码口径内",
            case.id
        );
    }
}

/// Rust PointerHead 数学与 Python 导出口径恒等：全部 79 case argmax 一致且
/// max|Δp| ≤ 1e-4（同 hidden 同权重的纯数学对拍，允许 f32 舍入误差）。
#[test]
fn rust_pointer_head_matches_fixture_golden_probs() {
    let manifest = read_manifest();
    let weights = read_head_weights(&manifest);
    let head = PointerHead::new(weights);
    let cases = read_cases();
    let mut max_delta = 0.0_f32;
    for case in &cases {
        let decide_hidden: Vec<f32> = case.decide_hidden.iter().map(|v| *v as f32).collect();
        let options_hidden: Vec<f32> = case.options_hidden.iter().map(|v| *v as f32).collect();
        let probs = head
            .score_options(&decide_hidden, &options_hidden)
            .expect("fixture hidden 应通过校验");
        let golden_argmax = case
            .golden_probs
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(index, _)| index)
            .expect("golden_probs 非空");
        let rust_argmax = probs
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(index, _)| index)
            .expect("probs 非空");
        assert_eq!(
            golden_argmax, rust_argmax,
            "[{}@{}] argmax 不一致：golden={:?} rust={:?}",
            case.dataset, case.id, case.golden_probs, probs
        );
        for (golden, actual) in case.golden_probs.iter().zip(probs.iter()) {
            max_delta = max_delta.max((golden - *actual as f64).abs() as f32);
        }
    }
    assert!(
        max_delta <= 1e-4,
        "PointerHead 数学对拍 max|Δp|={max_delta} 超出 1e-4"
    );
}
