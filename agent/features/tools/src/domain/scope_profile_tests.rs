use super::published_language::{ToolCapabilities, ToolCapability, ToolName};
use super::scope_profile::{
    is_authorized, ProfileExpansionError, RegistryScopeBuilder, RegistryScopeError, ToolProfile,
    ToolRegistrationSpec,
};

#[test]
fn profile_derivation_can_only_shrink_capabilities() {
    let parent = ToolProfile::baseline(ToolCapabilities::all());
    let requested = ToolCapabilities::Read | ToolCapabilities::NetworkAccess;
    let child = ToolProfile::derive_restricted(&parent, requested).unwrap();
    assert_eq!(child.allowed_capabilities(), requested);

    let read_only = ToolProfile::baseline(ToolCapabilities::Read);
    let error = ToolProfile::derive_restricted(
        &read_only,
        ToolCapabilities::Read | ToolCapabilities::Write,
    )
    .unwrap_err();
    assert_eq!(
        error,
        ProfileExpansionError::CapabilityExpansion {
            capabilities: ToolCapabilities::Write
        }
    );
}

#[test]
fn registry_scope_rejects_duplicate_names_and_missing_capability_declarations() {
    let duplicate = RegistryScopeBuilder::new("main")
        .register(ToolRegistrationSpec::new("Read", ToolCapabilities::Read))
        .unwrap()
        .register(ToolRegistrationSpec::new("READ", ToolCapabilities::Write))
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
        .register(ToolRegistrationSpec::new("Read", ToolCapabilities::Read))
        .unwrap()
        .register(ToolRegistrationSpec::new("Bash", ToolCapabilities::Execute))
        .unwrap()
        .build();

    let read = scope.get(&ToolName::new("READ")).unwrap();
    assert_eq!(read.name(), &ToolName::new("read"));
    assert_eq!(read.required_capabilities(), ToolCapabilities::Read);
    assert_eq!(scope.iter().count(), 2);
}

#[test]
fn authorization_requires_every_declared_capability() {
    let spec =
        ToolRegistrationSpec::new("Bash", ToolCapabilities::Read | ToolCapabilities::Execute);
    let read_only = ToolProfile::baseline(ToolCapabilities::Read);
    assert!(!is_authorized(&spec, &read_only));

    let allowed = ToolProfile::baseline(ToolCapabilities::Read | ToolCapabilities::Execute);
    assert!(is_authorized(&spec, &allowed));
}

#[test]
fn capability_enum_converts_to_profile_set() {
    let profile = ToolProfile::baseline(ToolCapabilities::from_caps([
        ToolCapability::Interact,
        ToolCapability::TaskRead,
    ]));
    assert!(profile
        .allowed_capabilities()
        .contains(ToolCapabilities::Interact));
    assert!(profile
        .allowed_capabilities()
        .contains(ToolCapabilities::TaskRead));
}
