//! 百炼 TypeSafe System One 协议 wire：请求/响应 DTO 与领域语言互转。
//!
//! 协议事实（2026-10-26 实测 + 官方文档，设计 #2095）：
//! - `POST {endpoint}`，body `{model, state, questions: {qid: {type, instructions, criteria}}}`；
//! - 响应 `{model, request_id, answers: {qid: {...}}, usage, latency_ms}`；
//! - `choice.probabilities` 为候选集内归一化（和≈1，含 0 项）；
//! - `score.score` 为档位概率加权期望（非整数、不归一化）；
//! - 响应不保证顺序，按 qid 取回；qid 由本客户端按请求顺序生成（q0..qN）。
//!
//! 领域转换语义：
//! - 云端无温度校准，`calibration` 恒 [`CalibrationLevel::Raw`]（引擎原始输出）；
//! - `probabilities` 按**请求** criteria/档位顺序还原，响应缺失项补 0——
//!   确定性输出不依赖 JSON map 迭代序。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::domain::published_language::{
    CalibrationLevel, ScoringAnswer, ScoringQuestion, ScoringState,
};

/// wire 转换失败原因。
#[derive(Debug, Clone, PartialEq)]
pub enum SystemOneHttpWireError {
    /// 响应缺少请求中的 qid 答案。
    MissingAnswer { question_id: String },
    /// 响应题型与请求题型不一致。
    TypeMismatch { question_id: String },
    /// 答案数值非法（概率越界 / choice 不在候选内等，沿用领域拒绝原因）。
    InvalidAnswer { question_id: String, reason: String },
}

impl std::fmt::Display for SystemOneHttpWireError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingAnswer { question_id } => {
                write!(formatter, "响应缺少问题 {question_id} 的答案")
            }
            Self::TypeMismatch { question_id } => {
                write!(formatter, "问题 {question_id} 的响应题型与请求不一致")
            }
            Self::InvalidAnswer {
                question_id,
                reason,
            } => {
                write!(formatter, "问题 {question_id} 的答案数值非法：{reason}")
            }
        }
    }
}

impl std::error::Error for SystemOneHttpWireError {}

/// 请求体。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemOneHttpRequest {
    pub model: String,
    pub state: String,
    pub questions: BTreeMap<String, SystemOneQuestionWire>,
}

/// 单题 wire 形态。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SystemOneQuestionWire {
    Choice {
        instructions: String,
        criteria: BTreeMap<String, String>,
    },
    Noul {
        instructions: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        criteria: Option<SystemOneNoulCriteriaWire>,
    },
    Score {
        instructions: String,
        criteria: Vec<String>,
    },
}

/// noul 题型 true/false 语义描述。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemOneNoulCriteriaWire {
    #[serde(rename = "true")]
    pub when_true: String,
    #[serde(rename = "false")]
    pub when_false: String,
}

/// 响应体。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemOneHttpResponse {
    pub model: String,
    #[serde(default)]
    pub request_id: String,
    pub answers: BTreeMap<String, SystemOneAnswerWire>,
    #[serde(default = "default_zero_token_usage")]
    pub usage: SystemOneUsageWire,
    #[serde(default)]
    pub latency_ms: f64,
}

fn default_zero_token_usage() -> SystemOneUsageWire {
    SystemOneUsageWire { input_tokens: 0 }
}

/// token 用量。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemOneUsageWire {
    #[serde(default)]
    pub input_tokens: u64,
}

/// 单题答案 wire 形态。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SystemOneAnswerWire {
    Noul {
        noul: f64,
    },
    Choice {
        choice: String,
        confidence: f64,
        probabilities: BTreeMap<String, f64>,
    },
    Score {
        score: f64,
        confidence: f64,
        #[serde(default)]
        legend: BTreeMap<String, serde_json::Value>,
        probabilities: BTreeMap<String, f64>,
    },
}

/// 按请求顺序生成 qid（q0..qN）构造请求体。
pub fn build_request(
    model: &str,
    state: &ScoringState,
    questions: &[ScoringQuestion],
) -> SystemOneHttpRequest {
    let questions = questions
        .iter()
        .enumerate()
        .map(|(index, question)| (format!("q{index}"), to_question_wire(question)))
        .collect();
    SystemOneHttpRequest {
        model: model.to_string(),
        state: state.as_str().to_string(),
        questions,
    }
}

fn to_question_wire(question: &ScoringQuestion) -> SystemOneQuestionWire {
    match question {
        ScoringQuestion::Noul {
            instructions,
            criteria,
        } => SystemOneQuestionWire::Noul {
            instructions: instructions.clone(),
            criteria: criteria.as_ref().map(|noul| SystemOneNoulCriteriaWire {
                when_true: noul.when_true().to_string(),
                when_false: noul.when_false().to_string(),
            }),
        },
        ScoringQuestion::Choice {
            instructions,
            criteria,
        } => SystemOneQuestionWire::Choice {
            instructions: instructions.clone(),
            criteria: criteria
                .iter()
                .map(|(key, description)| (key.clone(), description.clone()))
                .collect(),
        },
        ScoringQuestion::Score {
            instructions,
            levels,
        } => SystemOneQuestionWire::Score {
            instructions: instructions.clone(),
            criteria: levels.clone(),
        },
    }
}

