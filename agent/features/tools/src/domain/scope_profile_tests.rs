use super::published_language::{ToolCapabilities, ToolCapability, ToolName};
use super::scope_profile::{
    is_authorized, ProfileExpansionError, RegistryScopeBuilder, RegistryScopeError, ToolProfile,
    ToolRegistrationSpec,
};

#[test]
fn profile_derivation_can_only_shrink_capabilities() {
    let parent = ToolProfile::baseline(ToolCapabilities::all());
    let requested = ToolCapabilities::ReadWorkspace | ToolCapabilities::NetworkAccess;
    let child = ToolProfile::derive_restricted(&parent, requested).unwrap();
    assert_eq!(child.allowed_capabilities(), requested);

    let read_only = ToolProfile::baseline(ToolCapabilities::ReadWorkspace);
    let error = ToolProfile::derive_restricted(
        &read_only,
        ToolCapabilities::ReadWorkspace | ToolCapabilities::WriteWorkspace,
    )
    .unwrap_err();
    assert_eq!(
        error,
        ProfileExpansionError::CapabilityExpansion {
            capabilities: ToolCapabilities::WriteWorkspace
        }
    );
}

#[test]
fn registry_scope_rejects_duplicate_names_and_missing_capability_declarations() {
    let duplicate = RegistryScopeBuilder::new("main")
        .register(ToolRegistrationSpec::new(
            "Read",
            ToolCapabilities::ReadWorkspace,
        ))
        .unwrap()
        .register(ToolRegistrationSpec::new(
            "READ",
            ToolCapabilities::WriteWorkspace,
        ))
        .unwrap_err();
    assert_eq!(
        duplicate,
        RegistryScopeError::DuplicateTool(ToolName::new("read"))
    );

    let missing = ToolRegistrationSpec::try_new("Mystery", None).unwrap_err();
    assert_eq!(
        missing,
        RegistryScopeError::MissingCapabilityDeclaration(ToolName::new("mystery"))
    );
}

#[test]
fn registry_scope_supports_crate_internal_lookup_and_iteration() {
    let scope = RegistryScopeBuilder::new("main")
        .register(ToolRegistrationSpec::new(
            "Read",
            ToolCapabilities::ReadWorkspace,
        ))
        .unwrap()
        .register(ToolRegistrationSpec::new(
            "Bash",
            ToolCapabilities::ExecuteProcess,
        ))
        .unwrap()
        .build();

    let read = scope.get(&ToolName::new("READ")).unwrap();
    assert_eq!(read.name(), &ToolName::new("read"));
    assert_eq!(
        read.required_capabilities(),
        ToolCapabilities::ReadWorkspace
    );
    assert_eq!(scope.iter().count(), 2);
}

#[test]
fn authorization_requires_every_declared_capability() {
    let spec = ToolRegistrationSpec::new(
        "Bash",
        ToolCapabilities::ReadWorkspace | ToolCapabilities::ExecuteProcess,
    );
    let read_only = ToolProfile::baseline(ToolCapabilities::ReadWorkspace);
    assert!(!is_authorized(&spec, &read_only));

    let allowed =
        ToolProfile::baseline(ToolCapabilities::ReadWorkspace | ToolCapabilities::ExecuteProcess);
    assert!(is_authorized(&spec, &allowed));
}

#[test]
fn capability_enum_converts_to_profile_set() {
    let profile = ToolProfile::baseline(ToolCapabilities::from_caps([
        ToolCapability::UserInteraction,
        ToolCapability::TaskRead,
    ]));
    assert!(profile
        .allowed_capabilities()
        .contains(ToolCapabilities::UserInteraction));
    assert!(profile
        .allowed_capabilities()
        .contains(ToolCapabilities::TaskRead));
}
