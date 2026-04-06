//! Agent memory and state management
//!
//! Provides memory storage for agents, supporting:
//! - Short-term memory (current conversation)
//! - Long-term memory (persisted across sessions)
//! - Snapshots for recovery and branching

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Agent memory entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Unique ID for this memory
    pub id: String,
    /// Content of the memory
    pub content: String,
    /// Type of memory (fact, observation, decision, etc.)
    pub memory_type: MemoryType,
    /// Timestamp when created
    pub created_at: u64,
    /// Importance score (0-1)
    pub importance: f32,
    /// Associated metadata
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

/// Types of agent memory
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    /// A fact learned about the environment
    Fact,
    /// An observation made during execution
    Observation,
    /// A decision made by the agent
    Decision,
    /// An error encountered and its resolution
    ErrorResolution,
    /// Context about the current task
    TaskContext,
    /// User preference or instruction
    UserPreference,
}

impl std::fmt::Display for MemoryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryType::Fact => write!(f, "fact"),
            MemoryType::Observation => write!(f, "observation"),
            MemoryType::Decision => write!(f, "decision"),
            MemoryType::ErrorResolution => write!(f, "error_resolution"),
            MemoryType::TaskContext => write!(f, "task_context"),
            MemoryType::UserPreference => write!(f, "user_preference"),
        }
    }
}

/// Agent memory store
#[derive(Debug, Clone, Default)]
pub struct AgentMemory {
    /// Short-term memory (current session)
    short_term: Vec<MemoryEntry>,
    /// Long-term memory (persisted)
    long_term: Vec<MemoryEntry>,
    /// Maximum short-term memory size
    max_short_term: usize,
    /// Maximum long-term memory size
    max_long_term: usize,
}

impl AgentMemory {
    /// Create a new agent memory with default limits
    pub fn new() -> Self {
        Self {
            short_term: Vec::new(),
            long_term: Vec::new(),
            max_short_term: 50,
            max_long_term: 200,
        }
    }

    /// Create with custom limits
    pub fn with_limits(max_short_term: usize, max_long_term: usize) -> Self {
        Self {
            short_term: Vec::new(),
            long_term: Vec::new(),
            max_short_term,
            max_long_term,
        }
    }

    /// Add a memory entry
    pub fn add(&mut self, entry: MemoryEntry) {
        // High importance memories go to long-term
        if entry.importance >= 0.7 {
            self.long_term.push(entry.clone());
            if self.long_term.len() > self.max_long_term {
                // Remove lowest importance
                self.long_term.sort_by(|a, b| b.importance.partial_cmp(&a.importance).unwrap_or(std::cmp::Ordering::Equal));
                self.long_term.truncate(self.max_long_term);
            }
        }
        
        // All memories go to short-term initially
        self.short_term.push(entry);
        if self.short_term.len() > self.max_short_term {
            self.short_term.remove(0);
        }
    }

    /// Get all relevant memories for a context
    pub fn get_relevant(&self, context: &str) -> Vec<&MemoryEntry> {
        let mut relevant: Vec<&MemoryEntry> = Vec::new();
        
        // Simple keyword matching for relevance
        let keywords = context.split_whitespace().collect::<Vec<_>>();
        
        for entry in &self.short_term {
            let content_words = entry.content.split_whitespace().collect::<Vec<_>>();
            if keywords.iter().any(|k| content_words.iter().any(|c| c.contains(k))) {
                relevant.push(entry);
            }
        }
        
        for entry in &self.long_term {
            let content_words = entry.content.split_whitespace().collect::<Vec<_>>();
            if keywords.iter().any(|k| content_words.iter().any(|c| c.contains(k))) {
                relevant.push(entry);
            }
        }
        
        relevant
    }

    /// Clear short-term memory
    pub fn clear_short_term(&mut self) {
        self.short_term.clear();
    }

    /// Get memory summary for context injection
    pub fn get_summary(&self) -> String {
        if self.short_term.is_empty() && self.long_term.is_empty() {
            return "No previous memory.".to_string();
        }
        
        let mut summary = String::new();
        
        if !self.long_term.is_empty() {
            summary.push_str("Important facts:\n");
            for entry in &self.long_term {
                summary.push_str(&format!("- {} ({})\n", entry.content, entry.memory_type));
            }
        }
        
        if !self.short_term.is_empty() {
            summary.push_str("\nRecent context:\n");
            for entry in self.short_term.iter().rev().take(10) {
                summary.push_str(&format!("- {}\n", entry.content));
            }
        }
        
        summary
    }

