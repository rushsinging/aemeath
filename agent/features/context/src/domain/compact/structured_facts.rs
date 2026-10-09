use super::{
    is_compact_protocol_text, CheckpointError, CheckpointSections, ContinuationCheckpoint,
    ContinuationStatus,
};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactFactSource {
    MainUser,
    AssistantReport,
    ToolInvocation,
    ToolResult,
    SystemGenerated,
    SubagentInstruction,
    /// 上一份 checkpoint 的确定性回填（compact 级联通道）。
    ///
    /// 内部来源，不进入 LLM prompt 的 source 清单；其原始权威性已在更早一轮
    /// 归并时确认，级联时必须按对应 kind 的权威来源等价保留，否则每轮 compact
    /// 都会把已确立事实逐级降级（约束落进 scope unverified，已提交事实被标
    /// unverified，objective / resume cursor 被静默丢弃）。
    Checkpoint,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintScope {
    Session,
    TaskData,
    Phase,
    ToolCall,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintLifecycle {
    Persistent,
    UntilTaskEnd,
    UntilPhaseEnd,
    UntilToolCallEnd,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintAction {
    Grant,
    Restrict,
    Revoke,
    Supersede,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactFactKind {
    Constraint,
    Objective,
    CommittedFact,
    /// 已决策项：用户已拍板或由持久证据确立的选择，条目必须自包含
    /// （短指代需补足其指代的方案/上下文）。
    Decision,
    WorkingSet,
    Risk,
    ResumeCandidate,
    Revalidation,
    Milestone,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConstraintMetadata {
    scope: ConstraintScope,
    lifecycle: ConstraintLifecycle,
    action: ConstraintAction,
}

impl ConstraintMetadata {
    /// 构造约束元数据（compact 级联回填等生产路径与测试共用）。
    pub const fn new(
        scope: ConstraintScope,
        lifecycle: ConstraintLifecycle,
        action: ConstraintAction,
    ) -> Self {
        Self {
            scope,
            lifecycle,
            action,
        }
    }

    pub const fn scope(&self) -> ConstraintScope {
        self.scope
    }

    pub const fn lifecycle(&self) -> ConstraintLifecycle {
        self.lifecycle
    }

    pub const fn action(&self) -> ConstraintAction {
        self.action
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactFactEntity {
    PullRequest,
    CiRun,
    Branch,
    Worktree,
    TaskData,
    TestSuite,
    Deployment,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactFactDimension {
    Status,
    HeadRevision,
    CiStatus,
    Mergeability,
    Cleanliness,
    Progress,
    TestResult,
    DeploymentState,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactFactLifecycle {
    Persistent,
    Dynamic,
    TaskData,
    Phase,
    Ephemeral,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactFactIdentity {
    entity: CompactFactEntity,
    key: String,
    dimension: CompactFactDimension,
    lifecycle: CompactFactLifecycle,
}

impl CompactFactIdentity {
    /// 仅测试构造器（生产经 serde 反序列化路径构造）。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new(
        entity: CompactFactEntity,
        key: impl Into<String>,
        dimension: CompactFactDimension,
        lifecycle: CompactFactLifecycle,
    ) -> Result<Self, CompactFactError> {
        let key = key.into();
        if key.trim().is_empty() {
            return Err(CompactFactError::EmptyIdentityKey);
        }
        Ok(Self {
            entity,
            key,
            dimension,
            lifecycle,
        })
    }

    /// 仅测试访问器。
    #[cfg_attr(not(test), allow(dead_code))]
    pub const fn entity(&self) -> CompactFactEntity {
        self.entity
    }

    /// 仅测试访问器。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// 仅测试访问器。
    #[cfg_attr(not(test), allow(dead_code))]
    pub const fn dimension(&self) -> CompactFactDimension {
        self.dimension
    }

    pub const fn lifecycle(&self) -> CompactFactLifecycle {
        self.lifecycle
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompactFact {
    sequence: u64,
    source: CompactFactSource,
    kind: CompactFactKind,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    constraint: Option<ConstraintMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    identity: Option<CompactFactIdentity>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompactFactWire {
    sequence: u64,
    source: CompactFactSource,
    kind: CompactFactKind,
    text: String,
    constraint: Option<ConstraintMetadata>,
    identity: Option<CompactFactIdentity>,
}

impl<'de> Deserialize<'de> for CompactFact {
    fn deserialize<DeserializerType>(
        deserializer: DeserializerType,
    ) -> Result<Self, DeserializerType::Error>
    where
        DeserializerType: Deserializer<'de>,
    {
        use serde::de::Error;

        let wire = CompactFactWire::deserialize(deserializer)?;
        Self::new_with_metadata(
            wire.sequence,
            wire.source,
            wire.kind,
            wire.text,
            wire.constraint,
            wire.identity,
        )
        .map_err(DeserializerType::Error::custom)
    }
}

impl CompactFact {
    pub fn new(
        sequence: u64,
        source: CompactFactSource,
        kind: CompactFactKind,
        text: impl Into<String>,
        constraint: Option<ConstraintMetadata>,
    ) -> Result<Self, CompactFactError> {
        Self::new_with_metadata(sequence, source, kind, text, constraint, None)
    }

    fn new_with_metadata(
        sequence: u64,
        source: CompactFactSource,
        kind: CompactFactKind,
        text: impl Into<String>,
        constraint: Option<ConstraintMetadata>,
        identity: Option<CompactFactIdentity>,
    ) -> Result<Self, CompactFactError> {
        let text = text.into();
        if text.trim().is_empty() {
            return Err(CompactFactError::EmptyText);
        }
        match (kind, constraint.is_some(), identity.is_some()) {
            (CompactFactKind::Constraint, false, _) => {
                return Err(CompactFactError::MissingConstraintMetadata)
            }
            (CompactFactKind::Constraint, true, true) => {
                return Err(CompactFactError::UnexpectedFactIdentity)
            }
            (CompactFactKind::Constraint, true, false) | (_, false, _) => {}
            (_, true, _) => return Err(CompactFactError::UnexpectedConstraintMetadata),
        }
        if identity
            .as_ref()
            .is_some_and(|metadata| metadata.key.trim().is_empty())
        {
            return Err(CompactFactError::EmptyIdentityKey);
        }
        Ok(Self {
            sequence,
            source,
            kind,
            text,
            constraint,
            identity,
        })
    }

    /// 构造带 identity 的 fact（生产经 serde 反序列化路径；此构造器供测试与显式构造）。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new_with_identity(
        sequence: u64,
        source: CompactFactSource,
        kind: CompactFactKind,
        text: impl Into<String>,
        identity: CompactFactIdentity,
    ) -> Result<Self, CompactFactError> {
        Self::new_with_metadata(sequence, source, kind, text, None, Some(identity))
    }

    /// 仅测试构造器（生产经 serde 反序列化路径构造）；保留错误分支覆盖。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn constraint(
        sequence: u64,
        source: CompactFactSource,
        text: impl Into<String>,
        constraint: ConstraintMetadata,
    ) -> Result<Self, CompactFactError> {
        Self::new(
            sequence,
            source,
            CompactFactKind::Constraint,
            text,
            Some(constraint),
        )
    }

    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    pub const fn source(&self) -> CompactFactSource {
        self.source
    }

    pub const fn kind(&self) -> CompactFactKind {
        self.kind
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// 按给定序号重建（不动其他字段）。
    ///
    /// map-reduce 的每个 chunk 独立编号，跨 chunk 的 sequence 不可比；归并前
    /// 由调用方按 chunk 顺序统一重编号，避免不同 chunk 的相同序号交错后让
    /// 较早的事实覆盖较新的事实。
    pub fn with_sequence(mut self, sequence: u64) -> Self {
        self.sequence = sequence;
        self
    }

    pub const fn constraint_metadata(&self) -> Option<&ConstraintMetadata> {
        self.constraint.as_ref()
    }

    pub const fn identity(&self) -> Option<&CompactFactIdentity> {
        self.identity.as_ref()
    }

    pub fn normalize_scope(mut self) -> Self {
        if !matches!(
            self.source,
            CompactFactSource::MainUser | CompactFactSource::Checkpoint
        ) {
            if let Some(constraint) = &mut self.constraint {
                if constraint.scope == ConstraintScope::Session {
                    constraint.scope = ConstraintScope::Unknown;
                    constraint.lifecycle = ConstraintLifecycle::Unknown;
                }
            }
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactTaskBatchStatusData {
    Active,
    Paused,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactTaskStatusData {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactTaskItemData {
    sequence: u64,
    subject: String,
    status: CompactTaskStatusData,
    blocked_by_sequences: Vec<u64>,
}

impl CompactTaskItemData {
    pub fn pending(
        sequence: u64,
        subject: impl Into<String>,
        blocked_by_sequences: Vec<u64>,
    ) -> Self {
        Self::new(
            sequence,
            subject,
            CompactTaskStatusData::Pending,
            blocked_by_sequences,
        )
    }

    pub fn in_progress(sequence: u64, subject: impl Into<String>) -> Self {
        Self::new(
            sequence,
            subject,
            CompactTaskStatusData::InProgress,
            Vec::new(),
        )
    }

    pub fn completed(sequence: u64, subject: impl Into<String>) -> Self {
        Self::new(
            sequence,
            subject,
            CompactTaskStatusData::Completed,
            Vec::new(),
        )
    }

    pub fn new(
        sequence: u64,
        subject: impl Into<String>,
        status: CompactTaskStatusData,
        blocked_by_sequences: Vec<u64>,
    ) -> Self {
        Self {
            sequence,
            subject: subject.into(),
            status,
            blocked_by_sequences,
        }
    }

    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn subject(&self) -> &str {
        &self.subject
    }

    pub const fn status(&self) -> &CompactTaskStatusData {
        &self.status
    }

    pub fn blocked_by_sequences(&self) -> &[u64] {
        &self.blocked_by_sequences
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactTaskSnapshotData {
    revision: u64,
    batch_id: u64,
    batch_summary: String,
    batch_status: CompactTaskBatchStatusData,
    items: Vec<CompactTaskItemData>,
}

impl CompactTaskSnapshotData {
    pub fn active(
        revision: u64,
        batch_id: u64,
        batch_summary: impl Into<String>,
        items: Vec<CompactTaskItemData>,
    ) -> Self {
        Self::new(
            revision,
            batch_id,
            batch_summary,
            CompactTaskBatchStatusData::Active,
            items,
        )
    }

    pub fn paused(
        revision: u64,
        batch_id: u64,
        batch_summary: impl Into<String>,
        items: Vec<CompactTaskItemData>,
    ) -> Self {
        Self::new(
            revision,
            batch_id,
            batch_summary,
            CompactTaskBatchStatusData::Paused,
            items,
        )
    }

    pub fn new(
        revision: u64,
        batch_id: u64,
        batch_summary: impl Into<String>,
        batch_status: CompactTaskBatchStatusData,
        mut items: Vec<CompactTaskItemData>,
    ) -> Self {
        items.sort_by_key(CompactTaskItemData::sequence);
        Self {
            revision,
            batch_id,
            batch_summary: batch_summary.into(),
            batch_status,
            items,
        }
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub const fn batch_id(&self) -> u64 {
        self.batch_id
    }

    pub fn batch_summary(&self) -> &str {
        &self.batch_summary
    }

    pub const fn batch_status(&self) -> &CompactTaskBatchStatusData {
        &self.batch_status
    }

    pub fn items(&self) -> &[CompactTaskItemData] {
        &self.items
    }

    pub fn render_companion(&self) -> String {
        self.render_companion_with_limit(self.items.len())
    }

    pub fn render_companion_with_limit(&self, item_limit: usize) -> String {
        let completed = self
            .items
            .iter()
            .filter(|item| item.status == CompactTaskStatusData::Completed)
            .count();
        let mut lines = vec![format!(
            "BatchData #{} — Tasks: {completed}/{}",
            self.batch_id,
            self.items.len()
        )];
        let prioritized_items = self
            .items
            .iter()
            .filter(|item| item.status == CompactTaskStatusData::InProgress)
            .chain(
                self.items
                    .iter()
                    .filter(|item| item.status == CompactTaskStatusData::Pending),
            )
            .chain(
                self.items
                    .iter()
                    .filter(|item| item.status == CompactTaskStatusData::Completed),
            );
        for item in prioritized_items.take(item_limit) {
            let icon = match item.status {
                CompactTaskStatusData::Pending => "□",
                CompactTaskStatusData::InProgress => "■",
                CompactTaskStatusData::Completed => "✓",
            };
            let blocked_by = if item.blocked_by_sequences.is_empty() {
                String::new()
            } else {
                format!(
                    " (blocked by {})",
                    item.blocked_by_sequences
                        .iter()
                        .map(|sequence| format!("#{sequence}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            lines.push(format!(
                "{icon} [task:{} seq:{}] {}{blocked_by}",
                item.sequence, item.sequence, item.subject
            ));
        }
        let hidden_count = self.items.len().saturating_sub(item_limit);
        if hidden_count > 0 {
            lines.push(format!(
                "… {hidden_count} more tasks omitted to fit compact budget"
            ));
        }
        lines.join("\n")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactFactBatch {
    facts: Vec<CompactFact>,
}

impl CompactFactBatch {
    pub fn new(facts: Vec<CompactFact>) -> Self {
        Self { facts }
    }

    pub fn facts(&self) -> &[CompactFact] {
        &self.facts
    }

    pub fn into_facts(self) -> Vec<CompactFact> {
        self.facts
    }
}

pub fn reconcile_checkpoint_with_task_snapshot(
    checkpoint: ContinuationCheckpoint,
    task_snapshot: Option<&CompactTaskSnapshotData>,
) -> Result<ContinuationCheckpoint, CheckpointError> {
    let Some(task_snapshot) =
        task_snapshot.filter(|snapshot| task_snapshot_is_authoritative(snapshot))
    else {
        return Ok(checkpoint);
    };
    let mut wire = checkpoint.to_wire();
    let in_progress = task_snapshot
        .items
        .iter()
        .find(|item| item.status == CompactTaskStatusData::InProgress)
        .expect("authoritative task snapshot requires exactly one in-progress item");
    let completed_subjects = task_snapshot
        .items
        .iter()
        .filter(|item| item.status == CompactTaskStatusData::Completed)
        .map(|item| normalize_for_comparison(item.subject()))
        .collect::<Vec<_>>();

    wire.resume_cursor.next_action = in_progress.subject().to_string();
    wire.uncommitted_working_set
        .retain(|line| !contradicts_completed_work(line, &completed_subjects));
    wire.open_decisions_and_risks
        .retain(|line| !contradicts_completed_work(line, &completed_subjects));
    wire.required_revalidation
        .retain(|line| !contradicts_completed_work(line, &completed_subjects));
    for item in task_snapshot
        .items
        .iter()
        .filter(|item| item.status == CompactTaskStatusData::Pending)
    {
        let dependency = if item.blocked_by_sequences().is_empty() {
            String::new()
        } else {
            format!(
                " (blocked by {})",
                item.blocked_by_sequences()
                    .iter()
                    .map(|sequence| format!("task {sequence}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        wire.uncommitted_working_set
            .retain(|line| !same_task_working_item(line, item));
        wire.uncommitted_working_set.push(format!(
            "Pending task {}: {}{dependency}",
            item.sequence(),
            item.subject()
        ));
    }
    ContinuationCheckpoint::try_from(wire)
}

fn task_snapshot_is_authoritative(snapshot: &CompactTaskSnapshotData) -> bool {
    snapshot.batch_status == CompactTaskBatchStatusData::Active
        && !snapshot.batch_summary.trim().is_empty()
        && snapshot
            .items
            .iter()
            .filter(|item| item.status == CompactTaskStatusData::InProgress)
            .count()
            == 1
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn reduce_compact_facts(
    batch: CompactFactBatch,
) -> Result<ContinuationCheckpoint, CheckpointError> {
    reduce_compact_facts_with_objective_fallback(batch, None, None)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn reduce_compact_facts_with_task_snapshot(
    batch: CompactFactBatch,
    task_snapshot: Option<&CompactTaskSnapshotData>,
) -> Result<ContinuationCheckpoint, CheckpointError> {
    reduce_compact_facts_with_objective_fallback(batch, task_snapshot, None)
}

/// 将 typed facts 归并为 canonical checkpoint。
///
/// `objective_fallback` 是 facts 中不存在 main-user objective 时的确定性兜底目标，
/// 由调用方从**原始主用户消息**派生（LLM 分类可能缺失或降级，见 #1623）。
/// 空白值视为缺失，此时保持既有保守语义（占位符 + `Waiting for User`）。
pub fn reduce_compact_facts_with_objective_fallback(
    batch: CompactFactBatch,
    task_snapshot: Option<&CompactTaskSnapshotData>,
    objective_fallback: Option<&str>,
) -> Result<ContinuationCheckpoint, CheckpointError> {
    let mut indexed_facts = batch
        .into_facts()
        .into_iter()
        .enumerate()
        .collect::<Vec<_>>();
    indexed_facts.sort_by_key(|(original_index, fact)| (fact.sequence(), *original_index));
    let superseded_dynamic_facts = indexed_facts
        .iter()
        .filter_map(|(original_index, fact)| {
            fact.identity()
                .filter(|identity| identity.lifecycle() == CompactFactLifecycle::Dynamic)
                .map(|identity| (identity.clone(), *original_index))
        })
        .fold(
            std::collections::HashMap::new(),
            |mut latest_by_identity, (identity, original_index)| {
                latest_by_identity.insert(identity, original_index);
                latest_by_identity
            },
        );

    let mut immutable_constraints = Vec::new();
    let mut current_objective = None;
    let mut committed_facts = Vec::new();
    let mut committed_decisions = Vec::new();
    let mut working_set = Vec::new();
    let mut risks = Vec::new();
    let mut next_action = None;
    let mut revalidation = Vec::new();
    let mut milestones = Vec::new();

    for (original_index, fact) in indexed_facts {
        let fact = fact.normalize_scope();
        if is_compact_protocol_text(fact.text()) {
            continue;
        }
        if fact.identity().is_some_and(|identity| {
            identity.lifecycle() == CompactFactLifecycle::Dynamic
                && superseded_dynamic_facts.get(identity) != Some(&original_index)
        }) {
            continue;
        }
        let dynamic_fact = fact
            .identity()
            .is_some_and(|identity| identity.lifecycle() == CompactFactLifecycle::Dynamic);
        match fact.kind() {
            CompactFactKind::Constraint => {
                let metadata = fact
                    .constraint_metadata()
                    .expect("validated constraint fact must have metadata");
                if matches!(
                    fact.source(),
                    CompactFactSource::MainUser | CompactFactSource::Checkpoint
                ) && metadata.scope() == ConstraintScope::Session
                    && metadata.lifecycle() == ConstraintLifecycle::Persistent
                {
                    match metadata.action() {
                        ConstraintAction::Grant
                        | ConstraintAction::Restrict
                        | ConstraintAction::Supersede => {
                            if metadata.action() == ConstraintAction::Supersede {
                                immutable_constraints.clear();
                            }
                            immutable_constraints.push(as_fact_bullet(fact.text()));
                        }
                        ConstraintAction::Revoke => immutable_constraints.clear(),
                    }
                } else {
                    risks.push(format!(
                        "- scope unverified ({:?}/{:?}): {}",
                        metadata.scope(),
                        metadata.lifecycle(),
                        fact.text()
                    ));
                }
            }
            CompactFactKind::Objective
                if matches!(
                    fact.source(),
                    CompactFactSource::MainUser | CompactFactSource::Checkpoint
                ) =>
            {
                current_objective = Some(as_fact_bullet(fact.text()));
            }
            CompactFactKind::Objective => {}
            CompactFactKind::CommittedFact
                if matches!(
                    fact.source(),
                    CompactFactSource::ToolResult | CompactFactSource::Checkpoint
                ) && dynamic_fact =>
            {
                revalidation.push(as_fact_bullet(fact.text()));
            }
            CompactFactKind::CommittedFact
                if matches!(
                    fact.source(),
                    CompactFactSource::ToolResult | CompactFactSource::Checkpoint
                ) =>
            {
                committed_facts.push(as_fact_bullet(fact.text()));
            }
            CompactFactKind::CommittedFact => {
                risks.push(format!("- unverified fact: {}", fact.text()));
            }
            CompactFactKind::Decision
                if matches!(
                    fact.source(),
                    CompactFactSource::MainUser
                        | CompactFactSource::ToolResult
                        | CompactFactSource::Checkpoint
                ) =>
            {
                committed_decisions.push(as_fact_bullet(fact.text()));
            }
            CompactFactKind::Decision => {
                risks.push(format!("- unverified decision: {}", fact.text()));
            }
            CompactFactKind::WorkingSet => working_set.push(as_fact_bullet(fact.text())),
            CompactFactKind::Risk => risks.push(as_fact_bullet(fact.text())),
            CompactFactKind::ResumeCandidate
                if matches!(
                    fact.source(),
                    CompactFactSource::MainUser | CompactFactSource::Checkpoint
                ) =>
            {
                next_action = Some(fact.text().to_string());
            }
            CompactFactKind::ResumeCandidate => {}
            CompactFactKind::Revalidation => revalidation.push(as_fact_bullet(fact.text())),
            CompactFactKind::Milestone => milestones.push(as_fact_bullet(fact.text())),
        }
    }

    if let Some(task_snapshot) =
        task_snapshot.filter(|snapshot| task_snapshot_is_authoritative(snapshot))
    {
        let in_progress = task_snapshot
            .items
            .iter()
            .find(|item| item.status == CompactTaskStatusData::InProgress)
            .expect("active task reconciliation requires exactly one in-progress item");
        next_action = Some(in_progress.subject().to_string());

        let completed_subjects = task_snapshot
            .items
            .iter()
            .filter(|item| item.status == CompactTaskStatusData::Completed)
            .map(|item| normalize_for_comparison(item.subject()))
            .collect::<Vec<_>>();
        working_set.retain(|line| !contradicts_completed_work(line, &completed_subjects));
        risks.retain(|line| !contradicts_completed_work(line, &completed_subjects));
        revalidation.retain(|line| !contradicts_completed_work(line, &completed_subjects));

        for item in task_snapshot
            .items
            .iter()
            .filter(|item| item.status == CompactTaskStatusData::Pending)
        {
            let dependency = if item.blocked_by_sequences().is_empty() {
                String::new()
            } else {
                format!(
                    " (blocked by {})",
                    item.blocked_by_sequences()
                        .iter()
                        .map(|sequence| format!("task {sequence}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            working_set.retain(|line| !same_task_working_item(line, item));
            working_set.push(format!(
                "- Pending task {}: {}{dependency}",
                item.sequence(),
                item.subject()
            ));
        }
    }

    // #1623：facts 未提供 main-user objective 时，用原始主用户消息派生的目标兜底；
    // 只有 facts 与兜底都拿不到目标时，才允许退化为占位符与 Waiting for User。
    let recovered_objective = if current_objective.is_none() {
        objective_fallback
            .map(str::trim)
            .filter(|objective| !objective.is_empty())
            .map(as_fact_bullet)
    } else {
        None
    };
    let objective_recovered_from_messages = recovered_objective.is_some();
    if let Some(recovered) = recovered_objective {
        current_objective = Some(recovered);
    }
    let objective_missing = current_objective.is_none();
    let current_objective = current_objective
        .unwrap_or_else(|| "- Revalidate the latest user objective before continuing.".to_string());
    // 兜底恢复目标时，唯一 Next action 就是继续该目标；仍然 NEVER 依赖占位符文本判定状态。
    let next_action = next_action
        .or_else(|| objective_recovered_from_messages.then(|| current_objective.clone()));
    let next_action = next_action
        .unwrap_or_else(|| "Revalidate the latest user objective before continuing.".to_string());
    let status = if objective_missing {
        ContinuationStatus::WaitingForUser
    } else {
        ContinuationStatus::Continue
    };

    ContinuationCheckpoint::from_sections(CheckpointSections {
        immutable_constraints,
        current_objective: vec![current_objective],
        committed_facts,
        uncommitted_working_set: working_set,
        open_decisions_and_risks: risks,
        resume_cursor_lines: Vec::new(),
        next_action,
        required_revalidation: revalidation,
        committed_decisions,
        archived_milestones: milestones,
        status,
        status_reason: Some(match status {
            ContinuationStatus::Continue => "a main-user objective remains active.".to_string(),
            ContinuationStatus::WaitingForUser => {
                "no active main-user objective could be established.".to_string()
            }
            ContinuationStatus::Completed => unreachable!("fact reducer never infers completion"),
        }),
    })
}

fn same_task_working_item(line: &str, item: &CompactTaskItemData) -> bool {
    let normalized_line = normalize_for_comparison(line);
    let normalized_subject = normalize_for_comparison(item.subject());
    normalized_line.contains(&format!("pending task {}", item.sequence()))
        || (!normalized_subject.is_empty() && normalized_line.contains(&normalized_subject))
}

fn contradicts_completed_work(line: &str, completed_subjects: &[String]) -> bool {
    let normalized_line = normalize_for_comparison(line);
    let reports_missing_evidence = [
        "no reliable evidence",
        "no evidence",
        "not completed",
        "未完成",
        "无可靠证据",
        "没有可靠证据",
        "尚无证据",
    ]
    .iter()
    .any(|marker| normalized_line.contains(marker));
    reports_missing_evidence
        && (!completed_subjects.is_empty()
            && (normalized_line.contains("completed")
                || normalized_line.contains("完成")
                || completed_subjects.iter().any(|subject| {
                    subject
                        .split_whitespace()
                        .filter(|word| word.len() >= 4)
                        .any(|word| normalized_line.contains(word))
                })))
}

fn normalize_for_comparison(source: &str) -> String {
    source
        .trim_start_matches("- ")
        .to_lowercase()
        .replace(['`', '.', ',', ':', ';', '(', ')'], " ")
}

fn as_fact_bullet(source: &str) -> String {
    if source.trim_start().starts_with("- ") {
        source.trim().to_string()
    } else {
        format!("- {}", source.trim())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactFactError {
    EmptyText,
    EmptyIdentityKey,
    MissingConstraintMetadata,
    UnexpectedConstraintMetadata,
    UnexpectedFactIdentity,
}

impl fmt::Display for CompactFactError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyText => write!(formatter, "compact fact text must not be empty"),
            Self::EmptyIdentityKey => {
                write!(formatter, "compact fact identity key must not be empty")
            }
            Self::MissingConstraintMetadata => {
                write!(
                    formatter,
                    "constraint metadata is required for constraint facts"
                )
            }
            Self::UnexpectedConstraintMetadata => write!(
                formatter,
                "constraint metadata is only allowed for constraint facts"
            ),
            Self::UnexpectedFactIdentity => {
                write!(
                    formatter,
                    "fact identity is not allowed for constraint facts"
                )
            }
        }
    }
}

#[cfg(test)]
#[path = "structured_facts_tests.rs"]
mod tests;
