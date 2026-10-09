//! KevCausalRowBuilder：published language plan → kev 逐题 causal row。
//!
//! 复刻 kev `model.py::encode` + `rows_of` 的 serving 口径：
//! row = `[state] state_tokens` + `[q] instr` + 各 `[opt] span [/opt]` + `[decide]`；
//! readout 偏移 = decide（分支末 token）与每个 option-end 分隔 token。
//! state 超限截断、分支超限 fail-closed（对应 kev serve 的 422 → `UnavailableKind::Schema`）。
//!
//! 文本 → token id 的语义经 [`KevRowTokenizer`] seam 注入：生产为 HF fast
//! tokenizer（`kev_hf_tokenizer`），测试为确定性 fake，编码逻辑本身零 IO 可直测。

use crate::adapters::kev_question_plan::KevQuestionPlan;
use crate::adapters::llama_worker::CausalRow;
use crate::constants::{KEV_MAX_BRANCH_TOKENS, KEV_MAX_STATE_TOKENS};

/// kev 五个分隔 special token（与 `kev.model.SPECIAL` 顺序一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KevDelimiter {
    /// state 起始（SPECIAL[0]）。
    StateStart,
    /// 题目分支起始（SPECIAL[1]）。
    QuestionStart,
    /// 选项 span 起始（SPECIAL[2]）。
    OptionStart,
    /// 选项 span 结束（SPECIAL[3]，选项 readout 位置）。
    OptionEnd,
    /// decide（SPECIAL[4]，decide readout 位置）。
    Decide,
}

impl KevDelimiter {
    /// 分隔 token 的文本（HF tokenizer 的 special token 词表名）。
    pub(crate) fn token_text(self) -> &'static str {
        match self {
            Self::StateStart => "<|fim_prefix|>",
            Self::QuestionStart => "<|fim_middle|>",
            Self::OptionStart => "<|box_start|>",
            Self::OptionEnd => "<|box_end|>",
            Self::Decide => "<|fim_suffix|>",
        }
    }
}

/// causal row 构建失败：tokenize 失败、缺分隔 token 或分支超限（全部 → Schema 降级）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CausalRowBuildError {
    /// 文本编码失败（tokenizer 运行错误）。
    TokenizeFailed { detail: String },
    /// tokenizer 词表缺少 kev 分隔 token（安装的 tokenizer 资产不匹配）。
    MissingDelimiter { token_text: &'static str },
    /// state + 分支超出 row 限额。
    BranchTooLong {
        branch_tokens: usize,
        state_tokens: usize,
        limit: usize,
    },
}

impl std::fmt::Display for CausalRowBuildError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TokenizeFailed { detail } => write!(formatter, "文本编码失败：{detail}"),
            Self::MissingDelimiter { token_text } => {
                write!(formatter, "tokenizer 缺少 kev 分隔 token：{token_text}")
            }
            Self::BranchTooLong {
                branch_tokens,
                state_tokens,
                limit,
            } => write!(
                formatter,
                "causal row 超出上限：state {state_tokens} tokens + 分支 {branch_tokens} tokens > {limit} tokens"
            ),
        }
    }
}

impl std::error::Error for CausalRowBuildError {}

/// 文本 → token id 契约（生产：HF fast tokenizer；测试：确定性 fake）。
pub(crate) trait KevRowTokenizer {
    /// 按 kev `user_tokens` 语义编码调用方文本（不注入 special token）。
    fn tokenize_user_text(&self, text: &str) -> Result<Vec<i32>, CausalRowBuildError>;

    /// 解析 kev 分隔 token 的 id。
    fn delimiter_id(&self, delimiter: KevDelimiter) -> Result<i32, CausalRowBuildError>;
}

/// 按 kev serving 口径构建逐题 causal row。
pub(crate) struct KevCausalRowBuilder {
    tokenizer: Box<dyn KevRowTokenizer + Send + Sync>,
    max_state_tokens: usize,
    max_branch_tokens: usize,
}

impl KevCausalRowBuilder {
    /// 生产构造：kev serving 限额（`SERVE_MAX_STATE` / `SERVE_MAX_BRANCH`）。
    pub(crate) fn new(tokenizer: impl KevRowTokenizer + Send + Sync + 'static) -> Self {
        Self::with_limits(tokenizer, KEV_MAX_STATE_TOKENS, KEV_MAX_BRANCH_TOKENS)
    }

    /// 自定义限额（小限额用于测试分支超限 / state 截断路径）。
    ///
    /// `max_state_tokens` MUST ≥ 1（state 分隔 token 占一位）；生产与测试入参均满足。
    pub(crate) fn with_limits(
        tokenizer: impl KevRowTokenizer + Send + Sync + 'static,
        max_state_tokens: usize,
        max_branch_tokens: usize,
    ) -> Self {
        debug_assert!(
            max_state_tokens >= 1,
            "max_state_tokens 必须容得下 state 分隔 token"
        );
        Self {
            tokenizer: Box::new(tokenizer),
            max_state_tokens,
            max_branch_tokens,
        }
    }

    /// 一题一行：state 前缀 + 该题分支；空题目集返回空 vec（不触达 worker）。
    pub(crate) fn build_rows(
        &self,
        state_text: &str,
        plans: &[KevQuestionPlan],
    ) -> Result<Vec<CausalRow>, CausalRowBuildError> {
        let state_prefix = self.build_state_prefix(state_text)?;
        let state_token_count = state_prefix.len();
        let mut rows = Vec::with_capacity(plans.len());
        for plan in plans {
            let mut row = state_prefix.clone();
            let mut option_offsets = Vec::with_capacity(plan.option_texts.len());
            row.push(self.tokenizer.delimiter_id(KevDelimiter::QuestionStart)?);
            row.extend(self.tokenizer.tokenize_user_text(&plan.instruction_text)?);
            for option_text in &plan.option_texts {
                row.push(self.tokenizer.delimiter_id(KevDelimiter::OptionStart)?);
                row.extend(self.tokenizer.tokenize_user_text(option_text)?);
                row.push(self.tokenizer.delimiter_id(KevDelimiter::OptionEnd)?);
                option_offsets.push(row.len() - 1);
            }
            row.push(self.tokenizer.delimiter_id(KevDelimiter::Decide)?);
            if row.len() > self.max_branch_tokens {
                return Err(CausalRowBuildError::BranchTooLong {
                    branch_tokens: row.len() - state_token_count,
                    state_tokens: state_token_count,
                    limit: self.max_branch_tokens,
                });
            }
            let decide_offset = row.len() - 1;
            rows.push(CausalRow::new(row, decide_offset, option_offsets));
        }
        Ok(rows)
    }

    /// state 前缀：state 分隔 token + 截断后的 state token ids。
    fn build_state_prefix(&self, state_text: &str) -> Result<Vec<i32>, CausalRowBuildError> {
        let state_tokens = self.tokenizer.tokenize_user_text(state_text)?;
        let mut prefix = Vec::with_capacity(state_tokens.len() + 1);
        prefix.push(self.tokenizer.delimiter_id(KevDelimiter::StateStart)?);
        prefix.extend(
            state_tokens
                .into_iter()
                .take(self.max_state_tokens.saturating_sub(1)),
        );
        Ok(prefix)
    }
}

#[cfg(test)]
#[path = "kev_causal_row_tests.rs"]
mod tests;
