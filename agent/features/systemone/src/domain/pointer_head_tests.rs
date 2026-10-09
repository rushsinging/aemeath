//! PointerHead 纯数学：固定小维参考向量、温度缩放、溢出稳定性、错误与 argmax。

use super::{PointerHead, PointerHeadError, PointerHeadWeights};

/// 固定小维参考配置（hidden=2、pointer=2），期望值对齐 kev 口径的 Python 参考实现。
fn small_dimension_head(temperature: f32) -> PointerHead {
    let weights = PointerHeadWeights::new(
        2,
        2,
        temperature,
        &[1.0, 2.0, 3.0, 4.0],
        &[0.5, -0.5],
        &[1.0, 0.0, 0.0, 1.0],
        &[0.0, 0.0],
    )
    .expect("固定小维合法权重应通过校验");
    PointerHead::new(weights)
}

/// 断言概率向量与参考实现逐项一致、全部有限且求和约为 1。
fn assert_probabilities_match(probabilities: &[f32], reference: &[f32]) {
    assert_eq!(probabilities.len(), reference.len(), "概率向量长度不符");
    for (index, (found, expected)) in probabilities.iter().zip(reference).enumerate() {
        assert!(found.is_finite(), "第 {index} 个概率非有限：{found}");
        assert!(
            (found - expected).abs() <= 1e-5,
            "第 {index} 个概率 {found} 与参考值 {expected} 偏差超过 1e-5"
        );
    }
    let total: f32 = probabilities.iter().sum();
    assert!((total - 1.0).abs() <= 1e-6, "概率和 {total} 应约为 1");
}

/// 返回最大概率所在选项下标。
fn argmax(probabilities: &[f32]) -> usize {
    probabilities
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| left.total_cmp(right))
        .map(|(index, _)| index)
        .expect("概率向量非空")
}

#[test]
fn score_options_with_fixed_small_dimensions_matches_reference_vector() {
    let head = small_dimension_head(1.0);
    let probabilities = head
        .score_options(&[1.0, 1.0], &[1.0, 0.0, 0.0, 1.0, 2.0, 2.0])
        .expect("全部有限输入应成功");
    // q = [3.5, 6.5]；logits = [3.5, 6.5, 20] / sqrt(2) / 1
    assert_probabilities_match(&probabilities, &[8.569_151e-6, 7.148_51e-5, 0.9999199]);
}

#[test]
fn score_options_with_doubled_temperature_softens_distribution() {
    let head = small_dimension_head(2.0);
    let probabilities = head
        .score_options(&[1.0, 1.0], &[1.0, 0.0, 0.0, 1.0, 2.0, 2.0])
        .expect("全部有限输入应成功");
    // 同一组 logits 除以 temperature=2 后重做 softmax。
    assert_probabilities_match(&probabilities, &[0.0028944815, 0.008_360_065, 0.98874545]);
    // 温度只重缩放，NEVER 改变胜负排序。
    assert_eq!(argmax(&probabilities), 2);
}

#[test]
fn score_options_when_logits_exceed_exponential_range_stays_finite() {
    // logits = [800, 700]：naive exp 溢出，减去最大值后必须稳定。
    let weights = PointerHeadWeights::new(1, 1, 1.0, &[100.0], &[0.0], &[10.0], &[0.0])
        .expect("合法权重应通过校验");
    let head = PointerHead::new(weights);
    let probabilities = head
        .score_options(&[1.0], &[0.8, 0.7])
        .expect("溢出区间 logits 仍应产出稳定概率");
    assert_eq!(probabilities.len(), 2);
    for (index, probability) in probabilities.iter().enumerate() {
        assert!(
            probability.is_finite(),
            "第 {index} 个概率非有限：{probability}"
        );
    }
    assert!(
        probabilities[0] > 0.999_999,
        "最大 logit 选项应占满概率：{}",
        probabilities[0]
    );
    let total: f32 = probabilities.iter().sum();
    assert!((total - 1.0).abs() <= 1e-6, "概率和 {total} 应约为 1");
}

#[test]
fn score_options_argmax_matches_highest_logit_option() {
    let head = small_dimension_head(1.0);
    // 中间选项与 query 点积最大（dot=20），argmax 必须落在下标 1。
    let probabilities = head
        .score_options(&[1.0, 1.0], &[1.0, 0.0, 2.0, 2.0, 0.0, 1.0])
        .expect("全部有限输入应成功");
    assert_eq!(argmax(&probabilities), 1);
}

#[test]
fn new_when_query_weight_length_mismatched_rejects() {
    let error = PointerHeadWeights::new(
        2,
        2,
        1.0,
        &[1.0, 2.0],
        &[0.0, 0.0],
        &[1.0, 0.0, 0.0, 1.0],
        &[0.0, 0.0],
    )
    .expect_err("query_weight 长度应为 pointer×hidden");
    assert_eq!(
        error,
        PointerHeadError::WeightLengthMismatch {
            weight: "query_weight",
            expected: 4,
            found: 2
        }
    );
}

#[test]
fn new_when_key_bias_length_mismatched_rejects() {
    let error = PointerHeadWeights::new(
        2,
        2,
        1.0,
        &[1.0, 2.0, 3.0, 4.0],
        &[0.5, -0.5],
        &[1.0, 0.0, 0.0, 1.0],
        &[0.0],
    )
    .expect_err("key_bias 长度应为 pointer");
    assert_eq!(
        error,
        PointerHeadError::WeightLengthMismatch {
            weight: "key_bias",
            expected: 2,
            found: 1
        }
    );
}

#[test]
fn new_when_hidden_size_is_zero_rejects() {
    let error = PointerHeadWeights::new(0, 2, 1.0, &[], &[], &[], &[])
        .expect_err("hidden_size 为 0 必须拒绝");
    assert_eq!(
        error,
        PointerHeadError::NonPositiveDimension {
            dimension: "hidden_size"
        }
    );
}

