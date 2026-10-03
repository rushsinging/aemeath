//! 三题型与校准级别：System One 评分服务的领域语言。
//!
//! 语义与 Jev `/v1/systemone` 线格式一一对应（线格式序列化属 adapter 职责）。
//! 设计依据：`docs/design/02-modules/systemone/01-systemone-scoring.md` §2。

use serde::{Deserialize, Serialize};

/// 评分上下文：prose 文本。
///
/// **NEVER 传 JSON 原文**——评分头按散文训练，结构化输入先渲染为 prose。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoringState(String);

impl ScoringState {
    /// 构造评分上下文；`text` 为纯空白时返回 `None`。
    pub fn new(text: impl Into<String>) -> Option<Self> {
        let text = text.into();
        if text.trim().is_empty() {
            return None;
        }
        Some(Self(text))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

/// Noul 题型的 true/false 语义描述。
///
/// 两句都 **MUST** 是完整句子，NEVER 用短标签——实测证据：短标签使评分头输出退化。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoulCriteria {
    when_true: String,
    when_false: String,
}

impl NoulCriteria {
    /// 构造语义描述；任一句为纯空白时返回 `None`。
    pub fn new(when_true: impl Into<String>, when_false: impl Into<String>) -> Option<Self> {
        let when_true = when_true.into();
        let when_false = when_false.into();
        if when_true.trim().is_empty() || when_false.trim().is_empty() {
            return None;
        }
        Some(Self {
            when_true,
            when_false,
        })
    }

    pub fn when_true(&self) -> &str {
        &self.when_true
    }

    pub fn when_false(&self) -> &str {
        &self.when_false
    }
}

/// 评分问题：三题型。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ScoringQuestion {
    /// 是非判定：命题为真的概率。
    Noul {
        instructions: String,
        criteria: Option<NoulCriteria>,
    },
    /// 选项抉择：候选 key → 完整描述句，有序。
    Choice {
        instructions: String,
        criteria: Vec<(String, String)>,
    },
    /// 有序分档：等级描述升序排列。
    Score {
        instructions: String,
        levels: Vec<String>,
    },
}

/// 题型构造拒绝原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestionRejected {
    BlankInstructions,
    TooFewCriteria,
    TooManyCriteria,
    BlankCriterionKey,
    BlankCriterionDescription,
    DuplicateCriterionKey,
    BlankLevel,
}

impl ScoringQuestion {
    /// Choice 候选数合法区间（线格式上限 255）。
    pub const CHOICE_CRITERIA_MAX: usize = 255;

    pub fn noul(instructions: impl Into<String>, criteria: Option<NoulCriteria>) -> Option<Self> {
        let instructions = instructions.into();
        if instructions.trim().is_empty() {
            return None;
        }
        Some(Self::Noul {
            instructions,
            criteria,
        })
    }

    pub fn choice(
        instructions: impl Into<String>,
        criteria: Vec<(String, String)>,
    ) -> Result<Self, QuestionRejected> {
        let instructions = instructions.into();
        if instructions.trim().is_empty() {
            return Err(QuestionRejected::BlankInstructions);
        }
        if criteria.len() < 2 {
            return Err(QuestionRejected::TooFewCriteria);
        }
        if criteria.len() > Self::CHOICE_CRITERIA_MAX {
            return Err(QuestionRejected::TooManyCriteria);
        }
        for (index, (key, description)) in criteria.iter().enumerate() {
            if key.trim().is_empty() {
                return Err(QuestionRejected::BlankCriterionKey);
            }
            if description.trim().is_empty() {
                return Err(QuestionRejected::BlankCriterionDescription);
            }
            if criteria[..index].iter().any(|(seen, _)| seen == key) {
                return Err(QuestionRejected::DuplicateCriterionKey);
            }
        }
        Ok(Self::Choice {
            instructions,
            criteria,
        })
    }

    pub fn score(
        instructions: impl Into<String>,
        levels: Vec<String>,
    ) -> Result<Self, QuestionRejected> {
        let instructions = instructions.into();
        if instructions.trim().is_empty() {
            return Err(QuestionRejected::BlankInstructions);
        }
        if levels.len() < 2 {
            return Err(QuestionRejected::TooFewCriteria);
        }
        if levels.iter().any(|level| level.trim().is_empty()) {
            return Err(QuestionRejected::BlankLevel);
        }
        Ok(Self::Score {
            instructions,
            levels,
        })
    }
}

/// 评分答案：按题型承载概率分布与实际使用的校准级别（审计与降级判定用）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ScoringAnswer {
    Noul {
        p_true: f64,
        calibration: CalibrationLevel,
    },
    Choice {
        choice: String,
        probabilities: Vec<(String, f64)>,
        confidence: f64,
        calibration: CalibrationLevel,
    },
    Score {
        score: f64,
        probabilities: Vec<f64>,
        confidence: f64,
        calibration: CalibrationLevel,
    },
}

/// 答案构造拒绝原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerRejected {
    ProbabilityOutOfRange,
    EmptyProbabilities,
    ChoiceNotInProbabilities,
}

impl ScoringAnswer {
    pub fn noul(p_true: f64, calibration: CalibrationLevel) -> Result<Self, AnswerRejected> {
        if !is_valid_probability(p_true) {
            return Err(AnswerRejected::ProbabilityOutOfRange);
        }
        Ok(Self::Noul {
            p_true,
            calibration,
        })
    }

    pub fn choice(
        choice: impl Into<String>,
        probabilities: Vec<(String, f64)>,
        confidence: f64,
        calibration: CalibrationLevel,
    ) -> Result<Self, AnswerRejected> {
        let choice = choice.into();
        if probabilities.is_empty() {
            return Err(AnswerRejected::EmptyProbabilities);
        }
        if probabilities
            .iter()
            .any(|(_, probability)| !is_valid_probability(*probability))
            || !is_valid_probability(confidence)
        {
            return Err(AnswerRejected::ProbabilityOutOfRange);
        }
        if !probabilities.iter().any(|(key, _)| *key == choice) {
            return Err(AnswerRejected::ChoiceNotInProbabilities);
        }
        Ok(Self::Choice {
            choice,
            probabilities,
            confidence,
            calibration,
        })
    }

    pub fn score(
        score: f64,
        probabilities: Vec<f64>,
        confidence: f64,
        calibration: CalibrationLevel,
    ) -> Result<Self, AnswerRejected> {
        if probabilities.is_empty() {
            return Err(AnswerRejected::EmptyProbabilities);
        }
        if probabilities
            .iter()
            .any(|probability| !is_valid_probability(*probability))
            || !is_valid_probability(score)
            || !is_valid_probability(confidence)
        {
            return Err(AnswerRejected::ProbabilityOutOfRange);
        }
        Ok(Self::Score {
            score,
            probabilities,
            confidence,
            calibration,
        })
    }

    /// 答案实际使用的校准级别。
    pub fn calibration(&self) -> CalibrationLevel {
        match self {
            ScoringAnswer::Noul { calibration, .. }
            | ScoringAnswer::Choice { calibration, .. }
            | ScoringAnswer::Score { calibration, .. } => *calibration,
        }
    }
}

fn is_valid_probability(value: f64) -> bool {
    (0.0..=1.0).contains(&value)
}

/// 校准级别（引擎无关）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CalibrationLevel {
    /// 引擎原始输出。
    Raw,
    /// 出厂/在线拟合的全局温度缩放。
    Temperature,
    /// 闭式校准头（后置演进，port 语义预留）。
    Head,
}

#[cfg(test)]
#[path = "published_language_tests.rs"]
mod tests;
