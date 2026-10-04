//! Reminder 统一管线的领域层：kind / 广义 policy / source 契约 / Run 级队列。
//!
//! 设计真相源：`docs/design/02-modules/context-management/07-reminder-pipeline.md`。
//! 管线归 Context、事实来源归 Runtime：本模块只含纯逻辑（无 IO、无时钟），
//! 时间戳与渲染语言由 application 层在注入时提供。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::domain::constants::{
    ENVELOPE_VERSION, KIND_MEMORY_UPDATED, KIND_TASK_PROGRESS, PRIORITY_ENVIRONMENT,
    PRIORITY_EVENT, PRIORITY_TASK_STATE,
};

/// reminder kind 开放标识：新增 kind 只注册新 source，NEVER 扩展闭合 enum。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReminderKind(Arc<str>);

impl ReminderKind {
    pub fn new(value: impl Into<Arc<str>>) -> Self {
        Self(value.into())
    }

    pub fn task_progress() -> Self {
        Self::new(KIND_TASK_PROGRESS)
    }

    pub fn memory_updated() -> Self {
        Self::new(KIND_MEMORY_UPDATED)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 事件源标识（如 memory、background_task）：`OnEvent` 触发的来源句柄。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReminderEventSource(Arc<str>);

impl ReminderEventSource {
    pub fn new(value: impl Into<Arc<str>>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 何时 build reminder。
///
/// 变体名 `On*` 前缀是 07-reminder-pipeline.md 定稿的统一术语
/// （触发时机词汇族），NEVER 为满足机械 lint 规则拆散。
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshTrigger {
    OnRunStart,
    OnCompact,
    OnTaskMutation,
    /// 稳定 step 间隔周期重注入（对抗注意力衰减）。
    OnStepInterval(u32),
    /// 指定事件源到达时入队。
    OnEvent(ReminderEventSource),
}

impl RefreshTrigger {
    /// 动态触发（Run 内内容会变）的 kind MUST 使用 `TailUserMessage`。
    fn is_dynamic(&self) -> bool {
        matches!(self, Self::OnStepInterval(_) | Self::OnEvent(_))
    }
}

/// 注入位置：尾部 pending user message（默认）或 system block 尾部。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReminderPlacement {
    TailUserMessage,
    SystemTail,
}

/// 去重模式：`SkipIfUnchanged`（默认）与最近已注入 fingerprint 相同则不注入。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReminderDedup {
    SkipIfUnchanged,
    AlwaysInject,
}

/// 注入优先级：数值越大越先注入、越晚被预算截断。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReminderPriority(i32);

impl ReminderPriority {
    pub const fn event() -> Self {
        Self(PRIORITY_EVENT)
    }

    pub const fn task_state() -> Self {
        Self(PRIORITY_TASK_STATE)
    }

    pub const fn environment() -> Self {
        Self(PRIORITY_ENVIRONMENT)
    }

    pub const fn value(self) -> i32 {
        self.0
    }
}

/// 注入行为：去重模式 + 优先级。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InjectBehavior {
    pub dedup: ReminderDedup,
    pub priority: ReminderPriority,
}

/// auto-compact `Committed` 后的队列处置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactBehavior {
    /// 清空旧 entry，下次注入前从 source 现场重建。
    Rebuild,
    /// 保留最近快照，compact 后原样重新入队。
    Reinstate,
    /// 丢弃且本 Run 内同 kind 封锁。
    Drop,
}

/// 广义策略四维一体声明；注入机制只解释策略，NEVER 按 kind 硬编码。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderPolicy {
    pub refresh: RefreshTrigger,
    pub placement: ReminderPlacement,
    pub inject: InjectBehavior,
    pub compact: CompactBehavior,
}

impl ReminderPolicy {
    /// 缓存不变量：动态 refresh（OnStepInterval / OnEvent）MUST NOT 使用 SystemTail。
    pub fn is_valid(&self) -> bool {
        !(self.refresh.is_dynamic() && self.placement == ReminderPlacement::SystemTail)
    }
}

/// source 私有的中性快照数据：队列只存它，渲染由 source 按语言完成。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderSnapshot {
    pub data: String,
}