    /// Serialize for persistence
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&MemorySnapshot {
            short_term: self.short_term.clone(),
            long_term: self.long_term.clone(),
        })
    }

    /// Deserialize from persistence
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let snapshot: MemorySnapshot = serde_json::from_str(json)?;
        Ok(Self {
            short_term: snapshot.short_term,
            long_term: snapshot.long_term,
            max_short_term: 50,
            max_long_term: 200,
        })
    }
}

/// Serializable snapshot of memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySnapshot {
    short_term: Vec<MemoryEntry>,
    long_term: Vec<MemoryEntry>,
}

/// Agent state for lifecycle management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    /// Unique agent ID
    pub id: String,
    /// Agent type (local, remote, dream, teammate)
    pub agent_type: AgentType,
    /// Current status
    pub status: AgentStatus,
    /// Current prompt/task
    pub prompt: String,
    /// Progress tracking
    pub progress: AgentProgress,
    /// Model being used
    pub model: Option<String>,
    /// Error if failed
    pub error: Option<String>,
    /// Creation timestamp
    pub created_at: u64,
    /// Last update timestamp
    pub updated_at: u64,
}

/// Types of agents
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentType {
    /// Local agent running in current process
    Local,
    /// Remote agent running elsewhere
    Remote,
    /// Background dream task
    Dream,
    /// In-process teammate
    Teammate,
    /// Main session agent
    MainSession,
}

/// Agent status
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    /// Agent is being initialized
    Initializing,
    /// Agent is actively running
    Running,
    /// Agent is paused
    Paused,
    /// Agent completed successfully
    Completed,
    /// Agent failed with error
    Failed,
    /// Agent was stopped by user
    Stopped,
}

/// Progress tracking for agents
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentProgress {
    /// Number of tool calls made
    pub tool_use_count: u32,
    /// Total tokens used
    pub token_count: u32,
    /// Last activity description
    pub last_activity: Option<String>,
    /// Current tool being executed
    pub current_tool: Option<String>,
}

/// Agent snapshot for recovery and branching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSnapshot {
    /// Snapshot ID
    pub id: String,
    /// Agent ID this snapshot belongs to
    pub agent_id: String,
    /// Timestamp when snapshot was taken
    pub timestamp: u64,
    /// Agent state at snapshot time
    pub state: AgentState,
    /// Memory at snapshot time
    pub memory: MemorySnapshot,
    /// Messages at snapshot time (serialized)
    pub messages_json: String,
    /// Reason for snapshot
    pub reason: SnapshotReason,
}

/// Reasons for creating a snapshot
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotReason {
    /// User requested pause
    UserPause,
    /// Before risky operation
    BeforeRiskyOperation,
    /// At completion milestone
    Milestone,
    /// For branching/forking
    Branch,
    /// Periodic auto-snapshot
    Auto,
}

/// Manager for agent lifecycle
#[derive(Debug)]
pub struct AgentLifecycleManager {
    /// Active agents
    agents: HashMap<String, AgentState>,
    /// Agent memories
    memories: HashMap<String, AgentMemory>,
    /// Agent snapshots
    snapshots: HashMap<String, Vec<AgentSnapshot>>,
    /// Maximum snapshots per agent
    max_snapshots: usize,
    /// Maximum snapshot size in bytes (default 10MB)
    max_snapshot_size: usize,
}

