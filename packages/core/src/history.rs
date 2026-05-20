//! History management for command input
//!
//! Provides persistent storage for command history,
//! similar to shell history files.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// A single history entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// The command/input text
    pub text: String,
    /// Timestamp when the command was entered
    pub timestamp: u64,
    /// Session ID where it was entered
    pub session_id: String,
}

/// History manager
pub struct HistoryManager {
    /// History entries
    entries: Vec<HistoryEntry>,
    /// Maximum number of entries to keep
    max_entries: usize,
    /// History file path
    path: PathBuf,
}

impl HistoryManager {
    /// Create a new history manager
    pub fn new(max_entries: usize) -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let path = home.join(".aemeath").join("history.json");

        Self {
            entries: Vec::new(),
            max_entries,
            path,
        }
    }

    /// Create with default settings (1000 entries)
    pub fn default_manager() -> Self {
        Self::new(1000)
    }

    /// Load history from disk
    pub fn load(&mut self) -> Result<(), String> {
        if !self.path.exists() {
            return Ok(()); // No history file yet
        }

        let content =
            fs::read_to_string(&self.path).map_err(|e| format!("Failed to read history: {}", e))?;

        let entries: Vec<HistoryEntry> = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse history: {}", e))?;

        self.entries = entries;
        Ok(())
    }

    /// Save history to disk
    pub fn save(&self) -> Result<(), String> {
        // Ensure directory exists
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create history dir: {}", e))?;
        }

        let content = serde_json::to_string_pretty(&self.entries)
            .map_err(|e| format!("Failed to serialize history: {}", e))?;

        fs::write(&self.path, content).map_err(|e| format!("Failed to write history: {}", e))?;

        Ok(())
    }

    /// Add a new entry
    pub fn add(&mut self, text: String, session_id: &str) {
        // Don't add empty entries
        if text.trim().is_empty() {
            return;
        }

        // Don't add duplicates of the last entry
        if let Some(last) = self.entries.last() {
            if last.text == text {
                return;
            }
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.entries.push(HistoryEntry {
            text,
            timestamp,
            session_id: session_id.to_string(),
        });

        // Trim if exceeds max — use drain(0..1) instead of remove(0) to avoid O(n) shift
        if self.entries.len() > self.max_entries {
            self.entries.drain(0..self.entries.len() - self.max_entries);
        }
    }

    /// Get all entries
    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    /// Get entries for a specific session
    pub fn session_entries(&self, session_id: &str) -> Vec<&HistoryEntry> {
        self.entries
            .iter()
            .filter(|e| e.session_id == session_id)
            .collect()
    }

    /// Get recent entries (last N)
    pub fn recent(&self, count: usize) -> &[HistoryEntry] {
        let start = self.entries.len().saturating_sub(count);
        &self.entries[start..]
    }

    /// Search for entries matching a pattern
    pub fn search(&self, pattern: &str) -> Vec<&HistoryEntry> {
        self.entries
            .iter()
            .filter(|e| e.text.contains(pattern))
            .collect()
    }

    /// Clear all history
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Get the last entry
    pub fn last(&self) -> Option<&HistoryEntry> {
        self.entries.last()
    }

    /// Get history suggestions for autocomplete
    pub fn suggestions(&self, prefix: &str, limit: usize) -> Vec<&str> {
        // Find entries starting with prefix, from most recent
        self.entries
            .iter()
            .rev()
            .filter(|e| e.text.starts_with(prefix))
            .take(limit)
            .map(|e| e.text.as_str())
            .collect()
    }

    /// Get the history file path
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Get number of entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if history is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for HistoryManager {
    fn default() -> Self {
        Self::default_manager()
    }
}

/// History statistics
pub struct HistoryStats {
    pub total_entries: usize,
    pub sessions_count: usize,
    pub oldest_timestamp: Option<u64>,
    pub newest_timestamp: Option<u64>,
    pub most_common_commands: Vec<(String, usize)>,
}

impl HistoryManager {
    /// Get history statistics
    pub fn stats(&self) -> HistoryStats {
        let total_entries = self.entries.len();

        // Count unique sessions
        let sessions: std::collections::HashSet<_> =
            self.entries.iter().map(|e| e.session_id.clone()).collect();
        let sessions_count = sessions.len();

        // Find timestamps
        let oldest_timestamp = self.entries.first().map(|e| e.timestamp);
        let newest_timestamp = self.entries.last().map(|e| e.timestamp);

        // Count command frequency (entries starting with /)
        let mut command_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for entry in &self.entries {
            if entry.text.starts_with('/') {
                let cmd = entry.text.split_whitespace().next().unwrap_or("");
                *command_counts.entry(cmd.to_string()).or_insert(0) += 1;
            }
        }

        let mut most_common_commands: Vec<(String, usize)> = command_counts.into_iter().collect();
        most_common_commands.sort_by(|a, b| b.1.cmp(&a.1));
        most_common_commands.truncate(10);

        HistoryStats {
            total_entries,
            sessions_count,
            oldest_timestamp,
            newest_timestamp,
            most_common_commands,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_history_add() {
        let mut history = HistoryManager::new(100);
        history.add("test command".to_string(), "session1");
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn test_history_no_duplicates() {
        let mut history = HistoryManager::new(100);
        history.add("test".to_string(), "session1");
        history.add("test".to_string(), "session1");
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn test_history_max_entries() {
        let mut history = HistoryManager::new(5);
        for i in 0..10 {
            history.add(format!("cmd {}", i), "session1");
        }
        assert_eq!(history.len(), 5);
    }

    #[test]
    fn test_history_search() {
        let mut history = HistoryManager::new(100);
        history.add("test one".to_string(), "session1");
        history.add("test two".to_string(), "session1");
        history.add("other".to_string(), "session1");

        let results = history.search("test");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_history_suggestions() {
        let mut history = HistoryManager::new(100);
        history.add("/help".to_string(), "session1");
        history.add("/config set model".to_string(), "session1");
        history.add("/help exit".to_string(), "session1");

        let suggestions = history.suggestions("/help", 5);
        assert!(suggestions.len() >= 2);
    }
}
