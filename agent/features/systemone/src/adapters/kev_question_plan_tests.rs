//! KevQuestionPlan：published language → kev `to_record` 口径的指令、选项文案与概率 key。

use crate::adapters::kev_question_plan::{plan_kev_question, KevQuestionKind};
use crate::domain::{NoulCriteria, ScoringQuestion};

#[test]
fn noul_without_criteria_plans_no_and_yes_options() {
    let question = ScoringQuestion::noul("Is the task fully complete?", None).unwrap();
    let plan = plan_kev_question(&question);
    assert_eq!(plan.kind, KevQuestionKind::Noul);
    assert_eq!(plan.instruction_text, "Is the task fully complete?");
    assert_eq!(
        plan.option_texts,
        vec!["no".to_owned(), "yes".to_owned()],
        "无 criteria 时 Noul 选项为 kev 默认 no/yes"
    );
    assert_eq!(
        plan.probability_keys,
        vec!["false".to_owned(), "true".to_owned()]
    );
}

#[test]
fn noul_with_criteria_prefixes_semantic_sentences() {
    let criteria = NoulCriteria::new(
        "All requirements of the task are met.",
        "At least one requirement is unmet.",
    )
    .unwrap();
    let question = ScoringQuestion::noul(
        "Based on the state, is the task fully complete?",
        Some(criteria),
    )
    .unwrap();
    let plan = plan_kev_question(&question);
    assert_eq!(
        plan.option_texts,
        vec![
            "no: At least one requirement is unmet.".to_owned(),
            "yes: All requirements of the task are met.".to_owned(),
        ],
        "kev option_text 口径：name + ': ' + 完整语义句，false 在前 true 在后"
    );
    assert_eq!(
        plan.probability_keys,
        vec!["false".to_owned(), "true".to_owned()]
    );
}

#[test]
fn choice_plans_criteria_keys_and_texts_in_submission_order() {
    let question = ScoringQuestion::choice(
        "Given the session, retrieve the most relevant memory.",
        vec![
            ("0".to_owned(), "User prefers short answers.".to_owned()),
            (
                "1".to_owned(),
                "Deploy pipeline was reconfigured.".to_owned(),
            ),
        ],
    )
    .unwrap();
    let plan = plan_kev_question(&question);
    assert_eq!(plan.kind, KevQuestionKind::Choice);
    assert_eq!(
        plan.instruction_text,
        "Given the session, retrieve the most relevant memory."
    );
    assert_eq!(
        plan.probability_keys,
        vec!["0".to_owned(), "1".to_owned()],
        "概率 key 按 criteria 提交顺序，保序是候选展示顺序的来源"
    );
    assert_eq!(
        plan.option_texts,
        vec![
            "0: User prefers short answers.".to_owned(),
            "1: Deploy pipeline was reconfigured.".to_owned(),
        ]
    );
}

#[test]
fn score_plans_levels_in_ascending_order_with_index_keys() {
    let question = ScoringQuestion::score(
        "How risky is the requested operation?",
        vec![
            "The operation is easily reversible.".to_owned(),
            "The operation destroys data irreversibly.".to_owned(),
        ],
    )
    .unwrap();
    let plan = plan_kev_question(&question);
    assert_eq!(plan.kind, KevQuestionKind::Score);
    assert_eq!(
        plan.option_texts,
        vec![
            "The operation is easily reversible.".to_owned(),
            "The operation destroys data irreversibly.".to_owned(),
        ],
        "Score 选项即等级描述原文，升序 = 风险升序"
    );
    assert_eq!(plan.probability_keys, vec!["0".to_owned(), "1".to_owned()]);
}
