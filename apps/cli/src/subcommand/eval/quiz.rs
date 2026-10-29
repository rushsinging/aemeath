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

use super::constants::{
    ANSWER_SOURCE_TOKEN_BUDGET, QUIZ_MAX_QUESTIONS, QUIZ_MIN_QUESTIONS, QUIZ_SOURCE_TOKEN_BUDGET,
    QUIZ_TOKENS_PER_QUESTION,
};
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
    /// 事实来源段：early（最老，经历最多轮摘要）/ mid / recent（临近压缩点）。
    #[serde(default)]
    pub(crate) epoch: Option<String>,
    /// 事实类型：decision / constraint / objective / tradeoff / root_cause。
    #[serde(default)]
    pub(crate) kind: Option<String>,
}

/// 一次 quiz 各阶段的 token 用量（#2133：开思考的成本经统计呈现）。
#[derive(Debug, Default, Clone, Serialize)]
pub(crate) struct QuizUsage {
    pub(crate) stage: String,
    pub(crate) input_tokens: Option<u32>,
    pub(crate) output_tokens: Option<u32>,
    pub(crate) reasoning_tokens: Option<u32>,
}

/// 按 epoch 分层的续接正确率：验证「多次压缩后早期决断的记忆衰减」。
#[derive(Debug, Default, Clone, Serialize)]
pub(crate) struct EpochStat {
    pub(crate) epoch: String,
    pub(crate) questions: usize,
    pub(crate) correct: usize,
    pub(crate) correct_rate: Option<f64>,
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
    per_epoch: Vec<EpochStat>,
}

