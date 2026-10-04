//! Jev `/v1/systemone` 线格式：请求序列化与响应解析（纯函数，无 IO）。
//!
//! 保序约束：Choice criteria 的 key 顺序 = 候选展示顺序（影响引擎顺序敏感性），
//! `serde_json::Map` 不保插入序，因此请求序列化与响应 probabilities 解析
//! 均手写 serde Map access 按文档顺序读写。

use serde::ser::{SerializeMap, SerializeStruct};
use serde::{Serialize, Serializer};

use crate::domain::{AnswerRejected, ScoringAnswer, ScoringQuestion, ScoringState};

/// 线格式请求体。
pub struct WireRequest<'a> {
    pub state: &'a ScoringState,
    pub model: &'a str,
    /// (question_id, question) 按提交顺序。
    pub questions: &'a [(String, ScoringQuestion)],
}

impl Serialize for WireRequest<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut root = serializer.serialize_struct("WireRequest", 3)?;
        root.serialize_field("state", &self.state.as_str())?;
        root.serialize_field("model", self.model)?;
        root.serialize_field("questions", &WireQuestions(self.questions))?;
        root.end()
    }
}

struct WireQuestions<'a>(&'a [(String, ScoringQuestion)]);

impl Serialize for WireQuestions<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (id, question) in self.0 {
            map.serialize_entry(id, &WireQuestion(question))?;
        }
        map.end()
    }
}

struct WireQuestion<'a>(&'a ScoringQuestion);

impl Serialize for WireQuestion<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            ScoringQuestion::Noul {
                instructions,
                criteria,
            } => {
                let field_count = if criteria.is_some() { 3 } else { 2 };
                let mut question = serializer.serialize_struct("NoulQuestion", field_count)?;
                question.serialize_field("type", "noul")?;
                question.serialize_field("instructions", instructions)?;
                if let Some(criteria) = criteria {
                    question.serialize_field(
                        "criteria",
                        &NoulWireCriteria {
                            when_true: criteria.when_true(),
                            when_false: criteria.when_false(),
                        },
                    )?;
                }
                question.end()
            }
            ScoringQuestion::Choice {
                instructions,
                criteria,
            } => {
                let mut question = serializer.serialize_struct("ChoiceQuestion", 3)?;
                question.serialize_field("type", "choice")?;
                question.serialize_field("instructions", instructions)?;
                question.serialize_field("criteria", &ChoiceWireCriteria(criteria))?;
                question.end()
            }
            ScoringQuestion::Score {
                instructions,
                levels,
            } => {
                let mut question = serializer.serialize_struct("ScoreQuestion", 3)?;
                question.serialize_field("type", "score")?;
                question.serialize_field("instructions", instructions)?;
                question.serialize_field("criteria", levels)?;
                question.end()
            }
        }
    }
}

struct NoulWireCriteria<'a> {
    when_true: &'a str,
    when_false: &'a str,
}

impl Serialize for NoulWireCriteria<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("true", self.when_true)?;
        map.serialize_entry("false", self.when_false)?;
        map.end()
    }
}

struct ChoiceWireCriteria<'a>(&'a [(String, String)]);

impl Serialize for ChoiceWireCriteria<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, description) in self.0 {
            map.serialize_entry(key, description)?;
        }
        map.end()
    }
}

/// 线格式解析拒绝原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireRejected {
    /// 响应不是合法 JSON 或缺 `answers` 对象。
    MalformedBody,
    /// 请求过的 question id 在响应中缺失。
    MissingAnswer(String),
    /// 答案字段与题型不匹配或值非法。
    InvalidAnswer(String),
}

impl std::fmt::Display for WireRejected {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WireRejected::MalformedBody => write!(formatter, "响应体非法（非 JSON 或缺 answers）"),
            WireRejected::MissingAnswer(id) => write!(formatter, "响应缺失 question id：{id}"),
            WireRejected::InvalidAnswer(detail) => write!(formatter, "答案字段非法：{detail}"),
        }
    }
}

/// 按请求顺序解析响应 answers。
///
/// `requests` 为 (question_id, question)，用于按题型校验答案字段并保序返回。
pub fn parse_answers(
    body: &str,
    requests: &[(String, ScoringQuestion)],
) -> Result<Vec<ScoringAnswer>, WireRejected> {
    let document: serde_json::Value =
        serde_json::from_str(body).map_err(|_| WireRejected::MalformedBody)?;
    let answers = document
        .get("answers")
        .and_then(|value| value.as_object())
        .ok_or(WireRejected::MalformedBody)?;
    requests
        .iter()
        .map(|(id, question)| {
            let answer = answers
                .get(id)
                .ok_or_else(|| WireRejected::MissingAnswer(id.clone()))?;
            parse_answer(question, answer)
        })
        .collect()
}