#[test]
fn new_when_pointer_dimension_is_zero_rejects() {
    let error = PointerHeadWeights::new(2, 0, 1.0, &[], &[], &[], &[])
        .expect_err("pointer_dimension 为 0 必须拒绝");
    assert_eq!(
        error,
        PointerHeadError::NonPositiveDimension {
            dimension: "pointer_dimension"
        }
    );
}

#[test]
fn new_when_weight_value_is_not_finite_rejects() {
    let error = PointerHeadWeights::new(
        2,
        2,
        1.0,
        &[1.0, f32::NAN, 3.0, 4.0],
        &[0.5, -0.5],
        &[1.0, 0.0, 0.0, 1.0],
        &[0.0, 0.0],
    )
    .expect_err("NaN 权重必须拒绝");
    assert_eq!(
        error,
        PointerHeadError::NonFiniteWeight {
            weight: "query_weight"
        }
    );

    let error = PointerHeadWeights::new(
        2,
        2,
        1.0,
        &[1.0, 2.0, 3.0, 4.0],
        &[0.5, -0.5],
        &[1.0, 0.0, 0.0, 1.0],
        &[0.0, f32::INFINITY],
    )
    .expect_err("无穷 key_bias 必须拒绝");
    assert_eq!(
        error,
        PointerHeadError::NonFiniteWeight { weight: "key_bias" }
    );
}

#[test]
fn new_when_temperature_is_not_finite_positive_rejects() {
    for temperature in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let error = PointerHeadWeights::new(
            2,
            2,
            temperature,
            &[1.0, 2.0, 3.0, 4.0],
            &[0.5, -0.5],
            &[1.0, 0.0, 0.0, 1.0],
            &[0.0, 0.0],
        )
        .expect_err("非有限或非正 temperature 必须拒绝");
        assert_eq!(error, PointerHeadError::InvalidTemperature);
    }
}

#[test]
fn score_options_when_hidden_decide_length_mismatched_rejects() {
    let head = small_dimension_head(1.0);
    let error = head
        .score_options(&[1.0, 1.0, 1.0], &[1.0, 0.0])
        .expect_err("decide hidden 长度不符必须拒绝");
    assert_eq!(
        error,
        PointerHeadError::HiddenLengthMismatch {
            hidden: "decide",
            expected: 2,
            found: 3
        }
    );
}

#[test]
fn score_options_when_hidden_decide_not_finite_rejects() {
    let head = small_dimension_head(1.0);
    let error = head
        .score_options(&[1.0, f32::NAN], &[1.0, 0.0])
        .expect_err("NaN decide hidden 必须拒绝");
    assert_eq!(
        error,
        PointerHeadError::NonFiniteHidden { hidden: "decide" }
    );
}

#[test]
fn score_options_when_options_are_empty_rejects() {
    let head = small_dimension_head(1.0);
    let error = head
        .score_options(&[1.0, 1.0], &[])
        .expect_err("空选项集必须拒绝");
    assert_eq!(error, PointerHeadError::EmptyOptions);
}

#[test]
fn score_options_when_option_length_is_not_multiple_of_hidden_size_rejects() {
    let head = small_dimension_head(1.0);
    let error = head
        .score_options(&[1.0, 1.0], &[1.0, 0.0, 1.0])
        .expect_err("选项长度非 hidden 整数倍必须拒绝");
    assert_eq!(
        error,
        PointerHeadError::OptionsLengthMismatch {
            found: 3,
            hidden_size: 2
        }
    );
}

#[test]
fn score_options_when_option_value_is_not_finite_rejects() {
    let head = small_dimension_head(1.0);
    let error = head
        .score_options(&[1.0, 1.0], &[1.0, 0.0, f32::INFINITY, 1.0])
        .expect_err("无穷选项 hidden 必须拒绝");
    assert_eq!(error, PointerHeadError::NonFiniteOption { option_index: 1 });
}

#[test]
fn score_options_when_projection_overflows_to_infinity_rejects() {
    let weights = PointerHeadWeights::new(1, 1, 1.0, &[f32::MAX], &[0.0], &[1.0], &[0.0])
        .expect("f32::MAX 是有限值，应通过权重校验");
    let head = PointerHead::new(weights);
    let error = head
        .score_options(&[2.0], &[1.0])
        .expect_err("投影溢出必须拒绝");
    assert_eq!(
        error,
        PointerHeadError::NonFiniteProjection {
            projection: "query"
        }
    );
}

#[test]
fn score_options_when_logit_overflows_to_infinity_rejects() {
    let weights = PointerHeadWeights::new(1, 1, 1.0, &[f32::MAX], &[0.0], &[f32::MAX], &[0.0])
        .expect("f32::MAX 是有限值，应通过权重校验");
    let head = PointerHead::new(weights);
    let error = head
        .score_options(&[1.0], &[1.0])
        .expect_err("点积溢出产生的非有限 logit 必须拒绝");
    assert_eq!(error, PointerHeadError::NonFiniteLogit { option_index: 0 });
}

#[test]
fn getters_expose_declared_contract_values_for_loader_reconciliation() {
    let weights = PointerHeadWeights::new(
        2,
        2,
        0.07,
        &[1.0, 2.0, 3.0, 4.0],
        &[0.5, -0.5],
        &[1.0, 0.0, 0.0, 1.0],
        &[0.0, 0.0],
    )
    .expect("合法权重应通过校验");
    assert_eq!(weights.hidden_size(), 2);
    assert_eq!(weights.pointer_dimension(), 2);
    assert_eq!(weights.temperature(), 0.07);
}
