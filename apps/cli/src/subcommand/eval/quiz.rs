//! `eval compact quiz`（P3 续接问答）：压缩后模型还能不能答出早期上下文里的事实。
//!
//! 评测边界（MUST 如实呈现）：
//! - 出题者与答题者是同一模型（`--model`，默认主模型口径）：先读 early 历史
//!   出题（此时有完整上下文），再只给 summary + tail 答题——测的是「压缩结果
//!   是否保住了答题所需信息」，不是模型本身的能力。
//! - 判卷者是独立模型（`--judge`，**评测时显式指定，无默认值**）；未指定时
//!   本命令拒绝运行（不静默降级为某个默认判官）。
//! - correct_rate 是「续接可用性」代理：题目可答性依赖出题质量，只做版本间
//!   对比与趋势，不做绝对质量结论。
//! - 结构化输出对齐反思口径：解析失败重试一次，仍失败该 case 标
//!   `parse_failed`，不计入正确率分母。

use std::path::PathBuf;

use serde::Serialize;

use super::constants::{ANSWER_SOURCE_TOKEN_BUDGET, QUIZ_QUESTION_COUNT, QUIZ_SOURCE_TOKEN_BUDGET};
use super::telemetry::write_or_print;

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub(crate) struct QuizItem {
    pub(crate) question: String,
    #[serde(default)]
    pub(crate) gold: Option<String>,
    #[serde(default)]
    pub(crate) answer: Option<String>,
    #[serde(default)]
    pub(crate) correct: Option<bool>,
    #[serde(default)]
    pub(crate) evidence: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CaseQuiz {
    case_id: String,
    status: String,
    questions_total: usize,
    answered: usize,
    correct: usize,
    correct_rate: Option<f64>,
    error: Option<String>,
    items: Vec<QuizItem>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct QuizReport {
    answerer: Option<String>,
    judge: Option<String>,
    cases_total: usize,
    evaluated: usize,
    parse_failed: usize,
    correct_rate_mean: Option<f64>,
    cases: Vec<CaseQuiz>,
}

/// 剥离 LLM 输出可能的 Markdown 代码围栏后解析 JSON 数组。
pub(crate) fn parse_json_items(raw: &str) -> Option<Vec<QuizItem>> {
    let trimmed = raw.trim();
    let body = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|body| body.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);
    serde_json::from_str(body).ok()
}

/// 按估算 token 截断文本（保守：宁可少给不可超窗）。
pub(crate) fn truncate_by_tokens(text: &str, budget_tokens: usize) -> String {
    let mut used = 0.0f64;
    let mut chars = 0usize;
    let budget = budget_tokens as f64;
    for char in text.chars() {
        let cost = if matches!(char as u32, 0x3000..=0x303F | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF)
        {
            0.6
        } else {
            0.3
        };
        if used + cost > budget {
            break;
        }
        used += cost;
        chars += 1;
    }
    text.chars().take(chars).collect()
}

fn message_texts(messages: &[serde_json::Value]) -> Vec<String> {
    messages.iter().map(super::dataset::message_text).collect()
}

fn questions_prompt(early_text: &str) -> String {
    format!(
        "你在为一次上下文压缩验收出题。下面是一段对话的早期历史（已被压缩为摘要）。\n\
         请出 {QUIZ_QUESTION_COUNT} 道只有读了这段历史才能答出的事实题（目标、决策、改动文件、\n\
         关键约束、未完成事项），每题附简短标准答案。\n\
         只输出一个 JSON 数组，元素形如 {{\"question\":\"...\",\"gold\":\"...\"}}。\n\
         不要 Markdown 围栏、不要解释。\n\n<early_history>\n{early_text}\n</early_history>"
    )
}

fn answers_prompt(source_text: &str, items: &[QuizItem]) -> String {
    let questions = items
        .iter()
        .map(|item| format!("- {}", item.question))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "以下是当前会话可见的上下文（压缩摘要 + 近期消息）。\n\
         请逐条回答下面的问题；答不出就答 \"UNKNOWN\"。\n\
         只输出一个 JSON 数组，元素形如 {{\"question\":\"...\",\"answer\":\"...\"}}，顺序与问题一致。\n\
         不要 Markdown 围栏、不要解释。\n\n<context>\n{source_text}\n</context>\n\n<questions>\n{questions}\n</questions>"
    )
}