fn parse_answer(
    question: &ScoringQuestion,
    answer: &serde_json::Value,
) -> Result<ScoringAnswer, WireRejected> {
    match question {
        ScoringQuestion::Noul { .. } => {
            let p_true = answer
                .get("noul")
                .and_then(|value| value.as_f64())
                .ok_or_else(|| WireRejected::InvalidAnswer("noul 缺 p_true".to_owned()))?;
            ScoringAnswer::noul(p_true, crate::domain::CalibrationLevel::Raw)
                .map_err(|error| WireRejected::InvalidAnswer(answer_rejected_detail(error)))
        }
        ScoringQuestion::Choice { .. } => {
            let choice = answer
                .get("choice")
                .and_then(|value| value.as_str())
                .ok_or_else(|| WireRejected::InvalidAnswer("choice 缺选项 key".to_owned()))?;
            let probabilities = parse_ordered_probabilities(answer)?;
            let confidence = optional_confidence(answer).unwrap_or_else(|| {
                probabilities
                    .iter()
                    .map(|(_, probability)| *probability)
                    .fold(0.0_f64, f64::max)
            });
            ScoringAnswer::choice(
                choice,
                probabilities,
                confidence,
                crate::domain::CalibrationLevel::Raw,
            )
            .map_err(|error| WireRejected::InvalidAnswer(answer_rejected_detail(error)))
        }
        ScoringQuestion::Score { levels, .. } => {
            let score = answer
                .get("score")
                .and_then(|value| value.as_f64())
                .ok_or_else(|| WireRejected::InvalidAnswer("score 缺分数".to_owned()))?;
            // kev 实测：score 的 probabilities 为序号 keyed dict（{"0": p0, ...}）；
            // 兼容数组形态（其他引擎）。
            let probabilities: Vec<f64> = match answer.get("probabilities") {
                Some(serde_json::Value::Array(items)) => items
                    .iter()
                    .map(|value| {
                        value
                            .as_f64()
                            .ok_or_else(|| WireRejected::InvalidAnswer("概率非数值".to_owned()))
                    })
                    .collect::<Result<_, _>>()?,
                Some(serde_json::Value::Object(entries)) => {
                    let mut indexed: Vec<(usize, f64)> = entries
                        .iter()
                        .map(|(key, value)| {
                            let index = key.parse::<usize>().map_err(|_| {
                                WireRejected::InvalidAnswer(format!("概率 key 非序号：{key}"))
                            })?;
                            let probability = value.as_f64().ok_or_else(|| {
                                WireRejected::InvalidAnswer("概率非数值".to_owned())
                            })?;
                            Ok((index, probability))
                        })
                        .collect::<Result<_, WireRejected>>()?;
                    indexed.sort_by_key(|(index, _)| *index);
                    indexed
                        .iter()
                        .enumerate()
                        .all(|(position, (index, _))| position == *index)
                        .then(|| indexed.iter().map(|(_, p)| *p).collect())
                        .ok_or_else(|| WireRejected::InvalidAnswer("概率序号不连续".to_owned()))?
                }
                _ => {
                    return Err(WireRejected::InvalidAnswer(
                        "score 缺 probabilities".to_owned(),
                    ))
                }
            };
            if probabilities.len() != levels.len() {
                return Err(WireRejected::InvalidAnswer(format!(
                    "score probabilities 数量 {} 与 levels 数量 {} 不一致",
                    probabilities.len(),
                    levels.len()
                )));
            }
            let confidence = optional_confidence(answer)
                .unwrap_or_else(|| probabilities.iter().copied().fold(0.0_f64, f64::max));
            ScoringAnswer::score(
                score,
                probabilities,
                confidence,
                crate::domain::CalibrationLevel::Raw,
            )
            .map_err(|error| WireRejected::InvalidAnswer(answer_rejected_detail(error)))
        }
    }
}

/// 解析 choice probabilities 对象。
///
/// 顺序说明：`serde_json::Value` object 为字母序，解析后 `Vec` 顺序非线格式文档序。
/// 当前消费语义（按概率值取序 / 按 key 查找 / 审计字段完整性）均不依赖文档序，
/// 故接受该折中；若未来需要文档序（如审计严格回放），须改流式 Visitor 解析。
fn parse_ordered_probabilities(
    answer: &serde_json::Value,
) -> Result<Vec<(String, f64)>, WireRejected> {
    let entries = answer
        .get("probabilities")
        .and_then(|value| value.as_object())
        .ok_or_else(|| WireRejected::InvalidAnswer("choice 缺 probabilities".to_owned()))?;
    entries
        .iter()
        .map(|(key, value)| {
            value
                .as_f64()
                .map(|probability| (key.clone(), probability))
                .ok_or_else(|| WireRejected::InvalidAnswer("概率非数值".to_owned()))
        })
        .collect()
}

fn optional_confidence(answer: &serde_json::Value) -> Option<f64> {
    answer.get("confidence").and_then(|value| value.as_f64())
}

fn answer_rejected_detail(error: AnswerRejected) -> String {
    format!("答案不变量校验失败：{error:?}")
}

#[cfg(test)]
#[path = "jev_wire_tests.rs"]
mod tests;