/// 按请求 qid 顺序解析答案为领域类型（缺失候选/档位补 0，calibration=Raw）。
pub fn parse_answers(
    response: &SystemOneHttpResponse,
    questions: &[ScoringQuestion],
) -> Result<Vec<ScoringAnswer>, SystemOneHttpWireError> {
    questions
        .iter()
        .enumerate()
        .map(|(index, question)| {
            let question_id = format!("q{index}");
            let answer_wire = response.answers.get(&question_id).ok_or_else(|| {
                SystemOneHttpWireError::MissingAnswer {
                    question_id: question_id.clone(),
                }
            })?;
            let parsed = match (question, answer_wire) {
                (ScoringQuestion::Noul { .. }, SystemOneAnswerWire::Noul { noul }) => {
                    ScoringAnswer::noul(*noul, CalibrationLevel::Raw)
                }
                (
                    ScoringQuestion::Choice { criteria, .. },
                    SystemOneAnswerWire::Choice {
                        choice,
                        confidence,
                        probabilities,
                    },
                ) => {
                    let restored = criteria
                        .iter()
                        .map(|(key, _)| {
                            (key.clone(), probabilities.get(key).copied().unwrap_or(0.0))
                        })
                        .collect::<Vec<_>>();
                    ScoringAnswer::choice(
                        choice.clone(),
                        restored,
                        *confidence,
                        CalibrationLevel::Raw,
                    )
                }
                (
                    ScoringQuestion::Score { levels, .. },
                    SystemOneAnswerWire::Score {
                        score,
                        confidence,
                        probabilities,
                        ..
                    },
                ) => {
                    let restored = (0..levels.len())
                        .map(|level| {
                            probabilities
                                .get(&level.to_string())
                                .copied()
                                .unwrap_or(0.0)
                        })
                        .collect::<Vec<_>>();
                    ScoringAnswer::score(*score, restored, *confidence, CalibrationLevel::Raw)
                }
                _ => {
                    return Err(SystemOneHttpWireError::TypeMismatch { question_id });
                }
            };
            parsed.map_err(|rejected| SystemOneHttpWireError::InvalidAnswer {
                question_id,
                reason: format!("{rejected:?}"),
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "systemone_http_wire_tests.rs"]
mod tests;

// ── server 方向转换（#2096：daemon HTTP handler 与 client 共用同一 DTO 层）──

/// 解析请求为领域输入：qid 按 q0..qN 还原 Vec（与 `build_request` 互逆）。
pub fn parse_request(
    request: &SystemOneHttpRequest,
) -> Result<(ScoringState, Vec<ScoringQuestion>), SystemOneHttpWireError> {
    let state = ScoringState::new(request.state.clone()).ok_or_else(|| {
        SystemOneHttpWireError::InvalidAnswer {
            question_id: "<state>".into(),
            reason: "state 为空白".into(),
        }
    })?;
    let questions = (0..request.questions.len())
        .map(|index| {
            let question_id = format!("q{index}");
            let wire = request.questions.get(&question_id).ok_or_else(|| {
                SystemOneHttpWireError::MissingAnswer {
                    question_id: question_id.clone(),
                }
            })?;
            match wire {
                SystemOneQuestionWire::Noul {
                    instructions,
                    criteria,
                } => ScoringQuestion::noul(
                    instructions.clone(),
                    criteria.as_ref().map(|noul| {
                        crate::domain::published_language::NoulCriteria::new(
                            noul.when_true.clone(),
                            noul.when_false.clone(),
                        )
                        .expect("criteria 非空已在领域构造校验")
                    }),
                )
                .ok_or_else(|| SystemOneHttpWireError::InvalidAnswer {
                    question_id: question_id.clone(),
                    reason: "noul instructions 为空白".into(),
                }),
                SystemOneQuestionWire::Choice {
                    instructions,
                    criteria,
                } => {
                    let criteria = criteria
                        .iter()
                        .map(|(key, description)| (key.clone(), description.clone()))
                        .collect::<Vec<_>>();
                    ScoringQuestion::choice(instructions.clone(), criteria).map_err(|rejected| {
                        SystemOneHttpWireError::InvalidAnswer {
                            question_id: question_id.clone(),
                            reason: format!("{rejected:?}"),
                        }
                    })
                }
                SystemOneQuestionWire::Score {
                    instructions,
                    criteria,
                } => ScoringQuestion::score(instructions.clone(), criteria.clone()).map_err(
                    |rejected| SystemOneHttpWireError::InvalidAnswer {
                        question_id: question_id.clone(),
                        reason: format!("{rejected:?}"),
                    },
                ),
            }
        })
        .collect::<Result<Vec<_>, SystemOneHttpWireError>>()?;
    Ok((state, questions))
}

/// 构造响应：按 q0..qN 把领域答案写回 qid map（与 `parse_answers` 互逆）。
pub fn build_response(
    model: &str,
    answers: &[ScoringAnswer],
    latency_ms: f64,
) -> SystemOneHttpResponse {
    let answers = answers
        .iter()
        .enumerate()
        .map(|(index, answer)| {
            let wire = match answer {
                ScoringAnswer::Noul { p_true, .. } => SystemOneAnswerWire::Noul { noul: *p_true },
                ScoringAnswer::Choice {
                    choice,
                    probabilities,
                    confidence,
                    ..
                } => SystemOneAnswerWire::Choice {
                    choice: choice.clone(),
                    confidence: *confidence,
                    probabilities: probabilities
                        .iter()
                        .map(|(key, probability)| (key.clone(), *probability))
                        .collect(),
                },
                ScoringAnswer::Score {
                    score,
                    probabilities,
                    confidence,
                    ..
                } => SystemOneAnswerWire::Score {
                    score: *score,
                    confidence: *confidence,
                    legend: Default::default(),
                    probabilities: probabilities
                        .iter()
                        .enumerate()
                        .map(|(level, probability)| (level.to_string(), *probability))
                        .collect(),
                },
            };
            (format!("q{index}"), wire)
        })
        .collect();
    SystemOneHttpResponse {
        model: model.to_string(),
        request_id: String::new(),
        answers,
        usage: SystemOneUsageWire { input_tokens: 0 },
        latency_ms,
    }
}
