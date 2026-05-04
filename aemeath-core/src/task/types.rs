use serde::{Deserialize, Serialize};

/// Task priority levels
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, Ord, PartialOrd, Hash,
)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    #[default]
    Normal,
    Low,
    High,
    Urgent,
}

impl TaskPriority {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskPriority::Low => "low",
            TaskPriority::Normal => "normal",
            TaskPriority::High => "high",
            TaskPriority::Urgent => "urgent",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "low" => Some(TaskPriority::Low),
            "normal" | "medium" => Some(TaskPriority::Normal),
            "high" => Some(TaskPriority::High),
            "urgent" | "critical" => Some(TaskPriority::Urgent),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub subject: String,
    pub description: String,
    pub status: TaskStatus,
    #[serde(default)]
    pub active_form: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub blocked_by: Vec<String>,
    #[serde(default)]
    pub blocks: Vec<String>,
    /// Task priority
    #[serde(default)]
    pub priority: TaskPriority,
    /// Progress percentage (0-100)
    #[serde(default)]
    pub progress: u8,
    /// Progress message
    #[serde(default)]
    pub progress_message: Option<String>,
    /// Creation timestamp (milliseconds since epoch)
    #[serde(default = "default_timestamp")]
    pub created_at: u64,
    /// Last updated timestamp
    #[serde(default = "default_timestamp")]
    pub updated_at: u64,
    /// Session ID this task belongs to
    #[serde(default)]
    pub session_id: Option<String>,
    /// Tags for categorization
    #[serde(default)]
    pub tags: Vec<String>,
    /// Batch ID: tasks created in the same turn share the same batch.
    /// A new batch starts when all previous tasks are completed.
    #[serde(default)]
    pub batch: u64,
}

pub(crate) fn default_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or_default()
}

impl Task {
    /// Update progress
    pub fn set_progress(&mut self, progress: u8, message: Option<String>) {
        self.progress = progress.min(100);
        self.progress_message = message;
        self.updated_at = default_timestamp();
    }

    /// Set priority
    pub fn set_priority(&mut self, priority: TaskPriority) {
        self.priority = priority;
        self.updated_at = default_timestamp();
    }

    /// Add a tag
    pub fn add_tag(&mut self, tag: String) {
        if !self.tags.contains(&tag) {
            self.tags.push(tag);
            self.updated_at = default_timestamp();
        }
    }

    /// Remove a tag
    pub fn remove_tag(&mut self, tag: &str) {
        self.tags.retain(|t| t != tag);
        self.updated_at = default_timestamp();
    }

    /// Check if task is blocked (has incomplete blockers) - async
    /// NOTE: This method takes a snapshot of tasks to avoid nested locking deadlock.
    /// Lock order: tasks mutex -> then check snapshot, never re-lock.
    pub async fn is_blocked(&self, store: &crate::task::TaskStore) -> bool {
        // Take a snapshot to avoid nested locking
        let tasks_snapshot = store.tasks.lock().await.clone();
        for id in &self.blocked_by {
            if let Some(t) = tasks_snapshot.get(id) {
                if t.status != TaskStatus::Completed {
                    return true;
                }
            }
        }
        false
    }

    /// Check if adding this task as a blocker would create a circular dependency.
    /// Returns true if circular dependency detected.
    pub async fn would_create_cycle(
        &self,
        store: &crate::task::TaskStore,
        blocked_by_id: &str,
    ) -> bool {
        // If we would block ourselves, that's circular
        if self.id == blocked_by_id {
            return true;
        }

        // Take snapshot to avoid nested locking
        let tasks_snapshot = store.tasks.lock().await.clone();

        // DFS to detect cycle: starting from blocked_by_id, follow blocked_by chain
        // If we reach self.id, there's a cycle
        let mut visited: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut stack: Vec<&str> = vec![blocked_by_id];

        while let Some(current_id) = stack.pop() {
            if current_id == self.id {
                // Found cycle: blocked_by_id -> ... -> self.id
                return true;
            }
            if visited.contains(current_id) {
                continue;
            }
            visited.insert(current_id);

            // Add our blocked_by to the stack
            if let Some(task) = tasks_snapshot.get(current_id) {
                for dep_id in &task.blocked_by {
                    stack.push(dep_id.as_str());
                }
            }
        }

        false
    }
}

/// Batch status for lifecycle management.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BatchStatus {
    /// Active batch — currently being worked on
    Active,
    /// Paused batch — interrupted by user, can be resumed
    Paused,
    /// Archived batch — completed or discarded
    Archived,
}

impl Default for BatchStatus {
    fn default() -> Self {
        Self::Active
    }
}

/// Represents a batch (group of tasks from one turn).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Batch {
    pub id: u64,
    pub status: BatchStatus,
    pub created_at: u64,
    pub last_active_turn: u64,
    /// Number of completed turns since last activity
    #[serde(default)]
    pub silence_turns: u64,
}

impl Batch {
    pub fn new(id: u64) -> Self {
        let now = default_timestamp();
        Self {
            id,
            status: BatchStatus::Active,
            created_at: now,
            last_active_turn: 0,
            silence_turns: 0,
        }
    }
}

/// Serializable snapshot of a TaskStore for session persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSnapshot {
    pub tasks: Vec<Task>,
    pub next_id: u64,
    pub current_batch: u64,
    /// Batches metadata for lifecycle management
    #[serde(default)]
    pub batches: Vec<Batch>,
}
