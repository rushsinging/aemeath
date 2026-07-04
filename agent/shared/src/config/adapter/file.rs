//! FileAdapter — reads JSON config files into ConfigPatch.
//!
//! Replaces the file-reading logic that was in ConfigManager.
// TODO: S1 — migrate file reading logic from config_manager.rs.

use crate::config::domain::merge::ConfigPatch;

/// Placeholder — will be populated from config_manager.rs in S1.
pub fn read(_path: &std::path::Path) -> Option<ConfigPatch> {
    None
}
