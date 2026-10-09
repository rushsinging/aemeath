//! KevCausalRowBuilder：published language 计划 → 逐题 causal row（state 前缀 + 分支 + readout 偏移）。

use crate::adapters::kev_causal_row::{
    CausalRowBuildError, KevCausalRowBuilder, KevDelimiter, KevRowTokenizer,
};
use crate::adapters::kev_question_plan::plan_kev_question;
use crate::domain::{NoulCriteria, ScoringQuestion};

/// 确定性 fake tokenizer：文本 → 逐字节 token id；分隔 token → 负数哨兵 id。
struct FakeByteTokenizer;

impl KevRowTokenizer for FakeByteTokenizer {
    fn tokenize_user_text(&self, text: &str) -> Result<Vec<i32>, CausalRowBuildError> {
        Ok(text.bytes().map(i32::from).collect())
    }

    fn delimiter_id(&self, delimiter: KevDelimiter) -> Result<i32, CausalRowBuildError> {
        Ok(match delimiter {
            KevDelimiter::StateStart => -10,
            KevDelimiter::QuestionStart => -11,
            KevDelimiter::OptionStart => -12,
            KevDelimiter::OptionEnd => -13,
            KevDelimiter::Decide => -14,
        })
    }
}

fn choice_question() -> ScoringQuestion {
    ScoringQuestion::choice(
        "Which memory is most relevant?",
        vec![
            ("0".to_owned(), "User prefers short answers.".to_owned()),
            (
                "1".to_owned(),
                "Deploy pipeline was reconfigured.".to_owned(),
            ),
        ],
    )
    .unwrap()
}

fn noul_question() -> ScoringQuestion {
    let criteria =
        NoulCriteria::new("All requirements are met.", "A requirement is unmet.").unwrap();
    ScoringQuestion::noul("Is the task fully complete?", Some(criteria)).unwrap()
}

/// 按 kev encode 口径拼出期望 row：state 前缀 + [q] instr + 各 [opt] span [ /opt ] + [decide]。
fn expected_row(
    tokenizer: &FakeByteTokenizer,
    state_text: &str,
    max_state_tokens: usize,
    instruction_text: &str,
    option_texts: &[&str],
) -> Vec<i32> {
    let state_ids = tokenizer.tokenize_user_text(state_text).unwrap();
    let mut state_prefix = vec![tokenizer.delimiter_id(KevDelimiter::StateStart).unwrap()];
    state_prefix.extend(
        state_ids
            .into_iter()
            .take(max_state_tokens.saturating_sub(1)),
    );
    let mut row = state_prefix;
    row.push(tokenizer.delimiter_id(KevDelimiter::QuestionStart).unwrap());
    row.extend(tokenizer.tokenize_user_text(instruction_text).unwrap());
    for option_text in option_texts {
        row.push(tokenizer.delimiter_id(KevDelimiter::OptionStart).unwrap());
        row.extend(tokenizer.tokenize_user_text(option_text).unwrap());
        row.push(tokenizer.delimiter_id(KevDelimiter::OptionEnd).unwrap());
    }
    row.push(tokenizer.delimiter_id(KevDelimiter::Decide).unwrap());
    row
}

