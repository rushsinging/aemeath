//! 证据强度门禁与 lesson review（#2092）。
//!
//! 两个职责：
//! 1. **强度门禁**：把「本周期证据」与「历史累计」合起来判定，未达阈值
//!    只观察不落盘——防止偶然一次失败就写永久规则。
//! 2. **lesson review**：过期清理（长期未被引用）、有效性复核（复用
//!    #2080 状态机）、按结论选择性退役（替代一刀切全量退役）。
//!
//! 跨周期累计的真相源是 checkpoint 里已处理的 run 与 lesson corpus，
//! 本模块只做纯判定（IO 由调用方注入），便于 L2 全覆盖。

use crate::domain::evidence::EvidenceSignal;
use crate::domain::lesson::{LessonEntry, LessonStatus, VerificationStatus};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 证据强度档位（#2092 拍板分档）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrengthTier {
    /// 观察：仅在 reason/报告提示，不落盘（单块 ×2）。
    Observe,
    /// 轻量落盘：写一条规则（×3–5）。
    Light,
    /// 强落盘：必然提案，且要求更具体的规则（≥6 或跨 ≥3 周期）。
    Strong,
    /// 强制重修：该问题曾被修过但仍复发——升级强度，不重复写同类规则。
    Rework,
}

impl StrengthTier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Observe => "observe",
            Self::Light => "light",
            Self::Strong => "strong",
            Self::Rework => "rework",
        }
    }

    /// 是否允许落盘（Observe 不允许）。
    pub fn allows_apply(self) -> bool {
        !matches!(self, Self::Observe)
    }
}

/// 分档阈值（可配置，默认对齐 #2092 拍板值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrengthThresholds {
    /// 轻量落盘的最小出现次数（默认 3）。
    pub light_min_occurrences: usize,
    /// 强落盘的最小出现次数（默认 6）。
    pub strong_min_occurrences: usize,
    /// 强落盘的最小跨周期数（默认 3）。
    pub strong_min_cycles: usize,
}

impl Default for StrengthThresholds {
    fn default() -> Self {
        Self {
            light_min_occurrences: 3,
            strong_min_occurrences: 6,
            strong_min_cycles: 3,
        }
    }
}

/// 单条信号的强度判定输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrengthInput {
    pub signal_id: String,
    /// 本周期出现次数。
    pub current_occurrences: usize,
    /// 历史累计出现次数（跨周期，含本周期）。
    pub cumulative_occurrences: usize,
    /// 该信号出现过的不同周期数（含本周期）。
    pub cycles_seen: usize,
    /// 该信号是否曾被改进过（存在绑定它的 lesson）。
    pub previously_addressed: bool,
    /// 上次改进是否被判 ineffective（#2080）。
    pub last_verification_ineffective: bool,
}

/// 强度判定结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrengthVerdict {
    pub signal_id: String,
    pub tier: StrengthTier,
    /// 人读依据（进 reason/报告）。
    pub rationale: String,
}

/// 判定单条信号的强度档位（纯函数）。
///
/// 优先级：强制重修 > 强落盘 > 轻量 > 观察。
pub fn judge_strength(input: &StrengthInput, thresholds: &StrengthThresholds) -> StrengthVerdict {
    let rationale = format!(
        "本周期 ×{}，累计 ×{}，跨 {} 周期{}",
        input.current_occurrences,
        input.cumulative_occurrences,
        input.cycles_seen,
        if input.previously_addressed {
            if input.last_verification_ineffective {
                "（曾修过且未生效）"
            } else {
                "（曾修过）"
            }
        } else {
            ""
        }
    );
    // 曾修过但仍复发（且未生效）→ 强制重修：重复写同类规则无效，
    // 需要升级强度。
    if input.previously_addressed && input.last_verification_ineffective {
        return StrengthVerdict {
            signal_id: input.signal_id.clone(),
            tier: StrengthTier::Rework,
            rationale,
        };
    }
    let tier = if input.cumulative_occurrences >= thresholds.strong_min_occurrences
        || input.cycles_seen >= thresholds.strong_min_cycles
    {
        StrengthTier::Strong
    } else if input.cumulative_occurrences >= thresholds.light_min_occurrences {
        StrengthTier::Light
    } else {
        StrengthTier::Observe
    };
    StrengthVerdict {
        signal_id: input.signal_id.clone(),
        tier,
        rationale,
    }
}

