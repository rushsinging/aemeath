//! kev 答案映射：PointerHead 概率 + KevQuestionPlan → ScoringAnswer。
//!
//! 复刻 kev `api.py::to_answers` 口径：Noul 取 `probs[1]` 为 p(true)；
//! Choice 取 argmax key、probabilities 按 criteria 顺序；Score 取 `Σ i·p_i`。
//! 置信度公式与 kev 一致（TypeSafe reference adapter 口径）：
//! - choice：`(p_max - 1/K) / (1 - 1/K)`（均匀 → 0，全质量 → 1）；
//! - score：`max(0, 1 - E|level - mode| / D)`，`D` 为均匀分布对中心的平均绝对偏差。

use crate::adapters::kev_question_plan::{KevQuestionKind, KevQuestionPlan};
use crate::domain::{AnswerRejected, CalibrationLevel, ScoringAnswer};

/// 答案构建失败（概率数量 / 构造不变量）→ Server 降级。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AnswerBuildFailure {
    /// 概率数量与 plan 选项数不一致（worker 契约被破坏）。
    OptionCountMismatch { expected: usize, found: usize },
    /// 概率未通过 `ScoringAnswer` 构造不变量（非概率值等）。
    ProbabilityRejected { detail: String },
}

impl std::fmt::Display for AnswerBuildFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OptionCountMismatch { expected, found } => {
                write!(
                    formatter,
                    "概率数量与选项数不符：期望 {expected}，实际 {found}"
                )
            }
            Self::ProbabilityRejected { detail } => write!(formatter, "{detail}"),
        }
    }
}

impl std::error::Error for AnswerBuildFailure {}

/// 按题型把概率分布映射为 `ScoringAnswer`（校准级别 = 出厂温度已在 PointerHead 内生效）。
pub(crate) fn build_kev_answer(
    plan: &KevQuestionPlan,
    probabilities: &[f64],
) -> Result<ScoringAnswer, AnswerBuildFailure> {
    if probabilities.len() != plan.option_texts.len() {
        return Err(AnswerBuildFailure::OptionCountMismatch {
            expected: plan.option_texts.len(),
            found: probabilities.len(),
        });
    }
    match plan.kind {
        KevQuestionKind::Noul => {
            // kev：Noul → 2 options [false, true]，答案 = p(true) = probs[1]。
            let p_true = probabilities[1];
            ScoringAnswer::noul(p_true, CalibrationLevel::Temperature).map_err(|error| {
                AnswerBuildFailure::ProbabilityRejected {
                    detail: answer_rejected_detail(error),
                }
            })
        }
        KevQuestionKind::Choice => {
            let argmax_index = argmax_first(probabilities);
            let choice = plan.probability_keys[argmax_index].clone();
            let paired: Vec<(String, f64)> = plan
                .probability_keys
                .iter()
                .zip(probabilities.iter().copied())
                .map(|(key, probability)| (key.clone(), probability))
                .collect();
            let confidence = kev_choice_confidence(probabilities);
            ScoringAnswer::choice(choice, paired, confidence, CalibrationLevel::Temperature)
                .map_err(|error| AnswerBuildFailure::ProbabilityRejected {
                    detail: answer_rejected_detail(error),
                })
        }
        KevQuestionKind::Score => {
            let score: f64 = probabilities
                .iter()
                .enumerate()
                .map(|(index, probability)| index as f64 * probability)
                .sum();
            let confidence = kev_score_confidence(probabilities);
            ScoringAnswer::score(
                score,
                probabilities.to_vec(),
                confidence,
                CalibrationLevel::Temperature,
            )
            .map_err(|error| AnswerBuildFailure::ProbabilityRejected {
                detail: answer_rejected_detail(error),
            })
        }
    }
}

/// kev `choice_confidence`：均匀 → 0，全质量 → 1（单选项恒为 1）。
pub(crate) fn kev_choice_confidence(probabilities: &[f64]) -> f64 {
    let normalized = normalize_probabilities(probabilities);
    let option_count = normalized.len();
    if option_count == 1 {
        return 1.0;
    }
    let peak = normalized.iter().copied().fold(0.0_f64, f64::max);
    let uniform = 1.0 / option_count as f64;
    (peak - uniform) / (1.0 - uniform)
}

/// kev `score_confidence`：`max(0, 1 - E|level - mode| / D)`（mode 取首个最高概率档）。
pub(crate) fn kev_score_confidence(probabilities: &[f64]) -> f64 {
    let level_count = probabilities.len();
    if level_count == 1 {
        return 1.0;
    }
    let normalized = normalize_probabilities(probabilities);
    let mode = argmax_first(&normalized);
    let uniform_deviation: f64 = (0..level_count)
        .map(|index| (index as f64 - (level_count as f64 - 1.0) / 2.0).abs())
        .sum::<f64>()
        / level_count as f64;
    let mode_deviation: f64 = normalized
        .iter()
        .enumerate()
        .map(|(index, probability)| probability * (index as f64 - mode as f64).abs())
        .sum();
    (1.0 - mode_deviation / uniform_deviation).max(0.0)
}

/// 归一化为和 1 的分布（全零 → 均匀），与 kev `_normalize` 一致。
fn normalize_probabilities(probabilities: &[f64]) -> Vec<f64> {
    let total: f64 = probabilities.iter().sum();
    if total == 0.0 {
        return vec![1.0 / probabilities.len() as f64; probabilities.len()];
    }
    probabilities.iter().map(|value| value / total).collect()
}

/// 首个最大值的下标（kev `max(range, key=...)` 口径：并列取最先）。
fn argmax_first(probabilities: &[f64]) -> usize {
    let mut best_index = 0;
    for (index, probability) in probabilities.iter().enumerate().skip(1) {
        if *probability > probabilities[best_index] {
            best_index = index;
        }
    }
    best_index
}

/// `AnswerRejected` → 中文明细（与 jev_wire 的线格式拒绝明细同一口径）。
fn answer_rejected_detail(error: AnswerRejected) -> String {
    format!("答案不变量校验失败：{error:?}")
}

#[cfg(test)]
#[path = "kev_answer_mapping_tests.rs"]
mod tests;