fn verdict_prompt(items: &[QuizItem]) -> String {
    let pairs = items
        .iter()
        .map(|item| {
            format!(
                "- 问题：{}\n  标准答案：{}\n  作答：{}",
                item.question,
                item.gold.as_deref().unwrap_or("(缺失)"),
                item.answer.as_deref().unwrap_or("(未作答)")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "你是独立判卷人。逐条判定作答是否与标准答案一致（UNKNOWN/未作答=错误；\n\
         语义一致即正确，不要求字面相同）。\n\
         只输出一个 JSON 数组，元素形如 {{\"question\":\"...\",\"correct\":true,\"evidence\":\"引用作答中的关键片段\"}}。\n\
         不要 Markdown 围栏、不要解释。\n\n<items>\n{pairs}\n</items>"
    )
}

async fn complete_with_retry(
    selection: &str,
    system: &str,
    prompt: &str,
) -> Result<String, String> {
    let first = composition::app::eval_llm_complete(selection, system, prompt, 16_384).await?;
    if parse_json_items(&first).is_some() {
        return Ok(first);
    }
    // 解析失败重试一次（对齐反思口径：修复指令重发）。
    let retry_prompt =
        format!("{prompt}\n\n上一次输出不是纯 JSON 数组。请只输出 JSON 数组，不要任何其他文字。");
    composition::app::eval_llm_complete(selection, system, &retry_prompt, 16_384)
        .await
        .map_err(|error| format!("重试仍失败：{error}"))
}

pub(crate) async fn run(
    cases_dir: PathBuf,
    summaries_dir: PathBuf,
    model: String,
    judge: String,
    limit: usize,
    output: Option<PathBuf>,
) -> Result<(), String> {
    let index = super::run::load_cases_index(&cases_dir)?;
    let mut report = QuizReport {
        answerer: Some(model.clone()),
        judge: Some(judge.clone()),
        ..Default::default()
    };

    for case in index.iter().take(limit) {
        let case_id = case
            .get("case_id")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        report.cases_total += 1;
        // 与 run 的实际切分对齐：tail 条数来自 run_summary.json（压缩保留窗口）。
        let run_summary: Option<serde_json::Value> =
            std::fs::read_to_string(summaries_dir.join("run_summary.json"))
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok());
        let recent_messages = run_summary
            .as_ref()
            .and_then(|report| report.get("cases").and_then(|v| v.as_array()))
            .and_then(|cases| {
                cases.iter().find(|item| {
                    item.get("case_id").and_then(|v| v.as_str()) == Some(case_id.as_str())
                })
            })
            .and_then(|item| item.get("recent_messages"))
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);
        let summary_path = summaries_dir.join(format!("{case_id}.summary.txt"));
        let Ok(summary) = std::fs::read_to_string(&summary_path) else {
            report.parse_failed += 1;
            report.cases.push(CaseQuiz {
                case_id,
                status: "no_summary".to_string(),
                questions_total: 0,
                answered: 0,
                correct: 0,
                correct_rate: None,
                error: Some(format!("缺少 {}", summary_path.display())),
                items: Vec::new(),
            });
            continue;
        };
        let messages = load_case_messages(case, &cases_dir)?;

        match quiz_one(case, &messages, &summary, recent_messages, &model, &judge).await {
            Ok(item) => {
                report.evaluated += 1;
                report.cases.push(item);
            }
            Err(error) => {
                report.parse_failed += 1;
                report.cases.push(CaseQuiz {
                    case_id,
                    status: "failed".to_string(),
                    questions_total: 0,
                    answered: 0,
                    correct: 0,
                    correct_rate: None,
                    error: Some(error),
                    items: Vec::new(),
                });
            }
        }
    }

    let rates: Vec<f64> = report
        .cases
        .iter()
        .filter_map(|item| item.correct_rate)
        .collect();
    report.correct_rate_mean = if rates.is_empty() {
        None
    } else {
        Some(rates.iter().sum::<f64>() / rates.len() as f64)
    };
    let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    write_or_print(output.as_deref(), &json)
}