impl AgentLifecycleManager {
    /// Create a new lifecycle manager
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
            memories: HashMap::new(),
            snapshots: HashMap::new(),
            max_snapshots: 10,
            max_snapshot_size: 10 * 1024 * 1024, // 10MB
        }
    }

    /// Create a new agent
    pub fn create_agent(&mut self, agent_type: AgentType, prompt: String, model: Option<String>) -> String {
        let id = uuid_generator();
        let now = current_timestamp();
        
        let state = AgentState {
            id: id.clone(),
            agent_type,
            status: AgentStatus::Initializing,
            prompt,
            progress: AgentProgress::default(),
            model,
            error: None,
            created_at: now,
            updated_at: now,
        };
        
        self.agents.insert(id.clone(), state);
        self.memories.insert(id.clone(), AgentMemory::new());
        self.snapshots.insert(id.clone(), Vec::new());
        
        id
    }

    /// Get agent state
    pub fn get_agent(&self, id: &str) -> Option<&AgentState> {
        self.agents.get(id)
    }

    /// Update agent status
    pub fn update_status(&mut self, id: &str, status: AgentStatus) {
        if let Some(state) = self.agents.get_mut(id) {
            state.status = status;
            state.updated_at = current_timestamp();
        }
    }

    /// Update agent progress
    pub fn update_progress(&mut self, id: &str, progress: AgentProgress) {
        if let Some(state) = self.agents.get_mut(id) {
            state.progress = progress;
            state.updated_at = current_timestamp();
        }
    }

    /// Add memory to agent
    pub fn add_memory(&mut self, agent_id: &str, entry: MemoryEntry) {
        if let Some(memory) = self.memories.get_mut(agent_id) {
            memory.add(entry);
        }
    }

    /// Get agent memory
    pub fn get_memory(&self, agent_id: &str) -> Option<&AgentMemory> {
        self.memories.get(agent_id)
    }

    /// Create a snapshot of an agent
    /// Returns None if snapshot exceeds size limit
    pub fn create_snapshot(&mut self, agent_id: &str, reason: SnapshotReason, messages_json: String) -> Option<String> {
        let state = self.agents.get(agent_id)?;
        let memory = self.memories.get(agent_id)?;
        
        // Check size limit (estimate without full serialization)
        let state_estimate = format!("{:?}", state).len() + format!("{:?}", memory).len();
        let snapshot_size = messages_json.len() + state_estimate;
        
        if snapshot_size > self.max_snapshot_size {
            log::warn!(
                "Snapshot for agent {} exceeds size limit ({} > {} bytes), skipping",
                agent_id, snapshot_size, self.max_snapshot_size
            );
            return None;
        }
        
        let snapshot_id = uuid_generator();
        let snapshot = AgentSnapshot {
            id: snapshot_id.clone(),
            agent_id: agent_id.to_string(),
            timestamp: current_timestamp(),
            state: state.clone(),
            memory: MemorySnapshot {
                short_term: memory.short_term.clone(),
                long_term: memory.long_term.clone(),
            },
            messages_json,
            reason,
        };
        
        if let Some(snapshots) = self.snapshots.get_mut(agent_id) {
            snapshots.push(snapshot);
            // Keep only most recent snapshots
            if snapshots.len() > self.max_snapshots {
                snapshots.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
                snapshots.truncate(self.max_snapshots);
            }
        }
        
        Some(snapshot_id)
    }

    /// Get snapshots for an agent
    pub fn get_snapshots(&self, agent_id: &str) -> Option<&Vec<AgentSnapshot>> {
        self.snapshots.get(agent_id)
    }

    /// Restore from a snapshot (returns state and memory)
    pub fn restore_snapshot(&self, snapshot_id: &str) -> Option<(AgentState, AgentMemory, String)> {
        for snapshots in self.snapshots.values() {
            for snapshot in snapshots {
                if snapshot.id == snapshot_id {
                    let memory = AgentMemory {
                        short_term: snapshot.memory.short_term.clone(),
                        long_term: snapshot.memory.long_term.clone(),
                        max_short_term: 50,
                        max_long_term: 200,
                    };
                    return Some((snapshot.state.clone(), memory, snapshot.messages_json.clone()));
                }
            }
        }
        None
    }

    /// Stop an agent
    pub fn stop_agent(&mut self, id: &str) {
        if let Some(state) = self.agents.get_mut(id) {
            state.status = AgentStatus::Stopped;
            state.updated_at = current_timestamp();
        }
    }

    /// Remove an agent completely
    pub fn remove_agent(&mut self, id: &str) {
        self.agents.remove(id);
        self.memories.remove(id);
        self.snapshots.remove(id);
    }

    /// List all active agents
    pub fn list_agents(&self) -> Vec<&AgentState> {
        self.agents.values().collect()
    }

    /// List agents by status
    pub fn list_by_status(&self, status: AgentStatus) -> Vec<&AgentState> {
        self.agents.values().filter(|s| s.status == status).collect()
    }
}

impl Default for AgentLifecycleManager {
    fn default() -> Self {
        Self::new()
    }
}

/// UUID generator using uuid crate v4
fn uuid_generator() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Current timestamp in seconds
fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(std::time::Duration::ZERO)
        .as_secs()
}

/// Shared agent lifecycle manager
pub type SharedLifecycleManager = Arc<Mutex<AgentLifecycleManager>>;