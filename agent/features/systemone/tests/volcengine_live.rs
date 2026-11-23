//! 真端点 e2e（L5）：对火山方舟 System One（beta）真实调用，验证 beta 请求头
//! 与 wire 协议兼容性。
//!
//! **NEVER** 进 CI（`#[ignore]`）；本地显式运行：
//!
//! ```bash
//! AEMEATH_TEST_VOLCENGINE_API_KEY=ark-xxx \
//! cargo test -p systemone --test volcengine_live -- --ignored --nocapture
//! ```
//!
//! 可选覆盖：`AEMEATH_TEST_VOLCENGINE_BASE_URL`（默认方舟官方 `/api/v1`）、
//! `AEMEATH_TEST_VOLCENGINE_MODEL`（默认 `doubao-seed-2-1-lite-260915`）。
//! 缺 key 时跳过（非失败），凭证 **NEVER** 写入仓库。

use std::time::Duration;

use systemone::{ScoringAnswer, ScoringQuestion, ScoringState};
use systemone::{ScoringPort, VolcengineSystemOneClient};

const DEFAULT_BASE_URL: &str = "https://ark.cn-beijing.volces.com/api/v1";
const DEFAULT_MODEL: &str = "doubao-seed-2-1-lite-260915";

fn live_client() -> Option<VolcengineSystemOneClient> {
    let api_key = std::env::var("AEMEATH_TEST_VOLCENGINE_API_KEY").ok()?;
    let base_url = std::env::var("AEMEATH_TEST_VOLCENGINE_BASE_URL")
        .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
    let model = std::env::var("AEMEATH_TEST_VOLCENGINE_MODEL")
        .unwrap_or_else(|_| DEFAULT_MODEL.to_string());
    let endpoint = format!("{}/systemone", base_url.trim_end_matches('/'));
    Some(VolcengineSystemOneClient::new(
        endpoint,
        api_key,
        model,
        Duration::from_secs(30),
    ))
}

#[tokio::test]
#[ignore = "真端点 e2e：需 AEMEATH_TEST_VOLCENGINE_API_KEY，NEVER 进 CI"]
async fn live_volcengine_choice_reranks_memory_candidates_by_relevance() {
    let Some(client) = live_client() else {
        eprintln!("跳过：未设置 AEMEATH_TEST_VOLCENGINE_API_KEY");
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
}

#[tokio::test]
#[ignore = "真端点 e2e：需 AEMEATH_TEST_VOLCENGINE_API_KEY，NEVER 进 CI"]
async fn live_volcengine_noul_answers_binary_question() {
    let Some(client) = live_client() else {
        eprintln!("跳过：未设置 AEMEATH_TEST_VOLCENGINE_API_KEY");
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
