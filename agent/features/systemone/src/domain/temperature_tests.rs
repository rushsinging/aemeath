//! 温度缩放行为测试：argmax 不变性是校准克制原则的机械锁定。

use super::*;

fn argmax_index(probabilities: &[f64]) -> usize {
    probabilities
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| left.total_cmp(right))
        .map(|(index, _)| index)
        .expect("非空分布必有 argmax")
}

#[test]
fn temperature_one_is_identity() {
    let source = vec![0.7, 0.2, 0.1];
    let scaled = scale_probabilities(&source, 1.0).expect("T=1 应成功");
    for (original, scaled) in source.iter().zip(scaled.iter()) {
        assert!((original - scaled).abs() < 1e-9, "T=1 应保持原值");
    }
}

#[test]
fn temperature_above_one_softens_distribution() {
    let source = vec![0.8, 0.15, 0.05];
    let scaled = scale_probabilities(&source, 2.0).expect("T=2 应成功");
    assert!(
        scaled[0] < source[0],
        "软化应降低头部概率：{} -> {}",
        source[0],
        scaled[0]
    );
    assert!(scaled[2] > source[2], "软化应抬升尾部概率");
}

#[test]
fn temperature_below_one_sharpens_distribution() {
    let source = vec![0.6, 0.3, 0.1];
    let scaled = scale_probabilities(&source, 0.5).expect("T=0.5 应成功");
    assert!(
        scaled[0] > source[0],
        "锐化应抬升头部概率：{} -> {}",
        source[0],
        scaled[0]
    );
}

#[test]
fn argmax_invariant_across_temperatures() {
    let source = vec![0.1, 0.5, 0.25, 0.15];
    let expected = argmax_index(&source);
    for temperature in [0.1, 0.3, 0.7, 1.0, 1.5, 3.0, 10.0] {
        let scaled = scale_probabilities(&source, temperature).expect("合法温度应成功");
        assert_eq!(
            argmax_index(&scaled),
            expected,
            "T={temperature} 改变了胜负排序"
        );
    }
}

#[test]
fn scaled_output_stays_normalized() {
    let source = vec![0.4, 0.35, 0.25];
    let scaled = scale_probabilities(&source, 1.8).expect("合法温度应成功");
    let total: f64 = scaled.iter().sum();
    assert!(
        (total - 1.0).abs() < 1e-9,
        "缩放后应保持归一化，实际 {total}"
    );
}

#[test]
fn invalid_temperature_rejected() {
    let source = vec![0.5, 0.5];
    assert!(scale_probabilities(&source, 0.0).is_none());
    assert!(scale_probabilities(&source, -1.0).is_none());
    assert!(scale_probabilities(&source, f64::NAN).is_none());
    assert!(scale_probabilities(&source, f64::INFINITY).is_none());
}

#[test]
fn invalid_probability_rejected() {
    assert!(scale_probabilities(&[f64::NAN, 0.5], 1.0).is_none());
    assert!(scale_probabilities(&[1.5, 0.5], 1.0).is_none());
    assert!(scale_probabilities(&[-0.1, 0.5], 1.0).is_none());
    assert!(scale_probabilities(&[], 1.0).is_none());
}

#[test]
fn zero_probability_does_not_poison_output() {
    let source = vec![0.0, 0.9, 0.1];
    let scaled = scale_probabilities(&source, 0.8).expect("含零概率应成功");
    assert!(scaled.iter().all(|p| p.is_finite()), "不得产出 NaN/inf");
    assert_eq!(argmax_index(&scaled), 1);
}