fn load_case_messages(
    case: &serde_json::Value,
    cases_dir: &std::path::Path,
) -> Result<Vec<serde_json::Value>, String> {
    let path = case
        .get("messages_path")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "case 缺少 messages_path".to_string())?;
    super::run::load_case_json_lines(&cases_dir.join(path))
}

async fn quiz_one(
    case: &serde_json::Value,
    messages: &[serde_json::Value],
    summary: &str,
    recent_messages: Option<usize>,
    model: &str,
    judge: &str,
) -> Result<CaseQuiz, String> {
    // 与 run 的切分对齐：run 没有记录 recent（旧输出）时按 10% 条数兜底。
    let recent = recent_messages.unwrap_or(messages.len() / 10);
    let compacted = messages.len().saturating_sub(recent);
    let early = &messages[..compacted]; // allow unsafe_text_op: Vec slice
    let tail = &messages[compacted..]; // allow unsafe_text_op: Vec slice

    // 出题（答题模型读 early 历史）
    let early_text = truncate_by_tokens(&message_texts(early).join("\n"), QUIZ_SOURCE_TOKEN_BUDGET);
    let raw =
        complete_with_retry(model, "你是严格的出题人。", &questions_prompt(&early_text)).await?;
    let Some(questions) = parse_json_items(&raw) else {
        return Err("出题输出解析失败".to_string());
    };
    let questions: Vec<QuizItem> = questions
        .into_iter()
        .filter(|item| {
            !item.question.trim().is_empty()
                && item.gold.as_deref().is_some_and(|g| !g.trim().is_empty())
        })
        .take(QUIZ_QUESTION_COUNT)
        .collect();
    if questions.is_empty() {
        return Err("出题结果为空".to_string());
    }

    // 答题（同一模型只看 summary + tail）
    let source = format!(
        "<summary>\n{summary}\n</summary>\n<recent>\n{}\n</recent>",
        truncate_by_tokens(
            &message_texts(tail).join("\n"),
            ANSWER_SOURCE_TOKEN_BUDGET / 2
        )
    );
    let raw =
        complete_with_retry(model, "你是答题人。", &answers_prompt(&source, &questions)).await?;
    let Some(answers) = parse_json_items(&raw) else {
        return Err("答题输出解析失败".to_string());
    };
    let mut items = questions;
    for (item, answer) in items.iter_mut().zip(answers) {
        item.answer = answer.answer;
    }

    // 判卷（独立 judge）
    let raw = complete_with_retry(judge, "你是独立判卷人。", &verdict_prompt(&items)).await?;
    let Some(verdicts) = parse_json_items(&raw) else {
        return Err("判卷输出解析失败".to_string());
    };
    let mut correct = 0usize;
    for (item, verdict) in items.iter_mut().zip(verdicts) {
        item.correct = verdict.correct;
        item.evidence = verdict.evidence;
        if item.correct == Some(true) {
            correct += 1;
        }
    }
    let answered = items.iter().filter(|item| item.answer.is_some()).count();
    let case_id = case
        .get("case_id")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    Ok(CaseQuiz {
        case_id: case_id.to_string(),
        status: "evaluated".to_string(),
        questions_total: items.len(),
        answered,
        correct,
        correct_rate: Some(correct as f64 / items.len() as f64),
        error: None,
        items,
    })
}

#[cfg(test)]
#[path = "quiz_tests.rs"]
mod quiz_tests;
