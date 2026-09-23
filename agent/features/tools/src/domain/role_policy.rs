//! Compile a config-layer [`RolePolicyConfig`] into a [`ToolProfile`].
//!
//! Config stores plain capability names; this is the single boundary where
//! capability spellings are validated against the published enum. The
//! compiled profile is a pure capability allow-set — the agent's toolset is
//! assembled from the capability groups declared by its role.

use share::config::RolePolicyConfig;

use super::published_language::{ToolCapabilities, ToolCapability, ToolProfileName};
use super::scope_profile::ToolProfile;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RolePolicyCompileError {
    #[error("unknown capability in role policy: {name}")]
    UnknownCapability { name: String },
    #[error("role policy capabilities are empty")]
    EmptyCapabilities,
}

/// `role:<name>` profile naming shared by composition (registration) and
/// runtime (lookup); single source of truth.
pub fn role_profile_name(role: &str) -> ToolProfileName {
    ToolProfileName::new(format!("role:{role}"))
}

/// Compile a role policy into a capability-only profile.
pub fn compile_role_profile(
    policy: &RolePolicyConfig,
) -> Result<ToolProfile, RolePolicyCompileError> {
    if policy.capabilities.is_empty() {
        return Err(RolePolicyCompileError::EmptyCapabilities);
    }
    let mut declared = ToolCapabilities::empty();
    for capability in &policy.capabilities {
        let parsed = ToolCapability::parse(capability).ok_or_else(|| {
            RolePolicyCompileError::UnknownCapability {
                name: capability.clone(),
            }
        })?;
        declared |= ToolCapabilities::single(parsed);
    }
    Ok(ToolProfile::baseline(declared))
}
