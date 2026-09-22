use super::published_language::{RegistryScopeName, ToolCapabilities, ToolName};
use std::collections::{BTreeSet, HashMap};

/// Capability allow-set plus optional tool-name allowlist for a run.
///
/// Its private state can only be created as a baseline or derived by
/// shrinking an existing profile. `allowed_tool_names == None` keeps name
/// authorization open (legacy behavior); `Some(set)` restricts visibility
/// and authorization to the listed tools only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolProfile {
    allowed_capabilities: ToolCapabilities,
    allowed_tool_names: Option<BTreeSet<ToolName>>,
}

impl ToolProfile {
    pub fn baseline(allowed_capabilities: ToolCapabilities) -> Self {
        Self {
            allowed_capabilities,
            allowed_tool_names: None,
        }
    }

    pub fn baseline_with_names(
        allowed_capabilities: ToolCapabilities,
        allowed_tool_names: BTreeSet<ToolName>,
    ) -> Self {
        Self {
            allowed_capabilities,
            allowed_tool_names: Some(allowed_tool_names),
        }
    }

    /// Derive a child profile that may only shrink the parent.
    ///
    /// `requested_names: None` keeps name authorization open regardless of the
    /// parent's list; `Some(set)` must be a subset of the parent's set (when
    /// the parent carries one), otherwise [`ProfileExpansionError::ToolNameExpansion`].
    pub fn derive_restricted(
        parent: &Self,
        requested: ToolCapabilities,
        requested_names: Option<BTreeSet<ToolName>>,
    ) -> Result<Self, ProfileExpansionError> {
        let expansion = requested & !parent.allowed_capabilities;
        if !expansion.is_empty() {
            return Err(ProfileExpansionError::CapabilityExpansion {
                capabilities: expansion,
            });
        }
        if let (Some(parent_names), Some(names)) = (&parent.allowed_tool_names, &requested_names) {
            let tool_expansion: BTreeSet<ToolName> =
                names.difference(parent_names).cloned().collect();
            if !tool_expansion.is_empty() {
                return Err(ProfileExpansionError::ToolNameExpansion {
                    tools: tool_expansion,
                });
            }
        }
        Ok(Self {
            allowed_capabilities: requested,
            allowed_tool_names: requested_names,
        })
    }

    pub fn allowed_capabilities(&self) -> ToolCapabilities {
        self.allowed_capabilities
    }

    pub fn allowed_tool_names(&self) -> Option<&BTreeSet<ToolName>> {
        self.allowed_tool_names.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileExpansionError {
    CapabilityExpansion { capabilities: ToolCapabilities },
    ToolNameExpansion { tools: BTreeSet<ToolName> },
}

/// The single registration declaration for one tool in a registry scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRegistrationSpec {
    name: ToolName,
    required_capabilities: ToolCapabilities,
}

impl ToolRegistrationSpec {
    /// Builds a registration declaration from production/dynamic input.
    ///
    /// `None` is rejected rather than represented in a partially validated spec.
    pub fn try_new(
        name: impl Into<ToolName>,
        required_capabilities: Option<ToolCapabilities>,
    ) -> Result<Self, RegistryScopeError> {
        let name = name.into();
        let required_capabilities = required_capabilities
            .ok_or_else(|| RegistryScopeError::MissingCapabilityDeclaration(name.clone()))?;
        Ok(Self {
            name,
            required_capabilities,
        })
    }

    pub fn new(name: impl Into<ToolName>, required_capabilities: ToolCapabilities) -> Self {
        Self::try_new(name, Some(required_capabilities))
            .expect("an explicit capability declaration is always valid")
    }

    pub fn name(&self) -> &ToolName {
        &self.name
    }

    pub fn required_capabilities(&self) -> ToolCapabilities {
        self.required_capabilities
    }
}

/// A validated, named set of tools assembled for one kind of run.
#[derive(Debug, Clone)]
pub struct RegistryScope {
    name: RegistryScopeName,
    registrations: HashMap<ToolName, ToolRegistrationSpec>,
}

impl RegistryScope {
    pub fn name(&self) -> &RegistryScopeName {
        &self.name
    }

    pub fn len(&self) -> usize {
        self.registrations.len()
    }

    /// Looks up a validated registration by its normalized logical name.
    pub(crate) fn get(&self, name: &ToolName) -> Option<&ToolRegistrationSpec> {
        self.registrations.get(name)
    }

    pub(crate) fn insert_or_replace(&mut self, spec: ToolRegistrationSpec) {
        self.registrations.insert(spec.name.clone(), spec);
    }

    pub(crate) fn remove(&mut self, name: &ToolName) {
        self.registrations.remove(name);
    }

    /// Iterates validated registrations without exposing the scope from crate root.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &ToolRegistrationSpec> {
        self.registrations.values()
    }
}

#[derive(Debug, Clone)]
pub struct RegistryScopeBuilder {
    name: RegistryScopeName,
    registrations: HashMap<ToolName, ToolRegistrationSpec>,
}

impl RegistryScopeBuilder {
    pub fn new(name: impl Into<RegistryScopeName>) -> Self {
        Self {
            name: name.into(),
            registrations: HashMap::new(),
        }
    }

    #[cfg(test)]
    pub fn register(mut self, spec: ToolRegistrationSpec) -> Result<Self, RegistryScopeError> {
        self.register_mut(spec)?;
        Ok(self)
    }

    pub fn register_mut(&mut self, spec: ToolRegistrationSpec) -> Result<(), RegistryScopeError> {
        if self.registrations.contains_key(&spec.name) {
            return Err(RegistryScopeError::DuplicateTool(spec.name));
        }
        self.registrations.insert(spec.name.clone(), spec);
        Ok(())
    }

    pub fn build(self) -> RegistryScope {
        RegistryScope {
            name: self.name,
            registrations: self.registrations,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryScopeError {
    DuplicateTool(ToolName),
    MissingCapabilityDeclaration(ToolName),
}

/// Shared pure authorization predicate for catalog projection and execution.
pub fn is_authorized(spec: &ToolRegistrationSpec, profile: &ToolProfile) -> bool {
    let capabilities_authorized = spec
        .required_capabilities()
        .is_subset_of(profile.allowed_capabilities);
    let name_authorized = profile
        .allowed_tool_names
        .as_ref()
        .map_or(true, |names| names.contains(spec.name()));
    capabilities_authorized && name_authorized
}
