//! 经验库契约测试：证据不变量、长度上限、追加/读取/退役、损坏行容忍。

use crate::constants::{EXCERPT_MAX_CHARS, LESSON_MAX_CHARS};
use crate::domain::lesson::{
    EvidenceBinding, LessonEntry, LessonStatus, LessonStore, SignalKind, VerificationStatus,
};

fn entry(lesson: &str) -> Result<LessonEntry, crate::domain::errors::HarnessError> {
    LessonEntry::new(
        "id-1".to_string(),
        "skill",
        lesson,
        vec![EvidenceBinding {
            run_ref: "cycle-1".to_string(),
            signal: SignalKind::CycleObservation,
            excerpt: "组件正文缺失触发词".to_string(),
            signal_id: String::new(),
            source: None,
        }],
        1000,
    )
}

/// 无证据的条目 MUST 被拒（证据绑定不变量）。
#[test]
fn entry_without_evidence_rejected() {
    let error = LessonEntry::new("id-1".to_string(), "skill", "结论", vec![], 1000)
        .expect_err("无证据必须拒绝");
    assert!(error.message().contains("证据"));
}

/// lesson 超长 MUST 被拒；恰好等长通过。
#[test]
fn entry_enforces_lesson_length_bound() {
    let long: String = "长".repeat(LESSON_MAX_CHARS + 1);
    assert!(entry(&long).is_err());
    let exact: String = "长".repeat(LESSON_MAX_CHARS);
    assert!(entry(&exact).is_ok());
}

/// 证据摘录超长 MUST 被拒。
#[test]
fn entry_enforces_excerpt_length_bound() {
    let result = LessonEntry::new(
        "id-1".to_string(),
        "skill",
        "结论",
        vec![EvidenceBinding {
            run_ref: "cycle-1".to_string(),
            signal: SignalKind::CycleObservation,
            excerpt: "摘".repeat(EXCERPT_MAX_CHARS + 1),
            signal_id: String::new(),
            source: None,
        }],
        1000,
    );
    assert!(result.is_err());
}

/// 旧 EvidenceBinding JSON 无 signal_id/source 时兼容读取；新字段可 round-trip。
#[test]
fn evidence_binding_source_fields_default_and_round_trip() {
    let legacy: EvidenceBinding = serde_json::from_str(
        r#"{"run_ref":"cycle-1","signal":"cycle_observation","excerpt":"旧证据"}"#,
    )
    .expect("旧证据绑定必须兼容");
    assert!(legacy.signal_id.is_empty());
    assert!(legacy.source.is_none());

    let current = EvidenceBinding {
        run_ref: "cycle-1".to_string(),
        signal: SignalKind::RepeatedFailure,
        excerpt: "重复失败".to_string(),
        signal_id: "repeated_failure:Bash:x".to_string(),
        source: Some(crate::domain::evidence::EvidenceSourceRef {
            session_id: "s1.dataset".to_string(),
            run_id: "run-1".to_string(),
            step_id: "step-1".to_string(),
        }),
    };
    let serialized = serde_json::to_string(&current).expect("serialize");
    let parsed: EvidenceBinding = serde_json::from_str(&serialized).expect("round-trip");
    assert_eq!(parsed, current);
}

/// JSONL 往返：追加→读取等价；Active 过滤正确。
#[test]
fn store_append_load_round_trip() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = crate::domain::lesson::LessonStore::new(temp.path().join("library.jsonl"));
    let first = entry("技能应声明触发词").expect("entry");
    let second = entry("guardrails 保持简短").expect("entry");
    store.append(&first).expect("append");
    store.append(&second).expect("append");

    let loaded = store.load_all().expect("load");
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0], first);
    assert_eq!(store.load_active().expect("active").len(), 2);
}

/// retire 语义：目标条目转 Retired、其余不动；未知 id 返回 false。
#[test]
fn store_retire_marks_single_entry() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = crate::domain::lesson::LessonStore::new(temp.path().join("library.jsonl"));
    let first = entry("第一条").expect("entry");
    let mut second = entry("第二条").expect("second");
    second.id = "id-2".to_string();
    store.append(&first).expect("append");
    store.append(&second).expect("append");

    assert!(store.retire("id-2").expect("retire"));
    let active = store.load_active().expect("active");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, "id-1");
    let all = store.load_all().expect("all");
    assert_eq!(all[1].status, LessonStatus::Retired);

    assert!(!store.retire("id-404").expect("retire unknown"));
}

/// 损坏行跳过（warn），NEVER 整库失败。
#[test]
fn store_skips_corrupt_lines() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("library.jsonl");
    std::fs::write(&path, "{bad json}\n").expect("seed corrupt");
    let store = crate::domain::lesson::LessonStore::new(&path);
    let good = entry("好条目").expect("entry");
    store.append(&good).expect("append");

    let loaded = store.load_all().expect("损坏行 NEVER 拖垮整库");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].lesson, "好条目");
}

