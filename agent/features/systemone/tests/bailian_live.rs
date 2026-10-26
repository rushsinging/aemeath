//! 真端点 e2e（L5，#2095）：对百炼决策模型真实调用，验证 wire 兼容与概率排序。
//!
//! **NEVER** 进 CI（`#[ignore]`）；本地显式运行：
//!
//! ```bash
//! AEMEATH_TEST_BAILIAN_ENDPOINT=https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1/systemone \
//! AEMEATH_TEST_BAILIAN_API_KEY=sk-xxx \
//! cargo test -p systemone --test bailian_live -- --ignored --nocapture
//! ```
//!
//! 缺 env 时跳过（非失败），凭证 **NEVER** 写入仓库。

use std::time::Duration;

use systemone::{BailianSystemOneClient, ScoringPort};
use systemone::{ScoringAnswer, ScoringQuestion, ScoringState};

fn live_client() -> Option<BailianSystemOneClient> {
    let endpoint = std::env::var("AEMEATH_TEST_BAILIAN_ENDPOINT").ok()?;
    let api_key = std::env::var("AEMEATH_TEST_BAILIAN_API_KEY").ok()?;
    Some(BailianSystemOneClient::new(
        endpoint,
        api_key,
        "decision-model-preview".to_string(),
        Duration::from_secs(30),
    ))
}

#[tokio::test]
#[ignore = "真端点 e2e：需 AEMEATH_TEST_BAILIAN_ENDPOINT/API_KEY 环境变量，NEVER 进 CI"]
async fn live_bailian_choice_reranks_memory_candidates_by_relevance() {
    let Some(client) = live_client() else {
        eprintln!("跳过：未设置 AEMEATH_TEST_BAILIAN_ENDPOINT/API_KEY");
        return;
    };
    let state =
        ScoringState::new("用户问：如何预防感冒？勤洗手是有效的预防方法。").expect("state 构造");
    let question = ScoringQuestion::choice(
        "哪条记忆与当前对话最相关？",
        vec![
            (
                "m1".to_string(),
                "关于预防感冒的卫生习惯与洗手时机".to_string(),
            ),
            (
                "m2".to_string(),
                "Rust 编程语言的所有权与内存安全特性".to_string(),
            ),
            ("m3".to_string(), "今日股市行情与板块轮动分析".to_string()),
        ],
    )
    .expect("choice 构造");

    let answers = client
        .answer(&state, &[question])
        .await
        .expect("真端点调用应成功");
    assert_eq!(answers.len(), 1);
    let ScoringAnswer::Choice {
        choice,
        probabilities,
        ..
    } = &answers[0]
    else {
        panic!("应为 Choice 答案：{:?}", answers[0]);
    };
    eprintln!("live choice={choice} probabilities={probabilities:?}");
    assert_eq!(choice, "m1", "语义最相关候选应排第一");
    let m1 = probabilities
        .iter()
        .find(|(key, _)| key == "m1")
        .expect("m1 概率应存在")
        .1;
    let m2 = probabilities
        .iter()
        .find(|(key, _)| key == "m2")
        .expect("m2 概率应存在")
        .1;
    assert!(m1 > m2, "相关候选概率必须高于无关候选：m1={m1} m2={m2}");
    let total: f64 = probabilities
        .iter()
        .map(|(_, probability)| probability)
        .sum();
    assert!(
        (total - 1.0).abs() < 0.05,
        "候选集内概率和应≈1（实测 {total}）"
    );
}

#[tokio::test]
#[ignore = "真端点 e2e：需 AEMEATH_TEST_BAILIAN_ENDPOINT/API_KEY 环境变量，NEVER 进 CI"]
async fn live_bailian_noul_answers_binary_question() {
    let Some(client) = live_client() else {
        eprintln!("跳过：未设置 AEMEATH_TEST_BAILIAN_ENDPOINT/API_KEY");
        return;
    };
    let state = ScoringState::new("这段文本是一个用中文写成的疑问句。").expect("state 构造");
    let question = ScoringQuestion::noul("这段文本是否为中文？", None).expect("noul 构造");
    let answers = client
        .answer(&state, &[question])
        .await
        .expect("真端点调用应成功");
    let ScoringAnswer::Noul { p_true, .. } = &answers[0] else {
        panic!("应为 Noul 答案：{:?}", answers[0]);
    };
    eprintln!("live noul p_true={p_true}");
    assert!(*p_true > 0.5, "中文文本应判定为真（p_true={p_true}）");
}