/// reminder 来源的唯一扩展点：新增 kind 只实现本 trait 并注册，
/// NEVER 触碰队列、注入调度或渲染分发。
pub trait ReminderSource: Send + Sync {
    fn kind(&self) -> ReminderKind;
    fn policy(&self) -> ReminderPolicy;
    /// 读当前快照；`None` 表示本轮无内容（如当前无任务），不入队。
    fn build(&self) -> Option<ReminderSnapshot>;
    /// 按语言渲染 body。
    fn render(&self, snapshot: &ReminderSnapshot, language: &str) -> String;
}

/// 队列 entry：kind + 快照 + 内容指纹 + 序号 + 注入行为。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderQueueEntry {
    pub kind: ReminderKind,
    pub snapshot: ReminderSnapshot,
    pub fingerprint: ReminderFingerprint,
    pub seq: u64,
    pub inject: InjectBehavior,
}

/// 内容指纹（kind + 快照内容）：dedup 与滞留重试的身份依据。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReminderFingerprint(String);

impl ReminderFingerprint {
    fn of(kind: &ReminderKind, snapshot: &ReminderSnapshot) -> Self {
        // FNV-1a：域内稳定指纹，无密码学需求。
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in kind.as_str().bytes().chain(snapshot.data.bytes()) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
        Self(format!("{hash:016x}"))
    }
}

/// 一次注入的候选：从队列 drain 出的待注入 entry 视图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InjectionCandidate {
    pub kind: ReminderKind,
    pub snapshot: ReminderSnapshot,
    pub fingerprint: ReminderFingerprint,
    pub seq: u64,
    pub inject: InjectBehavior,
}

/// Run 级 reminder 队列：快照替换 / 事件累积 / dedup / compact 处置 / 滞留。
///
/// 生命周期由 application 层句柄管理（Run 启动创建、结束销毁）；
/// 本类型非线程安全，跨线程访问经句柄串行化。
#[derive(Debug, Default)]
pub struct ReminderQueue {
    entries: Vec<ReminderQueueEntry>,
    staging: Vec<ReminderQueueEntry>,
    last_injected: HashMap<ReminderKind, ReminderFingerprint>,
    last_snapshot: HashMap<ReminderKind, ReminderQueueEntry>,
    rebuild_flags: HashSet<ReminderKind>,
    dropped_kinds: HashSet<ReminderKind>,
    next_seq: u64,
}

