//! KevQuestionPlan：published language → kev `to_record` 口径的单一事实来源。
//!
//! 同一份 plan 同时喂给 causal row 编码（选项文案）与答案映射（概率 key），
//! 保证「引擎看到的选项顺序」与「答案汇报的 key 顺序」NEVER 分叉。
//! 文案口径复刻 kev `api.py::to_record` / `option_text`：`name` 或 `name: desc`。

use crate::domain::ScoringQuestion;

/// 三题型（编码与答案映射共用的判别）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KevQuestionKind {
    Noul,
    Choice,
    Score,
}

/// 一道题的 kev 视图：指令文案、选项文案（引擎输入顺序）与概率 key（答案汇报顺序）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KevQuestionPlan {
    /// 题型判别。
    pub(crate) kind: KevQuestionKind,
    /// 指令原文（kev `render(str)` 恒等）。
    pub(crate) instruction_text: String,
    /// 选项文案，按引擎选项顺序。
    pub(crate) option_texts: Vec<String>,
    /// 概率 key，与 `option_texts` 逐位对应。
    pub(crate) probability_keys: Vec<String>,
}

/// 将 published language 的一道题映射为 kev plan。
pub(crate) fn plan_kev_question(question: &ScoringQuestion) -> KevQuestionPlan {
    match question {
        ScoringQuestion::Noul {
            instructions,
            criteria,
        } => {
            // kev Noul → 2 options [false, true]：no 在前、yes 在后，
            // 概率汇报 p(true) = probs[1]。
            let (when_false, when_true) = match criteria {
                Some(criteria) => (
                    criteria.when_false().to_owned(),
                    criteria.when_true().to_owned(),
                ),
                None => (String::new(), String::new()),
            };
            KevQuestionPlan {
                kind: KevQuestionKind::Noul,
                instruction_text: instructions.clone(),
                option_texts: vec![
                    kev_option_text("no", &when_false),
                    kev_option_text("yes", &when_true),
                ],
                probability_keys: vec!["false".to_owned(), "true".to_owned()],
            }
        }
        ScoringQuestion::Choice {
            instructions,
            criteria,
        } => KevQuestionPlan {
            kind: KevQuestionKind::Choice,
            instruction_text: instructions.clone(),
            option_texts: criteria
                .iter()
                .map(|(key, description)| kev_option_text(key, description))
                .collect(),
            probability_keys: criteria.iter().map(|(key, _)| key.clone()).collect(),
        },
        ScoringQuestion::Score {
            instructions,
            levels,
        } => KevQuestionPlan {
            kind: KevQuestionKind::Score,
            instruction_text: instructions.clone(),
            option_texts: levels.clone(),
            probability_keys: (0..levels.len()).map(|index| index.to_string()).collect(),
        },
    }
}

/// kev `option_text` 口径：`name` 或 `name: desc`（空 desc 退回 name）。
fn kev_option_text(name: &str, description: &str) -> String {
    if description.is_empty() {
        name.to_owned()
    } else {
        format!("{name}: {description}")
    }
}

#[cfg(test)]
#[path = "kev_question_plan_tests.rs"]
mod tests;