/// 空库（文件不存在）返回空列表。
#[test]
fn store_missing_file_yields_empty() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = crate::domain::lesson::LessonStore::new(temp.path().join("none.jsonl"));
    assert!(store.load_all().expect("missing").is_empty());
    assert!(store.load_active().expect("missing active").is_empty());
}

// ---- #2065 B：组件级 lesson 退役 ----

#[test]
fn retire_for_component_supersedes_old_active_lessons() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = LessonStore::new(dir.path().join("lib.jsonl"));
    // 旧周期的 guardrails/default lesson（Active）
    let old_lesson = LessonEntry::new(
        "cycle-1:guardrails/default".to_string(),
        "guardrails",
        "旧结论".to_string(),
        vec![EvidenceBinding {
            run_ref: "cycle-1".to_string(),
            signal: SignalKind::CycleObservation,
            excerpt: "e".to_string(),
            signal_id: String::new(),
            source: None,
        }],
        1000,
    )
    .expect("构造");
    store.append(&old_lesson).expect("append");
    // 其他组件的 lesson（不应被退役）
    let other = LessonEntry::new(
        "cycle-1:skill/alpha".to_string(),
        "skill",
        "别的组件".to_string(),
        vec![EvidenceBinding {
            run_ref: "cycle-1".to_string(),
            signal: SignalKind::CycleObservation,
            excerpt: "e".to_string(),
            signal_id: String::new(),
            source: None,
        }],
        1000,
    )
    .expect("构造");
    store.append(&other).expect("append");

    let guardrails_id = crate::domain::component::ComponentId {
        kind: crate::domain::component::ComponentKind::Guardrails,
        name: crate::domain::component::ComponentName::new("default"),
    };
    let retired = store.retire_for_component(&guardrails_id).expect("retire");

    assert_eq!(retired, 1, "只退役目标组件");
    let active = store.load_active().expect("active");
    assert!(active
        .iter()
        .all(|entry| entry.id.ends_with(":skill/alpha")));
    assert!(active.len() == 1, "其他组件保留：{active:?}");
}

// ---- #2080 验证阶段：三态状态机与幂等 ----

fn pending_lesson(id: &str, target_signal: &str) -> LessonEntry {
    let mut entry = LessonEntry::new(
        id.to_string(),
        "guardrails",
        "规则结论".to_string(),
        vec![EvidenceBinding {
            run_ref: "cycle-1".to_string(),
            signal: SignalKind::RepeatedFailure,
            excerpt: "e".to_string(),
            signal_id: String::new(),
            source: None,
        }],
        1000,
    )
    .expect("构造");
    entry.verification.target_signal = target_signal.to_string();
    entry
}

/// 三态：信号仍在 → Ineffective；信号消失 → Verified；无新证据 → 不判定。
#[test]
fn verification_state_machine_three_states() {
    let lesson = pending_lesson("l1", "repeated_failure:Bash:permission denied");

    let still = crate::domain::lesson::judge_verification(
        &lesson,
        &["repeated_failure:Bash:permission denied: /etc/shadow".to_string()],
        true,
    )
    .expect("应判定");
    assert_eq!(still.status, VerificationStatus::Ineffective);

    let gone = crate::domain::lesson::judge_verification(
        &lesson,
        &["retry_pattern:Edit:abc".to_string()],
        true,
    )
    .expect("应判定");
    assert_eq!(gone.status, VerificationStatus::Verified);

    // 无新证据：样本不足，NEVER 编造结论
    assert!(crate::domain::lesson::judge_verification(&lesson, &[], false).is_none());
}

/// 旧条目（无 target_signal）与已判定条目不参与验证。
#[test]
fn verification_skips_legacy_and_checked() {
    let legacy = pending_lesson("l2", "");
    assert!(crate::domain::lesson::judge_verification(&legacy, &[], true).is_none());

    let mut checked = pending_lesson("l3", "repeated_failure:Bash:x");
    checked.verification.status = VerificationStatus::Verified;
    checked.verification.checked_at = 12345;
    assert!(crate::domain::lesson::judge_verification(&checked, &[], true).is_none());
}

/// store 级：判定落盘 + 幂等（第二次不再判定）。
#[test]
fn verify_pending_persists_and_is_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = LessonStore::new(dir.path().join("library.jsonl"));
    store
        .append(&pending_lesson("l4", "repeated_failure:Bash:boom"))
        .expect("append");

    let first = store
        .verify_pending(&["repeated_failure:Bash:boom".to_string()], true, 5000)
        .expect("verify");
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].status, VerificationStatus::Ineffective);

    let stored = store.load_all().expect("load");
    assert_eq!(
        stored[0].verification.status,
        VerificationStatus::Ineffective
    );
    assert_eq!(stored[0].verification.checked_at, 5000);

    // 幂等：已判定条目不再参与
    let second = store
        .verify_pending(&["repeated_failure:Bash:boom".to_string()], true, 9000)
        .expect("verify again");
    assert!(second.is_empty());
}
