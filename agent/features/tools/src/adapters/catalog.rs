//! Catalog adapter over the private mutable registry and exact scope/profile declarations.

use std::{collections::HashMap, sync::Arc};

use crate::adapters::tool_registry::ToolRegistry;
use crate::domain::published_language::{
    ConcurrencyDeclaration, InputSafetyDeclaration, RegistryScopeName, ToolCatalogError,
    ToolCatalogSnapshot, ToolDescriptor, ToolProfileName,
};
use crate::domain::scope_profile::{is_authorized, RegistryScope, ToolProfile};

#[derive(Debug, thiserror::Error)]
pub enum ToolBackingError {
    #[error("tool backing requires at least one registry scope")]
    NoScopes,
    #[error("tool backing requires at least one tool profile")]
    NoProfiles,
    #[error("scope key {key} does not match declaration {declared}")]
    ScopeNameMismatch { key: String, declared: String },
}

/// Shared private state for catalog and execution. A scope only contains tools
/// that were successfully assembled with all required resources; no synthetic
/// resource probe is performed at projection or dispatch time.
#[derive(Clone)]
pub struct ToolBacking {
    registry: Arc<ToolRegistry>,
    scopes: Arc<HashMap<RegistryScopeName, RegistryScope>>,
    profiles: Arc<HashMap<ToolProfileName, ToolProfile>>,
}

impl ToolBacking {
    pub fn try_new(
        registry: Arc<ToolRegistry>,
        scopes: HashMap<RegistryScopeName, RegistryScope>,
        profiles: HashMap<ToolProfileName, ToolProfile>,
    ) -> Result<Self, ToolBackingError> {
        if scopes.is_empty() {
            return Err(ToolBackingError::NoScopes);
        }
        if profiles.is_empty() {
            return Err(ToolBackingError::NoProfiles);
        }
        for (name, scope) in &scopes {
            if name != scope.name() {
                return Err(ToolBackingError::ScopeNameMismatch {
                    key: name.to_string(),
                    declared: scope.name().to_string(),
                });
            }
        }
        Ok(Self {
            registry,
            scopes: Arc::new(scopes),
            profiles: Arc::new(profiles),
        })
    }

    pub(crate) fn scope(&self, name: &RegistryScopeName) -> Option<&RegistryScope> {
        self.scopes.get(name)
    }

    pub(crate) fn profile(&self, name: &ToolProfileName) -> Option<&ToolProfile> {
        self.profiles.get(name)
    }

    pub(crate) fn registry(&self) -> &ToolRegistry {
        &self.registry
    }

    /// Conservative dynamic-source seam (including MCP Ready migration): add
    /// the callable to the registry only. It remains invisible and
    /// unexecutable unless composition has independently declared exact scope
    /// membership and profile authorization. This deliberately does not alter
    /// the existing MCP connection lifecycle.
    pub fn register_conservative_dynamic<T: crate::domain::TypedTool + 'static>(&self, tool: T) {
        self.registry.register(tool);
    }
}

#[derive(Clone)]
pub struct CatalogAdapter {
    backing: ToolBacking,
}

impl CatalogAdapter {
    pub fn new(backing: ToolBacking) -> Self {
        Self { backing }
    }
}

impl crate::domain::ToolCatalogPort for CatalogAdapter {
    fn snapshot(
        &self,
        scope_name: &RegistryScopeName,
        profile_name: &ToolProfileName,
    ) -> Result<ToolCatalogSnapshot, ToolCatalogError> {
        let scope =
            self.backing
                .scope(scope_name)
                .ok_or_else(|| ToolCatalogError::UnknownScope {
                    scope: scope_name.to_string(),
                })?;
        let profile =
            self.backing
                .profile(profile_name)
                .ok_or_else(|| ToolCatalogError::UnknownProfile {
                    profile: profile_name.to_string(),
                })?;

        let mut tools = scope
            .iter()
            .filter(|spec| is_authorized(spec, profile))
            .filter_map(|spec| {
                let tool = self.backing.registry().get(spec.name().as_str())?;
                Some(ToolDescriptor {
                    name: spec.name().clone(),
                    description: tool.description().to_string(),
                    input_schema: tool.input_schema(),
                    required_capabilities: spec.required_capabilities(),
                    concurrency: if tool.is_concurrency_safe() {
                        ConcurrencyDeclaration::safe()
                    } else {
                        ConcurrencyDeclaration::serialized()
                    },
                    cancellation: tool.cancellation(),
                    timeout_secs: tool.timeout_secs(),
                    read_only: tool.is_read_only(),
                    input_safety: if tool.name().eq_ignore_ascii_case("bash") {
                        InputSafetyDeclaration::ReadOnlyShellCommand
                    } else if tool.is_read_only() {
                        InputSafetyDeclaration::Always
                    } else {
                        InputSafetyDeclaration::Never
                    },
                    data_schema: tool.data_schema(),
                })
            })
            .collect::<Vec<_>>();
        tools.sort_by(|left, right| left.name.as_str().cmp(right.name.as_str()));

        Ok(ToolCatalogSnapshot::new(
            scope_name.clone(),
            profile_name.clone(),
            tools,
        ))
    }
}
