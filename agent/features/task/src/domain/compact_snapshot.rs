//! Compact 用 Task 状态投影（#1863 D 步迁自 context：投影归所在域顶级）。
//!
//! context 的 `CompactRequestData.task_snapshot` 字段与 runtime 的
//! pre-compact 反思材料消费此投影。

use serde::{Deserialize, Serialize};

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
    pub sequence: u64,
    pub subject: String,
    pub status: CompactTaskStatusData,
    pub blocked_by_sequences: Vec<u64>,
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
#[serde(rename_all = "snake_case")]
pub enum CompactTaskBatchStatusData {
    Active,
    Paused,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactTaskSnapshotData {
    pub revision: u64,
    pub batch_id: u64,
    pub batch_summary: String,
    pub batch_status: CompactTaskBatchStatusData,
    pub items: Vec<CompactTaskItemData>,
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
