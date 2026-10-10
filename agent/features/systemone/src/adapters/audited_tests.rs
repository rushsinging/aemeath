//! AuditedScoringAdapter 行为测试：日切事件流落盘、legacy 迁移、GC 与 fail-open。
//!
//! 契约（设计 `docs/design/02-modules/systemone/03-event-stream.md` §4–§7）：
//! 每次评分往 `{scoring_dir}/events/{yyyy-mm-dd}.jsonl` 追加一行完整
//! `ScoringEvent`（全量现场 + 关联字段 null），落盘失败 NEVER 影响评分返回。

use std::path::{Path, PathBuf};

use super::*;
use crate::domain::{CalibrationLevel, ScoringEvent, UnavailableKind};

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

/// 当前 UTC 日期：segment 文件名日期与断言同源。
fn today() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

/// 注入时钟：返回指定 UTC 日零点（事件落盘日期确定可断言）。
fn clock_at(day: &str) -> Arc<dyn Fn() -> String + Send + Sync> {
    let day = day.to_owned();
    Arc::new(move || format!("{day}T00:00:00Z"))
}

fn sample_state() -> ScoringState {
    ScoringState::new("用户正在调试日志路由。").expect("state 构造")
}

fn sample_questions() -> Vec<ScoringQuestion> {
    vec![ScoringQuestion::noul("任务是否已完成？", None).expect("question 构造")]
}

fn audited_with_day(
    outcome: Result<Vec<ScoringAnswer>, ScoringUnavailable>,
    scoring_dir: PathBuf,
    day: &str,
) -> AuditedScoringAdapter {
    AuditedScoringAdapter::new(
        Arc::new(StubScoringPort { outcome }),
        "kev-0.8b@2026-09-30",
        JsonlSegmentScoringEventStore::new(
            scoring_dir,
            share::config::scoring::DEFAULT_EVENT_RETENTION_DAYS,
        ),
        clock_at(day),
        "memory_rerank",
    )
}

fn read_daily_events(scoring_dir: &Path, day: &str) -> Vec<serde_json::Value> {
    let path = scoring_dir
        .join(crate::constants::EVENTS_DIR_NAME)
        .join(format!("{day}.jsonl"));
    std::fs::read_to_string(path)
        .expect("日切事件文件应存在")
        .lines()
        .map(|line| serde_json::from_str(line).expect("行应为合法 JSON"))
        .collect()
}

#[tokio::test]
async fn success_answer_appends_full_scoring_event_to_daily_segment() {
    let day = today();
    let temp = tempfile::TempDir::new().expect("临时目录");
    let scoring_dir = temp.path().join("scoring");
    let adapter = audited_with_day(
        Ok(vec![ScoringAnswer::choice(
            "a",
            vec![("a".to_owned(), 0.9), ("b".to_owned(), 0.1)],
            0.9,
            CalibrationLevel::Temperature,
        )
        .expect("答案构造")]),
        scoring_dir.clone(),
        &day,
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

    let events = read_daily_events(&scoring_dir, &day);
    assert_eq!(events.len(), 1, "一次评分应落一条日切事件");
    let event = &events[0];

    // Envelope：schema 版本、事件 id、时间戳同刻。
    assert_eq!(event["schema_version"], 1);
    assert!(event["event_id"]
        .as_str()
        .expect("event_id 存在")
        .starts_with("evt-"));
    assert!(event["timestamp"]
        .as_str()
        .expect("timestamp 存在")
        .starts_with(&day));
    assert!(event["ts_unix_ms"].is_u64());
    assert!(event["latency_ms"].is_u64());

    // 归因与评分现场。
    assert_eq!(event["scenario"], "memory_rerank");
    assert_eq!(event["engine_revision"], "kev-0.8b@2026-09-30");
    assert_eq!(event["outcome"], "ok");
    assert!(event["unavailable_kind"].is_null());
    assert_eq!(event["state_text"], "用户正在调试日志路由。");
    assert_eq!(event["question_count"], 1);
    let fingerprint = event["prompt_sha256"].as_str().expect("指纹存在");
    assert_eq!(fingerprint.len(), 64, "sha256 hex 应为 64 字符");

    // questions 全文在场（choice 面 + 选项 key/描述）。
    assert_eq!(event["questions"][0]["kind"], "choice");
    assert_eq!(event["questions"][0]["instructions"], "哪个最相关？");
    assert_eq!(event["questions"][0]["criteria"][0]["key"], "a");
    assert_eq!(event["questions"][0]["criteria"][0]["text"], "候选甲。");

    // answers 快照：概率分布 + 校准级别。
    assert_eq!(
        event["answers"],
        serde_json::json!([
            {"probabilities": [0.9, 0.1], "calibration": "Temperature"}
        ])
    );

    // ranking 占位 null；关联字段全 null（serde null，不省略）。
    assert!(event["ranking"].is_null());
    for field in [
        "correlation_id",
        "session_id",
        "run_ordinal",
        "step_ordinal",
        "tool_call_id",
    ] {
        assert!(event[field].is_null(), "{field} 应序列化为 null");
    }

    // 行必须可反序列化为强类型 ScoringEvent。
    let typed: ScoringEvent = serde_json::from_value(event.clone()).expect("ScoringEvent 反序列化");
    assert_eq!(typed.schema_version, ScoringEvent::SCHEMA_VERSION);
    assert_eq!(typed.outcome, "ok");
    assert_eq!(typed.state_text, "用户正在调试日志路由。");
    assert_eq!(typed.questions.len(), 1);
    assert_eq!(typed.answers[0].probabilities, vec![0.9, 0.1]);
    assert!(typed.correlation_id.is_none());
    assert!(typed.ranking.is_none());
}

#[tokio::test]
async fn unavailable_answer_records_outcome_and_kind() {
    let day = today();
    let temp = tempfile::TempDir::new().expect("临时目录");
    let scoring_dir = temp.path().join("scoring");
    let adapter = audited_with_day(
        Err(ScoringUnavailable::new(
            UnavailableKind::Timeout,
            "服务无响应",
        )),
        scoring_dir.clone(),
        &day,
    );

    let outcome = adapter.answer(&sample_state(), &sample_questions()).await;
    assert!(outcome.is_err(), "不可用必须透传给消费点");

    let events = read_daily_events(&scoring_dir, &day);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["outcome"], "unavailable");
    assert_eq!(events[0]["unavailable_kind"], "Timeout");
    assert_eq!(events[0]["answers"], serde_json::json!([]));
    assert_eq!(events[0]["state_text"], "用户正在调试日志路由。");
}