/// 跨周期累计统计（从历史证据摘要累计；调用方负责持久化）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalHistory {
    /// signal_id → 累计统计。
    pub entries: BTreeMap<String, SignalHistoryEntry>,
}

/// 单信号的历史统计。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalHistoryEntry {
    pub cumulative_occurrences: usize,
    pub cycles_seen: usize,
    /// 最后出现的周期 id（去重用：同周期重复累计只加次数不加班次）。
    pub last_cycle_id: String,
}

impl SignalHistory {
    /// 用本周期证据累计（同周期重复调用只累加次数，不重复计周期）。
    ///
    /// 同周期内同 signal_id 只取**最大** occurrences（多个数据源块各自
    /// 聚合同一信号时，重复相加会虚增强度——#2092 实测缺陷）。
    pub fn accumulate(&mut self, cycle_id: &str, signals: &[EvidenceSignal]) {
        let mut per_signal: BTreeMap<&str, usize> = BTreeMap::new();
        for signal in signals {
            let occurrences = signal.occurrences.max(1);
            per_signal
                .entry(signal.signal_id.as_str())
                .and_modify(|existing| *existing = (*existing).max(occurrences))
                .or_insert(occurrences);
        }
        for (signal_id, occurrences) in per_signal {
            let signal = EvidenceSignal {
                kind: crate::domain::evidence::SignalKind::RepeatedFailure,
                signal_id: signal_id.to_string(),
                summary: String::new(),
                excerpt: None,
                occurrences,
                sources: Vec::new(),
            };
            self.accumulate_one(cycle_id, &signal);
        }
    }

    fn accumulate_one(&mut self, cycle_id: &str, signal: &EvidenceSignal) {
        {
            let entry = self
                .entries
                .entry(signal.signal_id.clone())
                .or_insert_with(|| SignalHistoryEntry {
                    cumulative_occurrences: 0,
                    cycles_seen: 0,
                    last_cycle_id: cycle_id.to_string(),
                });
            entry.cumulative_occurrences += signal.occurrences.max(1);
            if entry.last_cycle_id != cycle_id {
                entry.cycles_seen += 1;
                entry.last_cycle_id = cycle_id.to_string();
            } else if entry.cycles_seen == 0 {
                entry.cycles_seen = 1;
            }
        }
    }

    /// 组装判定输入（结合 lesson 历史判断「曾修过」与「未生效」）。
    pub fn build_input(
        &self,
        cycle_id: &str,
        signal: &EvidenceSignal,
        lessons: &[LessonEntry],
    ) -> StrengthInput {
        let entry = self.entries.get(&signal.signal_id);
        let related: Vec<&LessonEntry> = lessons
            .iter()
            .filter(|lesson| lesson.verification.target_signal == signal.signal_id)
            .collect();
        let previously_addressed = !related.is_empty();
        let last_verification_ineffective = related.iter().any(|lesson| {
            lesson.verification.status == VerificationStatus::Ineffective
                && lesson.status == LessonStatus::Active
        });
        StrengthInput {
            signal_id: signal.signal_id.clone(),
            current_occurrences: signal.occurrences.max(1),
            cumulative_occurrences: entry
                .map(|entry| entry.cumulative_occurrences)
                .unwrap_or(signal.occurrences.max(1)),
            cycles_seen: entry.map(|entry| entry.cycles_seen.max(1)).unwrap_or(1),
            previously_addressed,
            last_verification_ineffective,
        }
        .tap_cycle(cycle_id)
    }
}

/// 内部小工具：保留 cycle_id 供未来日志（当前不影响判定）。
trait TapCycle {
    fn tap_cycle(self, cycle_id: &str) -> Self;
}

impl TapCycle for StrengthInput {
    fn tap_cycle(self, _cycle_id: &str) -> Self {
        self
    }
}

