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

    pub fn parse(s: &str) -> Option<Self> {
        s.parse().ok()
    }
}

impl std::str::FromStr for TaskPriority {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "low" => Ok(TaskPriority::Low),
            "normal" | "medium" => Ok(TaskPriority::Normal),
            "high" => Ok(TaskPriority::High),
            "urgent" | "critical" => Ok(TaskPriority::Urgent),
            _ => Err(()),
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskTimestamps {
    pub created_at: u64,
    pub updated_at: u64,
}

impl TaskTimestamps {
    pub fn new(created_at: u64, updated_at: u64) -> Self {
        Self {
            created_at,
            updated_at,
        }
    }
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
    #[serde(default)]
    pub created_at: u64,
    /// Last updated timestamp
    #[serde(default)]
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

impl Task {
    /// Update progress
    pub fn set_progress(&mut self, progress: u8, message: Option<String>, updated_at: u64) {
        self.progress = progress.min(100);
        self.progress_message = message;
        self.updated_at = updated_at;
    }

    /// Set priority
    pub fn set_priority(&mut self, priority: TaskPriority, updated_at: u64) {
        self.priority = priority;
        self.updated_at = updated_at;
    }

    /// Add a tag
    pub fn add_tag(&mut self, tag: String, updated_at: u64) {
        if !self.tags.contains(&tag) {
            self.tags.push(tag);
            self.updated_at = updated_at;
        }
    }

    /// Remove a tag
    pub fn remove_tag(&mut self, tag: &str, updated_at: u64) {
        self.tags.retain(|t| t != tag);
        self.updated_at = updated_at;
    }
}

/// Batch status for lifecycle management.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BatchStatus {
    /// Active batch — currently being worked on
    #[default]
    Active,
    /// Paused batch — interrupted by user, can be resumed
    Paused,
    /// Archived batch — completed or discarded
    Archived,
}

/// Represents a batch (group of tasks from one turn).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Batch {
    pub id: u64,
    /// Human-readable summary of the user request this batch belongs to.
    #[serde(default)]
    pub summary: Option<String>,
    pub status: BatchStatus,
    pub created_at: u64,
    pub last_active_turn: u64,
    /// Number of completed turns since last activity
    #[serde(default)]
    pub silence_turns: u64,
}

impl Batch {
    pub fn new(id: u64, created_at: u64) -> Self {
        Self {
            id,
            summary: None,
            status: BatchStatus::Active,
            created_at,
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