#[tokio::test]
async fn legacy_audit_migrated_then_only_daily_segment_written() {
    let day = today();
    let temp = tempfile::TempDir::new().expect("临时目录");
    let scoring_dir = temp.path().join("scoring");
    std::fs::create_dir_all(&scoring_dir).expect("scoring 目录创建");
    let legacy_lines = "{\"legacy\":1}\n{\"legacy\":2}\n";
    std::fs::write(
        scoring_dir.join(crate::constants::LEGACY_AUDIT_FILE),
        legacy_lines,
    )
    .expect("legacy 审计预置");

    let adapter = audited_with_day(
        Ok(vec![
            ScoringAnswer::noul(0.7, CalibrationLevel::Raw).expect("答案构造")
        ]),
        scoring_dir.clone(),
        &day,
    );

    // 构造期一次性迁移：旧行原样归档，原文件改名。
    let archive_path = scoring_dir
        .join(crate::constants::EVENTS_DIR_NAME)
        .join(crate::constants::LEGACY_AUDIT_ARCHIVE_FILE);
    assert_eq!(
        std::fs::read_to_string(&archive_path).expect("归档应存在"),
        legacy_lines,
        "legacy 行 MUST 原样归档"
    );
    assert!(
        !scoring_dir
            .join(crate::constants::LEGACY_AUDIT_FILE)
            .exists(),
        "原 legacy 路径迁移后不再存在"
    );
    let migrated = scoring_dir.join(format!("{}.migrated", crate::constants::LEGACY_AUDIT_FILE));
    assert_eq!(
        std::fs::read_to_string(&migrated).expect("迁移改名文件应存在"),
        legacy_lines,
        "改名后的原文件保留完整旧行"
    );

    adapter
        .answer(&sample_state(), &sample_questions())
        .await
        .expect("评分成功");

    // 之后只写日切 segment，不再触碰 legacy 路径。
    let events = read_daily_events(&scoring_dir, &day);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["outcome"], "ok");
    assert_eq!(
        std::fs::read_to_string(&archive_path).expect("归档应仍在"),
        legacy_lines,
        "新事件 NEVER 混入 legacy 归档"
    );
    assert!(!scoring_dir
        .join(crate::constants::LEGACY_AUDIT_FILE)
        .exists());
}

#[tokio::test]
async fn event_store_failure_does_not_break_answer() {
    // scoring_dir 指向一个已存在文件：迁移读、GC、append 全部 IO 失败。
    let temp = tempfile::TempDir::new().expect("临时目录");
    let blocker = temp.path().join("blocker");
    std::fs::write(&blocker, "占据路径").expect("写入占位文件");
    let adapter = audited_with_day(
        Ok(vec![
            ScoringAnswer::noul(0.8, CalibrationLevel::Raw).expect("答案构造")
        ]),
        blocker,
        &today(),
    );

    let answers = adapter
        .answer(&sample_state(), &sample_questions())
        .await
        .expect("事件落盘失败 NEVER 阻断评分返回");

    assert!(
        matches!(&answers[0], ScoringAnswer::Noul { p_true, .. } if (*p_true - 0.8).abs() < 1e-9)
    );
}

#[tokio::test]
async fn construction_runs_gc_on_expired_segments() {
    let day = today();
    let temp = tempfile::TempDir::new().expect("临时目录");
    let scoring_dir = temp.path().join("scoring");
    let events_dir = scoring_dir.join(crate::constants::EVENTS_DIR_NAME);
    std::fs::create_dir_all(&events_dir).expect("events 目录创建");
    let expired = events_dir.join("2000-01-01.jsonl");
    std::fs::write(&expired, "{\"stale\":true}\n").expect("过期 segment 预置");
    let recent = events_dir.join(format!("{day}.jsonl"));
    std::fs::write(&recent, "{\"fresh\":true}\n").expect("近期 segment 预置");

    let _adapter = audited_with_day(
        Ok(vec![
            ScoringAnswer::noul(0.5, CalibrationLevel::Raw).expect("答案构造")
        ]),
        scoring_dir.clone(),
        &day,
    );

    assert!(
        !expired.exists(),
        "超过保留期的 segment MUST 在构造期 GC 删除"
    );
    assert!(recent.exists(), "保留期内的 segment NEVER 被删");
}

#[tokio::test]
async fn prompt_fingerprint_stable_for_same_input_and_differs_for_different() {
    let day = today();
    let temp = tempfile::TempDir::new().expect("临时目录");
    let scoring_dir = temp.path().join("scoring");
    let adapter = audited_with_day(
        Ok(vec![
            ScoringAnswer::noul(0.5, CalibrationLevel::Raw).expect("答案构造")
        ]),
        scoring_dir.clone(),
        &day,
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

    let events = read_daily_events(&scoring_dir, &day);
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