impl ReminderQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// 快照类入队：同 kind 未消费旧 entry 被最新替换。
    pub fn push_snapshot(
        &mut self,
        kind: ReminderKind,
        snapshot: ReminderSnapshot,
        inject: InjectBehavior,
    ) {
        if self.dropped_kinds.contains(&kind) {
            return;
        }
        self.entries.retain(|entry| entry.kind != kind);
        self.append_entry(kind, snapshot, inject);
    }

    /// 事件类入队：同 kind entry 各自累积（各自独立注入块）。
    pub fn push_event(
        &mut self,
        kind: ReminderKind,
        snapshot: ReminderSnapshot,
        inject: InjectBehavior,
    ) {
        if self.dropped_kinds.contains(&kind) {
            return;
        }
        self.append_entry(kind, snapshot, inject);
    }

    /// auto-compact `Committed` 后按 per-kind 处置；`Skipped` 不调用本方法。
    ///
    /// compact 将历史替换为 summary 后，模型「已注入过该内容」的前提失效，
    /// 因此 Rebuild / Reinstate 都作废该 kind 的最近注入指纹（dedup 拦截不再成立）。
    pub fn apply_compact_outcome(&mut self, outcomes: &[(ReminderKind, CompactBehavior)]) {
        for (kind, behavior) in outcomes {
            match behavior {
                CompactBehavior::Rebuild => {
                    self.entries.retain(|entry| &entry.kind != kind);
                    self.staging.retain(|entry| &entry.kind != kind);
                    self.last_injected.remove(kind);
                    self.rebuild_flags.insert(kind.clone());
                }
                CompactBehavior::Reinstate => {
                    self.last_injected.remove(kind);
                    if let Some(latest) = self.last_snapshot.get(kind).cloned() {
                        if !self.entries.iter().any(|entry| &entry.kind == kind) {
                            self.entries.push(latest);
                            self.sort_entries();
                        }
                    }
                }
                CompactBehavior::Drop => {
                    self.entries.retain(|entry| &entry.kind != kind);
                    self.staging.retain(|entry| &entry.kind != kind);
                    self.last_snapshot.remove(kind);
                    self.dropped_kinds.insert(kind.clone());
                }
            }
        }
    }

    /// compact `Rebuild` 标记：注入前 MUST 从 source 现场重建（NEVER 注入陈旧快照）。
    pub fn needs_rebuild(&self, kind: &ReminderKind) -> bool {
        self.rebuild_flags.contains(kind)
    }

    /// 消费 `Rebuild` 标记（source 重建完成后调用）。
    pub fn clear_rebuild_flag(&mut self, kind: &ReminderKind) {
        self.rebuild_flags.remove(kind);
    }

    /// 取本轮注入候选：过滤 Drop、SkipIfUnchanged 去重、按 priority 降序；
    /// 候选移入 staging，经 [`Self::confirm_injection`] / [`Self::defer_injection`] 收敛。
    pub fn begin_injection(&mut self) -> Vec<InjectionCandidate> {
        let mut retained = Vec::new();
        for entry in std::mem::take(&mut self.entries) {
            let unchanged = self
                .last_injected
                .get(&entry.kind)
                .is_some_and(|previous| *previous == entry.fingerprint);
            let skip = entry.inject.dedup == ReminderDedup::SkipIfUnchanged && unchanged;
            if skip {
                self.last_snapshot.insert(entry.kind.clone(), entry);
            } else {
                retained.push(entry);
            }
        }
        retained.sort_by(|left, right| {
            right
                .inject
                .priority
                .value()
                .cmp(&left.inject.priority.value())
                .then_with(|| left.seq.cmp(&right.seq))
        });
        self.staging = retained;
        self.staging
            .iter()
            .map(|entry| InjectionCandidate {
                kind: entry.kind.clone(),
                snapshot: entry.snapshot.clone(),
                fingerprint: entry.fingerprint.clone(),
                seq: entry.seq,
                inject: entry.inject,
            })
            .collect()
    }

    /// 确认注入：staging 中对应 seq 的 entry 记为已注入（更新指纹），其余留在 staging 等待 defer。
    pub fn confirm_injection(&mut self, injected_seqs: &[u64]) {
        let injected: HashSet<u64> = injected_seqs.iter().copied().collect();
        self.staging.retain(|entry| {
            if injected.contains(&entry.seq) {
                self.last_injected
                    .insert(entry.kind.clone(), entry.fingerprint.clone());
                self.last_snapshot.insert(entry.kind.clone(), entry.clone());
                false
            } else {
                true
            }
        });
    }

    /// 预算截断滞留：staging 中被截断的 entry 回队（保 fingerprint），下一轮优先。
    pub fn defer_injection(&mut self, deferred_seqs: &[u64]) {
        let deferred: HashSet<u64> = deferred_seqs.iter().copied().collect();
        self.staging.retain(|entry| {
            if deferred.contains(&entry.seq) {
                self.entries.push(entry.clone());
                false
            } else {
                true
            }
        });
        self.sort_entries();
    }

    fn append_entry(
        &mut self,
        kind: ReminderKind,
        snapshot: ReminderSnapshot,
        inject: InjectBehavior,
    ) {
        self.next_seq += 1;
        let fingerprint = ReminderFingerprint::of(&kind, &snapshot);
        let entry = ReminderQueueEntry {
            kind: kind.clone(),
            snapshot,
            fingerprint,
            seq: self.next_seq,
            inject,
        };
        self.last_snapshot.insert(kind, entry.clone());
        self.entries.push(entry);
        self.sort_entries();
    }

    fn sort_entries(&mut self) {
        self.entries.sort_by(|left, right| {
            right
                .inject
                .priority
                .value()
                .cmp(&left.inject.priority.value())
                .then_with(|| left.seq.cmp(&right.seq))
        });
    }
}

/// 统一 envelope 输入：kind / body / 时间戳 / 序号。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderEnvelopeInput {
    pub kind: String,
    pub body: String,
    pub at: String,
    pub seq: u64,
}