/// lesson review 判定（#2092）：过期清理 + 选择性退役。
///
/// - 长期（`stale_after_cycles`）未被任何周期引用的 Active lesson →
///   Stale 退役（NEVER 无界保留）
/// - 已 ineffective 且无新价值 → 由调用方决定是否退役（本函数只标记）
pub fn review_lessons(
    lessons: &[LessonEntry],
    current_cycle_index: u64,
    stale_after_cycles: u64,
    referenced_signals: &[String],
) -> Vec<LessonReviewOutcome> {
    let mut outcomes = Vec::new();
    for lesson in lessons {
        if lesson.status != LessonStatus::Active {
            continue;
        }
        // 过期：未被本周期引用，且距上次确认已超阈值。
        // 用 adopted_at 与当前周期序的近似关系（调用方传入周期序号）。
        let referenced = referenced_signals
            .iter()
            .any(|signal| signal == &lesson.verification.target_signal);
        if !referenced
            && lesson.verification.checked_at > 0
            && current_cycle_index.saturating_sub(lesson.verification.checked_at)
                >= stale_after_cycles
        {
            outcomes.push(LessonReviewOutcome {
                lesson_id: lesson.id.clone(),
                action: ReviewAction::RetireStale,
                rationale: format!(
                    "超过 {stale_after_cycles} 周期未被引用（上次判定 {}）",
                    lesson.verification.checked_at
                ),
            });
        }
    }
    outcomes
}

/// review 动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewAction {
    /// 过期退役。
    RetireStale,
    /// 保留（无效但仍需作为重修输入）。
    Keep,
}

