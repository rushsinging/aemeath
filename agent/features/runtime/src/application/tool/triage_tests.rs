//! 权限/风险预筛（#1836）：System One noul 评分对 policy Allow 判定的
//! **单向加严**（Allow + 高风险 → 提级审批理由；NEVER 放宽既有判定）。

use std::sync::{Arc, Mutex};

use super::*;

/// 可编排 fake 评分端口：返回固定 p_true 或失败；记录收到的题目。
struct FakeNoulScoring {
    p_true: f64,
    fail: bool,
    captured: Mutex<Vec<systemone::ScoringQuestion>>,
}

#[async_trait::async_trait]
impl systemone::ScoringPort for FakeNoulScoring {
    async fn answer(
        &self,
        _state: &systemone::ScoringState,
        questions: &[systemone::ScoringQuestion],
    ) -> Result<Vec<systemone::ScoringAnswer>, systemone::ScoringUnavailable> {
        self.captured
            .lock()
            .expect("capture 锁")
            .extend(questions.iter().cloned());
        if self.fail {
            return Err(systemone::ScoringUnavailable::new(
                systemone::UnavailableKind::Server,
                "评分服务不可用",
            ));
        }
        Ok(vec![systemone::ScoringAnswer::noul(
            self.p_true,
            systemone::CalibrationLevel::Raw,
        )
        .expect("answer 构造")])
    }
}

fn triage_with(p_true: f64, fail: bool) -> (PolicyTriage, Arc<FakeNoulScoring>) {
    let fake = Arc::new(FakeNoulScoring {
        p_true,
        fail,
        captured: Mutex::new(Vec::new()),
    });
    (PolicyTriage::new(fake.clone()), fake)
}

fn sample_request() -> policy::PolicyRequestData {
    policy::PolicyRequestData::new(
        share::ids::RunId::new_v7(),
        share::ids::RunStepId::new_v7(),
        share::tools_vocab::ToolName::new("Bash"),
        share::tools_vocab::ToolCapabilities::single(share::tools_vocab::ToolCapability::Execute),
        "/tmp/triage-test",
    )
    .expect("request 构造")
}

/// 高风险（p_true ≥ 阈值）→ 生成含工具名的可读提级理由。
#[tokio::test]
async fn high_risk_tool_call_escalates() {
    let (triage, _fake) = triage_with(0.95, false);
    let escalation = triage
        .escalate(&sample_request())
        .await
        .expect("高风险应提级");
    assert!(
        escalation.reason.contains("Bash"),
        "提级理由应包含工具名：{}",
        escalation.reason
    );
}

/// 低风险（p_true < 阈值）→ 不提级（原 Allow 放行）。
#[tokio::test]
async fn low_risk_tool_call_passes_through() {
    let (triage, _fake) = triage_with(0.05, false);
    assert!(triage.escalate(&sample_request()).await.is_none());
}

/// 评分服务不可用 → 静默降级为不提级（NEVER 阻断工具执行主路径）。
#[tokio::test]
async fn scoring_unavailable_degrades_silently() {
    let (triage, _fake) = triage_with(0.99, true);
    assert!(triage.escalate(&sample_request()).await.is_none());
}

/// 阈值边界：恰等于阈值视为高风险（误放率优先，宁严勿松）。
#[tokio::test]
async fn threshold_boundary_counts_as_high_risk() {
    let threshold = f64::from(PolicyTriage::risk_threshold());
    let (triage, _fake) = triage_with(threshold, false);
    assert!(
        triage.escalate(&sample_request()).await.is_some(),
        "p_true == 阈值应提级"
    );
}

/// 题目口径：destructive / irreversible 单问句（与 permission_triage 数据集
/// 同款 instructions，保证离线对分与生产一致），state 携带工具与能力。
#[tokio::test]
async fn noul_question_uses_permission_triage_wording() {
    let (triage, fake) = triage_with(0.0, false);
    let _ = triage.escalate(&sample_request()).await;
    let captured = fake.captured.lock().expect("capture 锁").clone();
    assert_eq!(captured.len(), 1, "单题单问");
    match &captured[0] {
        systemone::ScoringQuestion::Noul { instructions, .. } => {
            assert!(instructions.contains("destructive or irreversible"));
        }
        other => panic!("必须使用 noul 题型，实际 {other:?}"),
    }
}
