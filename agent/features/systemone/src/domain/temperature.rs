//! 全局温度缩放：对概率分布做 softmax(log(p)/T) 重缩放。
//!
//! 置信度克制原则（吸收 rsi-jev）：校准只重缩放，**NEVER 改变选项胜负排序**
//! （T>0 时 log 单调，argmax 不变）。

/// 用温度 `temperature` 重缩放概率分布。
///
/// - `temperature == 1.0`：恒等；
/// - `> 1.0`：软化（分布更均匀）；
/// - `< 1.0`：锐化（头部更集中）。
///
/// 温度非法（≤0、NaN、无穷）或概率值非法（NaN、越界）时返回 `None`；
/// `0` 概率裁剪到 `1e-12` 避免 log(0) 传播。
pub fn scale_probabilities(probabilities: &[f64], temperature: f64) -> Option<Vec<f64>> {
    if !temperature.is_finite() || temperature <= 0.0 {
        return None;
    }
    if probabilities.is_empty() {
        return None;
    }
    if probabilities
        .iter()
        .any(|probability| !probability.is_finite() || !(0.0..=1.0).contains(probability))
    {
        return None;
    }
    const FLOOR: f64 = 1e-12;
    let scaled: Vec<f64> = probabilities
        .iter()
        .map(|probability| probability.max(FLOOR).ln() / temperature)
        .collect();
    let peak = scaled.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let weights: Vec<f64> = scaled.iter().map(|logit| (logit - peak).exp()).collect();
    let total: f64 = weights.iter().sum();
    Some(weights.iter().map(|weight| weight / total).collect())
}

#[cfg(test)]
#[path = "temperature_tests.rs"]
mod tests;