/// 题目集（generate 产物 / answer 输入）：题库固定后多档位才可比。
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub(crate) struct QuestionSet {
    pub(crate) case_id: String,
    pub(crate) items: Vec<QuizItem>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct QuizReport {
    answerer: Option<String>,
    judge: Option<String>,
    cases_total: usize,
    evaluated: usize,
    parse_failed: usize,
    correct_rate_mean: Option<f64>,
    /// 跨 case 按 epoch 合并：early 段正确率显著低于 recent 即记忆衰减信号。
    per_epoch: Vec<EpochStat>,
    /// 各阶段 token 用量（出题/答题/判卷分开，成本可见）。
    usage: Vec<QuizUsage>,
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

/// 题数按 early 规模浮动（每 `QUIZ_TOKENS_PER_QUESTION` 一题，clamp 到上下限）。
pub(crate) fn question_count_for(early_tokens: u64) -> usize {
    let by_size = (early_tokens / QUIZ_TOKENS_PER_QUESTION as u64) as usize;
    by_size.clamp(QUIZ_MIN_QUESTIONS, QUIZ_MAX_QUESTIONS)
}

fn questions_prompt(early_text: &str, question_count: usize) -> String {
    let per_epoch = question_count.div_ceil(3).max(1);
    format!(
        "你在为一次上下文压缩验收出题。下面是按时间顺序排列的早期历史（已被压缩为摘要）。\n\
         把它按顺序均分为三段：early（最老）、mid、recent（临近压缩点），每段出 {per_epoch} 道题。\n\
         \n\
         【事实类型 kind 与每段配额】\n\
         - constraint（硬约束，≥3 道）：继续工作时必须遵守/禁止做的事\n\
         - objective（目标与验收标准，≥2 道）\n\
         - tradeoff（被否决的方案及原因，≥2 道）：防止后续重走弯路\n\
         - root_cause（根因结论，≥1 道）\n\
         - state（当前状态，≥2 道）：做到哪一步、还有哪些未完成、下一步是什么\n\
         - decision（拍板方案，≤{decision_cap} 道）\n\
         \n\
         【禁止出题】\n\
         - 环境读取类：文件路径/行号、目录名、worktree/分支名、commit sha、\n\
           PR/issue 编号、命令输出、日志原文、临时 id、函数签名、代码块内容；\n\
         - 流程性治理细节：issue 如何处置、处置模板、分组/分轨计划、合入节奏、\n\
           登记表比例统计、review 轮次安排——这类是过程记录，不是继续工作所需事实。\n\
         答案必须来自对话中用户或决策明确陈述过的语义结论，不能来自工具输出。\n\
         \n\
         每题标注来源段与 kind，并附简短标准答案。\n\
         只输出一个 JSON 数组，元素形如\n\
         {{\"epoch\":\"early|mid|recent\",\"kind\":\"constraint|objective|tradeoff|root_cause|state|decision\",\"question\":\"...\",\"gold\":\"...\"}}。\n\
         不要 Markdown 围栏、不要解释。\n\n<early_history>\n{early_text}\n</early_history>",
        decision_cap = per_epoch / 2,
    )
}

/// 跨 case 合并 per-epoch 统计。
fn merge_epoch_stats(cases: &[CaseQuiz]) -> Vec<EpochStat> {
    let order = ["early", "mid", "recent"];
    let mut buckets: std::collections::BTreeMap<String, (usize, usize)> =
        std::collections::BTreeMap::new();
    for case in cases {
        for item in &case.items {
            let epoch = item.epoch.clone().unwrap_or_else(|| "unknown".to_string());
            let entry = buckets.entry(epoch).or_default();
            entry.0 += 1;
            if item.correct == Some(true) {
                entry.1 += 1;
            }
        }
    }
    let mut stats: Vec<EpochStat> = buckets
        .into_iter()
        .map(|(epoch, (questions, correct))| EpochStat {
            epoch: epoch.clone(),
            questions,
            correct,
            correct_rate: if questions == 0 {
                None
            } else {
                Some(correct as f64 / questions as f64)
            },
        })
        .collect();
    stats.sort_by_key(|stat| {
        order
            .iter()
            .position(|name| *name == stat.epoch)
            .unwrap_or(order.len())
    });
    stats
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

#[allow(clippy::too_many_arguments)]
async fn complete_with_retry(
    selection: &str,
    system: &str,
    prompt: &str,
    reasoning: &str,
    usage_sink: &mut Vec<QuizUsage>,
    stage: &str,
) -> Result<String, String> {
    let record = |outcome: &composition::app::EvalCompleteOutcome| QuizUsage {
        stage: stage.to_string(),
        input_tokens: outcome.usage.input_tokens,
        output_tokens: outcome.usage.output_tokens,
        reasoning_tokens: outcome.usage.reasoning_tokens,
    };
    let first = composition::app::eval_llm_complete(selection, system, prompt, 16_384, reasoning)
        .await
        .map_err(|error| error.to_string())?;
    usage_sink.push(record(&first));
    if parse_json_items(&first.text).is_some() {
        return Ok(first.text);
    }
    // 解析失败重试一次（对齐反思口径：修复指令重发）。
    let retry_prompt =
        format!("{prompt}\n\n上一次输出不是纯 JSON 数组。请只输出 JSON 数组，不要任何其他文字。");
    let retry =
        composition::app::eval_llm_complete(selection, system, &retry_prompt, 16_384, reasoning)
            .await
            .map_err(|error| format!("重试仍失败：{error}"))?;
    usage_sink.push(record(&retry));
    Ok(retry.text)
}

/// 出题：把 early 历史题目集落盘（`QuestionSet` 数组），供多档位复用。
pub(crate) async fn generate(
    cases_dir: PathBuf,
    model: String,
    reasoning: String,
    limit: usize,
    output: PathBuf,
) -> Result<(), String> {
    let index = super::run::load_cases_index(&cases_dir)?;
    let mut sets: Vec<QuestionSet> = Vec::new();
    let mut usage: Vec<QuizUsage> = Vec::new();

    for case in index.iter().take(limit) {
        let case_id = case
            .get("case_id")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        let messages = load_case_messages(case, &cases_dir)?;
        // 出题只依赖 early（压缩点之前）：优先 case.json 的 compacted_message_count。
        let compacted = case
            .get("compacted_message_count")
            .and_then(|value| value.as_u64())
            .map(|value| value as usize)
            .unwrap_or_else(|| messages.len().saturating_sub(messages.len() / 10));
        let early = &messages[..compacted.min(messages.len())]; // allow unsafe_text_op: Vec slice
        let early_joined = message_texts(early).join("\n");
        let early_tokens = crate::subcommand::eval::dataset::estimate_tokens(&early_joined);
        let question_count = question_count_for(early_tokens);
        let early_text = truncate_by_tokens(&early_joined, QUIZ_SOURCE_TOKEN_BUDGET);
        let raw = complete_with_retry(
            &model,
            "你是严格的出题人。",
            &questions_prompt(&early_text, question_count),
            &reasoning,
            &mut usage,
            "quiz",
        )
        .await?;
        let Some(questions) = parse_json_items(&raw) else {
            return Err(format!("{case_id}: 出题输出解析失败"));
        };
        let mut items: Vec<QuizItem> = questions
            .into_iter()
            .filter(|item| {
                !item.question.trim().is_empty()
                    && item.gold.as_deref().is_some_and(|g| !g.trim().is_empty())
            })
            .map(|mut item| {
                if item.epoch.is_none() {
                    item.epoch = Some("unknown".to_string());
                }
                item
            })
            .collect();
        // 近重复去重：同义题占分母会稀释正确率（字符 bigram Jaccard >= 0.8）。
        let mut deduped: Vec<QuizItem> = Vec::new();
        for item in items.drain(..) {
            let duplicated = deduped
                .iter()
                .any(|kept| super::feedback::jaccard(&kept.question, &item.question) >= 0.8);
            if !duplicated {
                deduped.push(item);
            }
        }
        let items: Vec<QuizItem> = deduped.into_iter().take(question_count).collect();
        if items.is_empty() {
            return Err(format!("{case_id}: 出题结果为空"));
        }
        println!("[quiz:generate] {case_id} questions={}", items.len());
        sets.push(QuestionSet { case_id, items });
    }

    let json = serde_json::to_string_pretty(&sets).map_err(|error| error.to_string())?;
    write_or_print(Some(&output), &json)?;
    println!(
        "generated {} question set(s) → {} (usage stages: {})",
        sets.len(),
        output.display(),
        usage.len()
    );
    Ok(())
}

/// 答题 + 判卷：读固定题库，档位只影响答题/判卷（出题已固定）。
#[allow(clippy::too_many_arguments)]
pub(crate) async fn answer(
    cases_dir: PathBuf,
    questions: PathBuf,
    summaries_dir: PathBuf,
    model: String,
    judge: String,
    reasoning: String,
    judge_reasoning: Option<String>,
    limit: usize,
    output: Option<PathBuf>,
) -> Result<(), String> {
    let judge_reasoning = judge_reasoning.unwrap_or_else(|| reasoning.clone());
    let text = std::fs::read_to_string(&questions)
        .map_err(|error| format!("读取题库失败 {}: {error}", questions.display()))?;
    let sets: Vec<QuestionSet> =
        serde_json::from_str(&text).map_err(|error| format!("解析题库失败: {error}"))?;
    let mut report = QuizReport {
        answerer: Some(model.clone()),
        judge: Some(judge.clone()),
        ..Default::default()
    };

    for set in sets.iter().take(limit) {
        report.cases_total += 1;
        let case_id = set.case_id.clone();
        let case = super::run::load_cases_index(&cases_dir)?
            .into_iter()
            .find(|item| item.get("case_id").and_then(|v| v.as_str()) == Some(case_id.as_str()))
            .ok_or_else(|| format!("case 索引中找不到 {case_id}"))?;
        let Ok(summary) =
            std::fs::read_to_string(summaries_dir.join(format!("{case_id}.summary.txt")))
        else {
            report.parse_failed += 1;
            report.cases.push(CaseQuiz {
                case_id,
                status: "no_summary".to_string(),
                questions_total: 0,
                answered: 0,
                correct: 0,
                correct_rate: None,
                error: Some("缺少 summary 文件".to_string()),
                items: Vec::new(),
                per_epoch: Vec::new(),
            });
            continue;
        };
        let messages = load_case_messages(&case, &cases_dir)?;
        let recent_messages = {
            let run_summary: Option<serde_json::Value> =
                std::fs::read_to_string(summaries_dir.join("run_summary.json"))
                    .ok()
                    .and_then(|text| serde_json::from_str(&text).ok());
            run_summary
                .as_ref()
                .and_then(|report| report.get("cases").and_then(|v| v.as_array()))
                .and_then(|cases| {
                    cases.iter().find(|item| {
                        item.get("case_id").and_then(|v| v.as_str()) == Some(case_id.as_str())
                    })
                })
                .and_then(|item| item.get("recent_messages"))
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
        };
        match answer_with_questions(
            &case,
            &messages,
            &summary,
            recent_messages,
            &set.items,
            &model,
            &judge,
            &reasoning,
            &judge_reasoning,
            &mut report.usage,
        )
        .await
        {
            Ok(result) => {
                report.evaluated += 1;
                report.cases.push(result);
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
                    per_epoch: Vec::new(),
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
    report.per_epoch = merge_epoch_stats(&report.cases);
    let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    write_or_print(output.as_deref(), &json)
}

/// 用给定题目集跑答题 + 判卷（`answer` 与一步式 `quiz_one` 共用）。
#[allow(clippy::too_many_arguments)]
async fn answer_with_questions(
    case: &serde_json::Value,
    messages: &[serde_json::Value],
    summary: &str,
    recent_messages: Option<usize>,
    questions: &[QuizItem],
    model: &str,
    judge: &str,
    reasoning: &str,
    judge_reasoning: &str,
    usage: &mut Vec<QuizUsage>,
) -> Result<CaseQuiz, String> {
    let recent = recent_messages.unwrap_or(messages.len() / 10);
    let compacted = messages.len().saturating_sub(recent);
    let tail = &messages[compacted..]; // allow unsafe_text_op: Vec slice
    let source = format!(
        "<summary>\n{summary}\n</summary>\n<recent>\n{}\n</recent>",
        truncate_by_tokens(
            &message_texts(tail).join("\n"),
            ANSWER_SOURCE_TOKEN_BUDGET / 2
        )
    );
    let mut items: Vec<QuizItem> = questions.to_vec();
    let raw = complete_with_retry(
        model,
        "你是答题人。",
        &answers_prompt(&source, &items),
        reasoning,
        usage,
        "answer",
    )
    .await?;
    let Some(answers) = parse_json_items(&raw) else {
        return Err("答题输出解析失败".to_string());
    };
    for (item, answer) in items.iter_mut().zip(answers) {
        item.answer = answer.answer;
    }
    let raw = complete_with_retry(
        judge,
        "你是独立判卷人。",
        &verdict_prompt(&items),
        judge_reasoning,
        usage,
        "verdict",
    )
    .await?;
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
    let per_epoch = merge_epoch_stats(&[CaseQuiz {
        case_id: String::new(),
        status: String::new(),
        questions_total: 0,
        answered: 0,
        correct: 0,
        correct_rate: None,
        error: None,
        items: items.clone(),
        per_epoch: Vec::new(),
    }]);
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
        per_epoch,
    })
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

#[cfg(test)]
#[path = "quiz_tests.rs"]
mod quiz_tests;