#[test]
fn build_rows_produces_one_row_per_question_with_decide_at_branch_end() {
    let builder = KevCausalRowBuilder::new(FakeByteTokenizer);
    let plans = vec![
        plan_kev_question(&noul_question()),
        plan_kev_question(&choice_question()),
    ];
    let rows = builder
        .build_rows("the session state", &plans)
        .expect("causal row 构建成功");

    assert_eq!(rows.len(), 2, "一题一行");

    let tokenizer = FakeByteTokenizer;
    let expected_first = expected_row(
        &tokenizer,
        "the session state",
        crate::constants::KEV_MAX_STATE_TOKENS,
        "Is the task fully complete?",
        &[
            "no: A requirement is unmet.",
            "yes: All requirements are met.",
        ],
    );
    assert_eq!(
        rows[0].token_ids, expected_first,
        "state 前缀 + 分支逐 token 对齐 kev encode"
    );
    assert_eq!(
        rows[0].decide_offset,
        expected_first.len() - 1,
        "decide 是分支最后一个 token"
    );
    assert_eq!(
        rows[0].token_ids[rows[0].decide_offset],
        tokenizer.delimiter_id(KevDelimiter::Decide).unwrap()
    );
    assert_eq!(rows[0].option_offsets.len(), 2, "Noul 固定两个选项 readout");
    for option_offset in &rows[0].option_offsets {
        assert_eq!(
            rows[0].token_ids[*option_offset],
            tokenizer.delimiter_id(KevDelimiter::OptionEnd).unwrap(),
            "每个选项 readout 落在 option-end 分隔 token"
        );
    }
}

#[test]
fn build_rows_keeps_shared_state_prefix_and_question_order() {
    let builder = KevCausalRowBuilder::new(FakeByteTokenizer);
    let plans = vec![
        plan_kev_question(&noul_question()),
        plan_kev_question(&choice_question()),
    ];
    let rows = builder.build_rows("shared state", &plans).unwrap();

    let state_token_count = 1 + FakeByteTokenizer
        .tokenize_user_text("shared state")
        .unwrap()
        .len();
    let first_prefix = &rows[0].token_ids[..state_token_count];
    let second_prefix = &rows[1].token_ids[..state_token_count];
    assert_eq!(
        first_prefix, second_prefix,
        "同一 state 前缀在所有行中完全一致"
    );

    let tokenizer = FakeByteTokenizer;
    let expected_second = expected_row(
        &tokenizer,
        "shared state",
        crate::constants::KEV_MAX_STATE_TOKENS,
        "Which memory is most relevant?",
        &[
            "0: User prefers short answers.",
            "1: Deploy pipeline was reconfigured.",
        ],
    );
    assert_eq!(rows[1].token_ids, expected_second, "第二行分支属于第二题");
    assert_eq!(rows[1].option_offsets.len(), 2);
}

#[test]
fn build_rows_when_branch_exceeds_limit_reports_branch_too_long() {
    let builder = KevCausalRowBuilder::with_limits(FakeByteTokenizer, 64, 24);
    let plans = vec![plan_kev_question(&choice_question())];
    let error = builder
        .build_rows("state", &plans)
        .expect_err("分支超出 row 限额必须失败");
    match &error {
        CausalRowBuildError::BranchTooLong {
            branch_tokens,
            state_tokens,
            limit,
        } => {
            assert_eq!(*state_tokens, 1 + 5, "state 分隔 token + state 字节");
            assert_eq!(*limit, 24);
            assert!(*branch_tokens > 24, "分支长度必须超出限额");
        }
        other => panic!("错误类型不符：{other:?}"),
    }
    assert!(
        error.to_string().contains("超出"),
        "错误消息为中文：{}",
        error
    );
}

#[test]
fn build_rows_truncates_state_at_max_state_tokens() {
    let builder = KevCausalRowBuilder::with_limits(FakeByteTokenizer, 4, 4096);
    let plans = vec![plan_kev_question(&noul_question())];
    let rows = builder.build_rows("abcdef", &plans).unwrap();
    let state_token_count = 1 + 3;
    assert_eq!(
        rows[0].token_ids[..state_token_count],
        [-10, i32::from(b'a'), i32::from(b'b'), i32::from(b'c')],
        "state 超限时截断为 state 分隔 token + 前 max_state-1 个 token"
    );
}

#[test]
fn build_rows_with_no_questions_returns_no_rows() {
    let builder = KevCausalRowBuilder::new(FakeByteTokenizer);
    let rows = builder.build_rows("state", &[]).unwrap();
    assert!(rows.is_empty(), "空题目集不产生 row，也不触达 worker");
}