/// 单条 review 结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LessonReviewOutcome {
    pub lesson_id: String,
    pub action: ReviewAction,
    pub rationale: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::evidence::SignalKind;
    use crate::domain::lesson::{EvidenceBinding, LessonStore};

    fn signal(id: &str, occurrences: usize) -> EvidenceSignal {
        EvidenceSignal {
            kind: SignalKind::RepeatedFailure,
            signal_id: id.to_string(),
            summary: "s".to_string(),
            excerpt: None,
            occurrences,
            sources: Vec::new(),
        }
    }

    fn base_input(signal_id: &str) -> StrengthInput {
        StrengthInput {
            signal_id: signal_id.to_string(),
            current_occurrences: 2,
            cumulative_occurrences: 2,
            cycles_seen: 1,
            previously_addressed: false,
            last_verification_ineffective: false,
        }
    }

    /// 分档：×2 观察 / ×3–5 轻量 / ≥6 或跨 ≥3 周期强落盘。
    #[test]
    fn strength_tiers_by_occurrences_and_cycles() {
        let thresholds = StrengthThresholds::default();

        let observe = judge_strength(&base_input("s1"), &thresholds);
        assert_eq!(observe.tier, StrengthTier::Observe);
        assert!(!observe.tier.allows_apply(), "观察档不落盘");

        let mut light = base_input("s2");
        light.cumulative_occurrences = 4;
        assert_eq!(
            judge_strength(&light, &thresholds).tier,
            StrengthTier::Light
        );

        let mut strong = base_input("s3");
        strong.cumulative_occurrences = 6;
        assert_eq!(
            judge_strength(&strong, &thresholds).tier,
            StrengthTier::Strong
        );

        // 跨周期达标同样强落盘（次数少但分散）
        let mut spread = base_input("s4");
        spread.cumulative_occurrences = 3;
        spread.cycles_seen = 3;
        assert_eq!(
            judge_strength(&spread, &thresholds).tier,
            StrengthTier::Strong
        );
    }

    /// 曾修过且未生效 → 强制重修（优先于其它档位）。
    #[test]
    fn rework_takes_precedence() {
        let thresholds = StrengthThresholds::default();
        let mut input = base_input("s5");
        input.previously_addressed = true;
        input.last_verification_ineffective = true;
        input.cumulative_occurrences = 10;
        let verdict = judge_strength(&input, &thresholds);
        assert_eq!(verdict.tier, StrengthTier::Rework);
        assert!(verdict.rationale.contains("曾修过且未生效"));
    }

    /// 跨周期累计：同周期重复只加次数，跨周期才加班次。
    #[test]
    fn history_accumulates_across_cycles_not_within() {
        let mut history = SignalHistory::default();
        history.accumulate("cycle-1", &[signal("sig-a", 2)]);
        history.accumulate("cycle-1", &[signal("sig-a", 3)]);
        let entry = history.entries.get("sig-a").expect("entry");
        assert_eq!(entry.cumulative_occurrences, 5);
        assert_eq!(entry.cycles_seen, 1, "同周期不重复计班次");

        history.accumulate("cycle-2", &[signal("sig-a", 1)]);
        let entry = history.entries.get("sig-a").expect("entry");
        assert_eq!(entry.cumulative_occurrences, 6);
        assert_eq!(entry.cycles_seen, 2);
    }

    /// 同周期多块重复同一 signal_id：取最大 occurrences，NEVER 相加。
    #[test]
    fn accumulate_dedupes_within_cycle() {
        let mut history = SignalHistory::default();
        // 两个块各自聚合出同一条信号（×2）
        history.accumulate("cycle-1", &[signal("sig-dup", 2), signal("sig-dup", 2)]);
        let entry = history.entries.get("sig-dup").expect("entry");
        assert_eq!(entry.cumulative_occurrences, 2, "同周期重复不相加");
        assert_eq!(entry.cycles_seen, 1);
    }

    /// 分散在多个周期的同类失败最终达到强落盘（此前永不聚合的缺陷）。
    #[test]
    fn dispersed_occurrences_reach_strong_tier() {
        let mut history = SignalHistory::default();
        let thresholds = StrengthThresholds::default();
        for (index, cycle) in ["c1", "c2", "c3"].iter().enumerate() {
            history.accumulate(cycle, &[signal("sig-b", 1)]);
            let input = history.build_input(cycle, &signal("sig-b", 1), &[]);
            let verdict = judge_strength(&input, &thresholds);
            if index < 2 {
                assert_eq!(
                    verdict.tier,
                    StrengthTier::Observe,
                    "第 {} 次仍观察",
                    index + 1
                );
            } else {
                assert_eq!(verdict.tier, StrengthTier::Strong, "跨 3 周期 → 强落盘");
            }
        }
    }

    /// 曾修过且 ineffective 的 lesson 让信号升级为强制重修。
    #[test]
    fn ineffective_lesson_upgrades_to_rework() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = LessonStore::new(dir.path().join("lib.jsonl"));
        let mut lesson = LessonEntry::new(
            "l1".to_string(),
            "guardrails",
            "结论".to_string(),
            vec![EvidenceBinding {
                run_ref: "c1".to_string(),
                signal: SignalKind::RepeatedFailure,
                excerpt: "e".to_string(),
                signal_id: String::new(),
                source: None,
            }],
            1,
        )
        .expect("构造");
        lesson.verification.target_signal = "sig-c".to_string();
        lesson.verification.status = VerificationStatus::Ineffective;
        store.append(&lesson).expect("append");

        let mut history = SignalHistory::default();
        history.accumulate("c2", &[signal("sig-c", 1)]);
        let input = history.build_input("c2", &signal("sig-c", 1), &[lesson]);
        assert!(input.previously_addressed);
        assert!(input.last_verification_ineffective);
        assert_eq!(
            judge_strength(&input, &StrengthThresholds::default()).tier,
            StrengthTier::Rework
        );
    }

    /// 过期清理：超阈值未被引用的 Active lesson → RetireStale。
    #[test]
    fn review_retires_stale_lessons() {
        let mut lesson = LessonEntry::new(
            "l2".to_string(),
            "guardrails",
            "结论".to_string(),
            vec![EvidenceBinding {
                run_ref: "c1".to_string(),
                signal: SignalKind::RepeatedFailure,
                excerpt: "e".to_string(),
                signal_id: String::new(),
                source: None,
            }],
            1,
        )
        .expect("构造");
        lesson.verification.target_signal = "sig-old".to_string();
        lesson.verification.checked_at = 100;

        // 距上次判定 20 周期、未被引用 → 退役
        let outcomes = review_lessons(&[lesson.clone()], 120, 10, &[]);
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].action, ReviewAction::RetireStale);

        // 被引用 → 不退役
        let kept = review_lessons(&[lesson], 120, 10, &["sig-old".to_string()]);
        assert!(kept.is_empty());
    }
}
