//! Compile a config-layer [`RolePolicyConfig`] into a [`ToolProfile`].
//!
//! Config stores plain strings; this is the single boundary where tool-name
//! and capability-name spellings are validated against the registry. Derived
//! capability bits are the union of the required capabilities of the
//! allowlisted tools, intersected with the declared capability restriction.

use std::collections::BTreeSet;

use share::config::RolePolicyConfig;

use super::published_language::{ToolCapabilities, ToolCapability, ToolName, ToolProfileName};
use super::scope_profile::ToolProfile;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RolePolicyCompileError {
    #[error("unknown tool name in role policy: {name}")]
    UnknownToolName { name: String },
    #[error("unknown capability in role policy: {name}")]
    UnknownCapability { name: String },
    #[error("role policy allowlist is empty")]
    EmptyAllowlist,
}

/// `role:<name>` profile naming shared by composition (registration) and
/// runtime (lookup); single source of truth.
pub fn role_profile_name(role: &str) -> ToolProfileName {
    ToolProfileName::new(format!("role:{role}"))
}

/// Compile a role policy into a profile.
///
/// `required_caps_of` resolves a tool name (as registered) to its required
/// capabilities; composition passes a closure over the assembled main scope.
pub fn compile_role_profile(
    policy: &RolePolicyConfig,
    required_caps_of: &dyn Fn(&str) -> Option<ToolCapabilities>,
) -> Result<ToolProfile, RolePolicyCompileError> {
    if policy.allowed_tools.is_empty() {
        return Err(RolePolicyCompileError::EmptyAllowlist);
    }
    let mut allowed_tool_names = BTreeSet::new();
    let mut derived_caps = ToolCapabilities::empty();
    for tool in &policy.allowed_tools {
        let required = required_caps_of(tool)
            .ok_or_else(|| RolePolicyCompileError::UnknownToolName { name: tool.clone() })?;
        allowed_tool_names.insert(ToolName::new(tool));
        derived_caps |= required;
    }
    if !policy.capabilities.is_empty() {
        let mut declared = ToolCapabilities::empty();
        for capability in &policy.capabilities {
            let parsed = ToolCapability::parse(capability).ok_or_else(|| {
                RolePolicyCompileError::UnknownCapability {
                    name: capability.clone(),
                }
            })?;
            declared |= ToolCapabilities::single(parsed);
        }
        derived_caps &= declared;
    }
    Ok(ToolProfile::baseline_with_names(
        derived_caps,
        allowed_tool_names,
    ))
}
