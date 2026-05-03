use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

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

fn default_timestamp() -> u64 {
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
    /// Lock order: tasks mutex -> then check snapshot, never re-acquire locks.
    pub async fn is_blocked(&self, store: &TaskStore) -> bool {
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
    pub async fn would_create_cycle(&self, store: &TaskStore, blocked_by_id: &str) -> bool {
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

#[derive(Debug, Clone)]
pub struct TaskStore {
    tasks: Arc<Mutex<HashMap<String, Task>>>,
    next_id: Arc<Mutex<u64>>,
    /// Path for persistence
    persistence_path: Option<PathBuf>,
    /// Monotonically increasing batch ID. Each `create()` call checks if a new
    /// turn has started (no non-completed tasks exist) and bumps the batch.
    current_batch: Arc<Mutex<u64>>,
}

impl TaskStore {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(1)),
            persistence_path: None,
            current_batch: Arc::new(Mutex::new(0)),
        }
    }

    /// Create a TaskStore with persistence
    pub async fn with_persistence(path: PathBuf) -> Self {
        let store = Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(1)),
            persistence_path: Some(path.clone()),
            current_batch: Arc::new(Mutex::new(0)),
        };
        // Try to load existing data (async version for init)
        store.load().await.ok();
        store
    }

    /// Get default tasks file path (~/.aemeath/tasks.json)
    pub fn default_path() -> PathBuf {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(".aemeath").join("tasks.json")
    }

    /// Load tasks from disk (async)
    pub async fn load(&self) -> Result<(), String> {
        let default_path = Self::default_path();
        let path = self.persistence_path.as_ref().unwrap_or(&default_path);

        if !path.exists() {
            return Ok(());
        }

        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| format!("Failed to read tasks file: {}", e))?;

        let data: TaskStoreData = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse tasks file: {}", e))?;

        *self.tasks.lock().await = data.tasks;
        *self.next_id.lock().await = data.next_id;

        Ok(())
    }

    /// Save tasks to disk (async)
    pub async fn save(&self) -> Result<(), String> {
        let default_path = Self::default_path();
        let path = self.persistence_path.as_ref().unwrap_or(&default_path);

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create tasks directory: {}", e))?;
        }

        let data = TaskStoreData {
            version: 1,
            tasks: self.tasks.lock().await.clone(),
            next_id: *self.next_id.lock().await,
        };

        let content = serde_json::to_string_pretty(&data)
            .map_err(|e| format!("Failed to serialize tasks: {}", e))?;

        tokio::fs::write(path, content)
            .await
            .map_err(|e| format!("Failed to write tasks file: {}", e))?;

        Ok(())
    }

    /// Create a new task with all fields (async for auto-save)
    pub async fn create(
        &self,
        subject: String,
        description: String,
        active_form: Option<String>,
    ) -> Task {
        let id = {
            let mut next_id = self.next_id.lock().await;
            let id = next_id.to_string();
            *next_id += 1;
            id
            // next_id lock released here
        };

        // Bump batch if all existing tasks are completed (new turn)
        let batch = {
            let (has_active, has_any) = {
                let tasks = self.tasks.lock().await;
                let has_any = !tasks.is_empty();
                let has_active = tasks
                    .values()
                    .any(|t| t.status != TaskStatus::Completed && t.status != TaskStatus::Deleted);
                (has_active, has_any)
            };
            let mut batch = self.current_batch.lock().await;
            if has_any && !has_active {
                *batch += 1;
            }
            *batch
        };

        let now = default_timestamp();
        let task = Task {
            id: id.clone(),
            subject,
            description,
            status: TaskStatus::Pending,
            active_form,
            owner: None,
            blocked_by: Vec::new(),
            blocks: Vec::new(),
            priority: TaskPriority::default(),
            progress: 0,
            progress_message: None,
            created_at: now,
            updated_at: now,
            session_id: None,
            tags: Vec::new(),
            batch,
        };

        self.tasks.lock().await.insert(id, task.clone());
        // Auto-save (safe now: no locks held)
        if let Err(e) = self.save().await {
            log::warn!("Failed to save task store: {}", e);
        }
        task
    }

    /// Create a task with priority (async for auto-save)
    pub async fn create_with_priority(
        &self,
        subject: String,
        description: String,
        active_form: Option<String>,
        priority: TaskPriority,
    ) -> Task {
        let id = {
            let mut next_id = self.next_id.lock().await;
            let id = next_id.to_string();
            *next_id += 1;
            id
        };

        // Bump batch if all existing tasks are completed (new turn)
        let batch = {
            let (has_active, has_any) = {
                let tasks = self.tasks.lock().await;
                let has_any = !tasks.is_empty();
                let has_active = tasks
                    .values()
                    .any(|t| t.status != TaskStatus::Completed && t.status != TaskStatus::Deleted);
                (has_active, has_any)
            };
            let mut batch = self.current_batch.lock().await;
            if has_any && !has_active {
                *batch += 1;
            }
            *batch
        };

        let now = default_timestamp();
        let task = Task {
            id: id.clone(),
            subject,
            description,
            status: TaskStatus::Pending,
            active_form,
            owner: None,
            blocked_by: Vec::new(),
            blocks: Vec::new(),
            priority,
            progress: 0,
            progress_message: None,
            created_at: now,
            updated_at: now,
            session_id: None,
            tags: Vec::new(),
            batch,
        };

        self.tasks.lock().await.insert(id, task.clone());
        if let Err(e) = self.save().await {
            log::warn!("Failed to save task store: {}", e);
        }
        task
    }

    /// Get a task by ID (async)
    pub async fn get(&self, id: &str) -> Option<Task> {
        self.tasks.lock().await.get(id).cloned()
    }

    /// Update a task (async for auto-save)
    pub async fn update(&self, id: &str, f: impl FnOnce(&mut Task)) -> Option<Task> {
        let mut tasks = self.tasks.lock().await;
        if let Some(task) = tasks.get_mut(id) {
            f(task);
            let task = task.clone();
            // Auto-save on update
            drop(tasks);
            if let Err(e) = self.save().await {
                log::warn!("Failed to save task store: {}", e);
            }
            Some(task)
        } else {
            None
        }
    }

    /// List all tasks (async)
    pub async fn list(&self) -> Vec<Task> {
        let tasks = self.tasks.lock().await;
        let mut result: Vec<Task> = tasks
            .values()
            .filter(|t| t.status != TaskStatus::Deleted)
            .cloned()
            .collect();
        // Sort by priority (urgent first), then by created_at
        result.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.created_at.cmp(&b.created_at))
        });
        result
    }

    /// List tasks by status (async)
    pub async fn list_by_status(&self, status: TaskStatus) -> Vec<Task> {
        self.list()
            .await
            .into_iter()
            .filter(|t| t.status == status)
            .collect()
    }

    /// List tasks by priority (async)
    pub async fn list_by_priority(&self, priority: TaskPriority) -> Vec<Task> {
        self.list()
            .await
            .into_iter()
            .filter(|t| t.priority == priority)
            .collect()
    }

    /// List tasks for a session (async)
    pub async fn list_by_session(&self, session_id: &str) -> Vec<Task> {
        self.list()
            .await
            .into_iter()
            .filter(|t| t.session_id.as_ref() == Some(&session_id.to_string()))
            .collect()
    }

    /// Delete a task (soft delete by setting status to Deleted, async for auto-save)
    pub async fn delete(&self, id: &str) -> bool {
        self.update(id, |t| t.status = TaskStatus::Deleted)
            .await
            .is_some()
    }

    /// Clear all tasks
    pub async fn clear(&self) {
        {
            let mut tasks = self.tasks.lock().await;
            tasks.clear();
        }
        // Release tasks lock before acquiring next_id lock
        {
            let mut next_id = self.next_id.lock().await;
            *next_id = 1;
        }
    }

    /// List tasks belonging to the latest batch only.
    /// This shows the current turn's task list, including completed ones,
    /// but hides tasks from previous turns.
    pub async fn list_current_batch(&self) -> Vec<Task> {
        let tasks = self.tasks.lock().await;
        let max_batch = tasks.values().map(|t| t.batch).max().unwrap_or(0);
        let mut result: Vec<Task> = tasks
            .values()
            .filter(|t| t.batch == max_batch && t.status != TaskStatus::Deleted)
            .cloned()
            .collect();
        result.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.created_at.cmp(&b.created_at))
        });
        result
    }

    /// Clear all deleted tasks from memory (async for auto-save)
    pub async fn purge_deleted(&self) {
        let mut tasks = self.tasks.lock().await;
        tasks.retain(|_, t| t.status != TaskStatus::Deleted);
        drop(tasks);
        if let Err(e) = self.save().await {
            log::warn!("Failed to save task store: {}", e);
        }
    }

    /// Get statistics (async)
    pub async fn stats(&self) -> TaskStoreStats {
        let tasks = self.tasks.lock().await;
        let total = tasks.len();
        let pending = tasks
            .values()
            .filter(|t| t.status == TaskStatus::Pending)
            .count();
        let in_progress = tasks
            .values()
            .filter(|t| t.status == TaskStatus::InProgress)
            .count();
        let completed = tasks
            .values()
            .filter(|t| t.status == TaskStatus::Completed)
            .count();
        let deleted = tasks
            .values()
            .filter(|t| t.status == TaskStatus::Deleted)
            .count();

        let by_priority = tasks
            .values()
            .filter(|t| t.status != TaskStatus::Deleted)
            .fold(HashMap::new(), |mut acc, t| {
                *acc.entry(t.priority).or_insert(0) += 1;
                acc
            });

        TaskStoreStats {
            total,
            pending,
            in_progress,
            completed,
            deleted,
            by_priority,
        }
    }
}

impl Default for TaskStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Serializable task store data with version for migration support
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TaskStoreData {
    /// Schema version for migrations
    #[serde(default = "default_version")]
    pub version: u32,
    tasks: HashMap<String, Task>,
    next_id: u64,
}

fn default_version() -> u32 {
    1
}

/// Task store statistics
#[derive(Debug, Clone)]
pub struct TaskStoreStats {
    pub total: usize,
    pub pending: usize,
    pub in_progress: usize,
    pub completed: usize,
    pub deleted: usize,
    pub by_priority: HashMap<TaskPriority, usize>,
}
