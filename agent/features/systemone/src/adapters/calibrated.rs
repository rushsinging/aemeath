//! 校准装饰器：按 CalibrationStore 的 artifact 对内层评分答案做温度重缩放。
//!
//! 置信度克制原则（吸收 rsi-jev）：校准只重缩放，**NEVER 改变选项胜负排序**
//! （温度缩放 argmax 不变，Choice 的 choice key 与 Score 的分档顺序保持）。

use std::sync::Arc;

use async_trait::async_trait;

use crate::adapters::calibration_store::CalibrationStore;
use crate::domain::temperature::scale_probabilities;
use crate::domain::{
    CalibrationLevel, ScoringAnswer, ScoringQuestion, ScoringState, ScoringUnavailable,
};
use crate::ports::ScoringPort;

/// 带校准的评分装饰器：内层 adapter 的答案按校准 artifact 重缩放。
pub struct CalibratedScoringAdapter {
    inner: Arc<dyn ScoringPort>,
    level: CalibrationLevel,
    temperature: Option<f64>,
}

impl CalibratedScoringAdapter {
    /// 从 `CalibrationStore` 装配：有 artifact → `Temperature`，无 → `Raw` 透传。
    pub fn new(inner: Arc<dyn ScoringPort>, store: &CalibrationStore) -> Self {
        match store.artifact() {
            Some(artifact) => Self {
                inner,
                level: CalibrationLevel::Temperature,
                temperature: Some(artifact.temperature()),
            },
            None => Self {
                inner,
                level: CalibrationLevel::Raw,
                temperature: None,
            },
        }
    }
}

#[async_trait]
impl ScoringPort for CalibratedScoringAdapter {
    async fn answer(
        &self,
        state: &ScoringState,
        questions: &[ScoringQuestion],
    ) -> Result<Vec<ScoringAnswer>, ScoringUnavailable> {
        let answers = self.inner.answer(state, questions).await?;
        Ok(answers
            .into_iter()
            .map(|answer| self.rescale(answer))
            .collect())
    }
}

impl CalibratedScoringAdapter {
    fn rescale(&self, answer: ScoringAnswer) -> ScoringAnswer {
        match (self.level, self.temperature) {
            (CalibrationLevel::Temperature, Some(temperature)) => {
                rescale_with_temperature(answer, temperature)
            }
            // Raw 透传；Head 为后置演进，当前按 Raw 处理。
            _ => answer,
        }
    }
}

fn rescale_with_temperature(answer: ScoringAnswer, temperature: f64) -> ScoringAnswer {
    match &answer {
        ScoringAnswer::Noul { p_true, .. } => {
            match scale_probabilities(&[*p_true, 1.0 - p_true], temperature) {
                Some(scaled) => ScoringAnswer::Noul {
                    p_true: scaled[0],
                    calibration: CalibrationLevel::Temperature,
                },
                None => answer,
            }
        }
        ScoringAnswer::Choice {
            choice,
            probabilities,
            ..
        } => {
            let source: Vec<f64> = probabilities.iter().map(|(_, p)| *p).collect();
            match scale_probabilities(&source, temperature) {
                Some(scaled) => {
                    let rescaled: Vec<(String, f64)> = probabilities
                        .iter()
                        .map(|(key, _)| key.clone())
                        .zip(scaled.iter().copied())
                        .collect();
                    let confidence = scaled.iter().copied().fold(0.0_f64, f64::max);
                    ScoringAnswer::Choice {
                        choice: choice.clone(),
                        probabilities: rescaled,
                        confidence,
                        calibration: CalibrationLevel::Temperature,
                    }
                }
                None => answer,
            }
        }
        ScoringAnswer::Score { probabilities, .. } => {
            match scale_probabilities(probabilities, temperature) {
                Some(scaled) => {
                    let score = weighted_expectation(&scaled);
                    let confidence = scaled.iter().copied().fold(0.0_f64, f64::max);
                    ScoringAnswer::Score {
                        score,
                        probabilities: scaled,
                        confidence,
                        calibration: CalibrationLevel::Temperature,
                    }
                }
                None => answer,
            }
        }
    }
}

/// 分档期望值：第 i 档取值 i/(n-1)，score = Σ p_i × value_i。
fn weighted_expectation(probabilities: &[f64]) -> f64 {
    let count = probabilities.len();
    if count < 2 {
        return probabilities.first().copied().unwrap_or(0.0);
    }
    probabilities
        .iter()
        .enumerate()
        .map(|(index, probability)| probability * index as f64 / (count - 1) as f64)
        .sum()
}

#[cfg(test)]
#[path = "calibrated_tests.rs"]
mod tests;
