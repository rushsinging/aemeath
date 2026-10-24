//! 经验库契约测试：证据不变量、长度上限、追加/读取/退役、损坏行容忍。

use crate::constants::{EXCERPT_MAX_CHARS, LESSON_MAX_CHARS};
use crate::domain::lesson::{EvidenceBinding, LessonEntry, LessonStatus, LessonStore, SignalKind};

fn entry(lesson: &str) -> Result<LessonEntry, crate::domain::errors::HarnessError> {
    LessonEntry::new(
        "id-1".to_string(),
        "skill",
        lesson,
        vec![EvidenceBinding {
            run_ref: "cycle-1".to_string(),
            signal: SignalKind::CycleObservation,
            excerpt: "组件正文缺失触发词".to_string(),
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
        }],
        1000,
    );
    assert!(result.is_err());
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