/// 渲染统一 envelope：日志关联、TUI 剥离、dedup 与 compact 重建识别都依赖它。
pub fn compose_reminder_envelope(input: &ReminderEnvelopeInput) -> String {
    format!(
        "<system-reminder kind=\"{}\" version=\"{}\" at=\"{}\" seq=\"{}\">\n{}\n</system-reminder>",
        input.kind.replace('_', "-"),
        ENVELOPE_VERSION,
        input.at,
        input.seq,
        input.body
    )
}

/// 多 reminder 拼装：合并为单条尾部 user message（NEVER 相邻 user-user 轮次）。
pub fn compose_tail_user_message(blocks: &[String]) -> String {
    blocks.join("\n")
}

/// reminder 文案的 HTML 转义（subject / model id 等不可信文本）。
pub(crate) fn escape_reminder_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// 既有 4 类 reminder（`InvocationReminderData` 载体）的 body 渲染：
/// zh/en 双语、不含 envelope 包裹（envelope 由
/// [`compose_reminder_envelope`] 统一添加）。
///
/// runtime 侧 source 的 `render` 经 serde 反序列化后委托本函数，
/// 保持文案单一真相在 Context。
pub fn render_invocation_reminder_body(
    data: &crate::domain::InvocationReminderData,
    language: &str,
) -> String {
    match data {
        crate::domain::InvocationReminderData::TaskProgress(progress) => {
            let mut lines = vec![match language {
                "zh" => format!("━━ 任务：{}/{} ━━", progress.completed, progress.total),
                _ => format!("━━ Tasks: {}/{} ━━", progress.completed, progress.total),
            }];
            for item in &progress.items {
                let status = match item.status {
                    crate::domain::TaskProgressStatus::Completed => "✓",
                    crate::domain::TaskProgressStatus::InProgress => "■",
                    crate::domain::TaskProgressStatus::Pending => "□",
                };
                let blocked = if item.blocked_by_sequences.is_empty() {
                    String::new()
                } else {
                    let sequences = item
                        .blocked_by_sequences
                        .iter()
                        .map(u64::to_string)
                        .collect::<Vec<_>>()
                        .join(", ");
                    match language {
                        "zh" => format!("（被 #{sequences} 阻塞）"),
                        _ => format!(" (blocked by #{sequences})"),
                    }
                };
                lines.push(format!(
                    "{status} #{} {}{blocked}",
                    item.sequence,
                    escape_reminder_text(&item.subject)
                ));
            }
            if progress.hidden_count > 0 {
                lines.push(match language {
                    "zh" => format!("另有 {} 个任务未显示", progress.hidden_count),
                    _ => format!("{} additional tasks are omitted", progress.hidden_count),
                });
            }
            match language {
                "zh" => format!("当前任务进度：\n{}", lines.join("\n")),
                _ => format!("Current task progress:\n{}", lines.join("\n")),
            }
        }
        crate::domain::InvocationReminderData::GuidanceSourcesChanged => match language {
            "zh" => "guidance 来源已变更；当前 Session 的冻结系统提示保持不变。新 Session 才会重新物化这些来源。".to_string(),
            _ => "Guidance sources changed. This Session's frozen system prompt remains unchanged; a new Session will materialize the updated sources.".to_string(),
        },
        crate::domain::InvocationReminderData::ModelGuidanceMismatch {
            session_model_id,
            run_model_id,
        } => match language {
            "zh" => format!(
                "Session 冻结模型 {} 与当前 Run 模型 {} 不同；继续使用 Session 冻结的系统提示。",
                escape_reminder_text(session_model_id),
                escape_reminder_text(run_model_id)
            ),
            _ => format!(
                "The Session-frozen model {} differs from the current Run model {}; continue using the Session-frozen system prompt.",
                escape_reminder_text(session_model_id),
                escape_reminder_text(run_model_id)
            ),
        },
        crate::domain::InvocationReminderData::MemoryUpdated { changed } => match language {
            "zh" => format!(
                "记忆已更新 {changed} 条；需要最新内容时用 memory tool 的 list / search 查看，不要凭记忆假设。"
            ),
            _ => format!(
                "Memory was updated ({changed} entries). Use the memory tool's list / search actions to read the current content instead of assuming what it says."
            ),
        },
    }
}
