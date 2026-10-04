//! AuditedScoringAdapter 行为测试：事件字段完整性、失败路径、指纹稳定性。

use super::*;
use crate::domain::UnavailableKind;

struct StubScoringPort {
    outcome: Result<Vec<ScoringAnswer>, ScoringUnavailable>,
}

#[async_trait]
impl ScoringPort for StubScoringPort {
    async fn answer(
        &self,
        _state: &ScoringState,
        _questions: &[ScoringQuestion],
    ) -> Result<Vec<ScoringAnswer>, ScoringUnavailable> {
        self.outcome.clone()
    }
}

fn fixed_clock() -> Arc<dyn Fn() -> String + Send + Sync> {
    Arc::new(|| "2026-10-04T00:00:00Z".to_owned())
}

fn sample_state() -> ScoringState {
    ScoringState::new("用户正在调试日志路由。").expect("state 构造")
}

fn sample_questions() -> Vec<ScoringQuestion> {
    vec![ScoringQuestion::noul("任务是否已完成？", None).expect("question 构造")]
}

fn audited_with(
    outcome: Result<Vec<ScoringAnswer>, ScoringUnavailable>,
    audit_path: PathBuf,
) -> AuditedScoringAdapter {
    AuditedScoringAdapter::new(
        Arc::new(StubScoringPort { outcome }),
        "kev-0.8b@2026-09-30",
        audit_path,
        fixed_clock(),
    )
}

fn read_events(path: &std::path::Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path)
        .expect("audit.jsonl 应存在")
        .lines()
        .map(|line| serde_json::from_str(line).expect("行应为合法 JSON"))
        .collect()
}

#[tokio::test]
async fn success_answer_appends_audit_event_with_full_fields() {
    let temp = tempfile::TempDir::new().expect("临时目录");
    let audit_path = temp.path().join("audit.jsonl");
    let adapter = audited_with(
        Ok(vec![ScoringAnswer::choice(
            "a",
            vec![("a".to_owned(), 0.9), ("b".to_owned(), 0.1)],
            0.9,
            CalibrationLevel::Temperature,
        )
        .expect("答案构造")]),
        audit_path.clone(),
    );
    let questions = vec![ScoringQuestion::choice(
        "哪个最相关？",
        vec![
            ("a".to_owned(), "候选甲。".to_owned()),
            ("b".to_owned(), "候选乙。".to_owned()),
        ],
    )
    .expect("question 构造")];

    let answers = adapter
        .answer(&sample_state(), &questions)
        .await
        .expect("评分应成功");
    assert_eq!(answers.len(), 1);

    let events = read_events(&audit_path);
    assert_eq!(events.len(), 1, "一次评分应落一条审计事件");
    let event = &events[0];
    assert_eq!(event["timestamp"], "2026-10-04T00:00:00Z");
    assert_eq!(event["engine_revision"], "kev-0.8b@2026-09-30");
    assert_eq!(event["question_count"], 1);
    assert_eq!(event["outcome"], "ok");
    assert_eq!(event["probabilities"], serde_json::json!([[0.9, 0.1]]));
    assert_eq!(event["calibration"], serde_json::json!(["Temperature"]));
    assert!(event["latency_ms"].is_u64() || event["latency_ms"].is_i64());
    let fingerprint = event["prompt_sha256"].as_str().expect("指纹存在");
    assert_eq!(fingerprint.len(), 64, "sha256 hex 应为 64 字符");
}

#[tokio::test]
async fn unavailable_answer_appends_outcome_and_kind() {
    let temp = tempfile::TempDir::new().expect("临时目录");
    let audit_path = temp.path().join("audit.jsonl");
    let adapter = audited_with(
        Err(ScoringUnavailable::new(
            UnavailableKind::Timeout,
            "服务无响应",
        )),
        audit_path.clone(),
    );

    let outcome = adapter.answer(&sample_state(), &sample_questions()).await;
    assert!(outcome.is_err(), "不可用必须透传给消费点");

    let events = read_events(&audit_path);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["outcome"], "unavailable");
    assert_eq!(events[0]["unavailable_kind"], "Timeout");
    assert!(
        events[0].get("probabilities").is_none(),
        "空概率列表应被 skip_serializing_if 省略"
    );
}

#[tokio::test]
async fn audit_write_failure_does_not_break_answer() {
    // 审计路径指向非法位置（路径穿越到已存在文件下），落盘必失败。
    let temp = tempfile::TempDir::new().expect("临时目录");
    let blocker = temp.path().join("blocker");
    std::fs::write(&blocker, "占据路径").expect("写入占位文件");
    let audit_path = blocker.join("audit.jsonl");
    let adapter = audited_with(
        Ok(vec![
            ScoringAnswer::noul(0.8, CalibrationLevel::Raw).expect("答案构造")
        ]),
        audit_path,
    );

    let answers = adapter
        .answer(&sample_state(), &sample_questions())
        .await
        .expect("审计失败 NEVER 阻断评分返回");

    assert!(
        matches!(&answers[0], ScoringAnswer::Noul { p_true, .. } if (*p_true - 0.8).abs() < 1e-9)
    );
}

#[tokio::test]
async fn prompt_fingerprint_stable_for_same_input_and_differs_for_different() {
    let temp = tempfile::TempDir::new().expect("临时目录");
    let audit_path = temp.path().join("audit.jsonl");
    let adapter = audited_with(
        Ok(vec![
            ScoringAnswer::noul(0.5, CalibrationLevel::Raw).expect("答案构造")
        ]),
        audit_path.clone(),
    );

    adapter
        .answer(&sample_state(), &sample_questions())
        .await
        .expect("第一次");
    adapter
        .answer(&sample_state(), &sample_questions())
        .await
        .expect("第二次");
    let other_questions = vec![ScoringQuestion::noul("另一个命题？", None).expect("question 构造")];
    adapter
        .answer(&sample_state(), &other_questions)
        .await
        .expect("第三次");

    let events = read_events(&audit_path);
    assert_eq!(events.len(), 3);
    assert_eq!(
        events[0]["prompt_sha256"], events[1]["prompt_sha256"],
        "相同输入指纹必须稳定"
    );
    assert_ne!(
        events[0]["prompt_sha256"], events[2]["prompt_sha256"],
        "不同输入指纹必须不同"
    );
}
