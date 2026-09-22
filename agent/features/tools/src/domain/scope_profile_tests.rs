use super::published_language::{ToolCapabilities, ToolCapability, ToolName};
use super::scope_profile::{
    is_authorized, ProfileExpansionError, RegistryScopeBuilder, RegistryScopeError, ToolProfile,
    ToolRegistrationSpec,
};

#[test]
fn profile_derivation_can_only_shrink_capabilities() {
    let parent = ToolProfile::baseline(ToolCapabilities::all());
    let requested = ToolCapabilities::ReadWorkspace | ToolCapabilities::NetworkAccess;
    let child = ToolProfile::derive_restricted(&parent, requested, None).unwrap();
    assert_eq!(child.allowed_capabilities(), requested);

    let read_only = ToolProfile::baseline(ToolCapabilities::ReadWorkspace);
    let error = ToolProfile::derive_restricted(
        &read_only,
        ToolCapabilities::ReadWorkspace | ToolCapabilities::WriteWorkspace,
        None,
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
fn profile_authorizes_only_allowlisted_tool_names() {
    let profile = ToolProfile::baseline_with_names(
        ToolCapabilities::all(),
        ["Read", "Grep"]
            .iter()
            .map(|name| ToolName::new(*name))
            .collect(),
    );
    let read = ToolRegistrationSpec::new(ToolName::new("Read"), ToolCapabilities::ReadWorkspace);
    let grep = ToolRegistrationSpec::new(ToolName::new("Grep"), ToolCapabilities::ReadWorkspace);
    let write = ToolRegistrationSpec::new(ToolName::new("Write"), ToolCapabilities::WriteWorkspace);
    assert!(is_authorized(&read, &profile));
    assert!(is_authorized(&grep, &profile));
    assert!(!is_authorized(&write, &profile));
}

#[test]
fn profile_without_allowlist_keeps_name_authorization_open() {
    let profile = ToolProfile::baseline(ToolCapabilities::all());
    let write = ToolRegistrationSpec::new(ToolName::new("Write"), ToolCapabilities::WriteWorkspace);
    assert!(is_authorized(&write, &profile));
}

#[test]
fn derive_restricted_rejects_tool_name_expansion() {
    let parent = ToolProfile::baseline_with_names(
        ToolCapabilities::all(),
        ["Read"].iter().map(|name| ToolName::new(*name)).collect(),
    );
    let requested_names = ["Read", "Write"]
        .iter()
        .map(|name| ToolName::new(*name))
        .collect();
    let error =
        ToolProfile::derive_restricted(&parent, ToolCapabilities::all(), Some(requested_names))
            .unwrap_err();
    assert!(matches!(
        error,
        ProfileExpansionError::ToolNameExpansion { .. }
    ));
}

#[test]
fn derive_restricted_allows_name_shrink_within_parent() {
    let parent = ToolProfile::baseline_with_names(
        ToolCapabilities::all(),
        ["Read", "Write"]
            .iter()
            .map(|name| ToolName::new(*name))
            .collect(),
    );
    let requested_names = ["Read"].iter().map(|name| ToolName::new(*name)).collect();
    let child =
        ToolProfile::derive_restricted(&parent, ToolCapabilities::all(), Some(requested_names))
            .unwrap();
    let names = child.allowed_tool_names().expect("names preserved");
    assert_eq!(names.len(), 1);
    assert!(names.contains(&ToolName::new("Read")));
}

#[test]
fn derive_restricted_child_without_names_inherits_open_name_authorization() {
    // 子请求不带名单时保持开放（不继承父名单收缩），capability 语义不变；
    // 名单只由携带名单的派生收紧。
    let parent = ToolProfile::baseline_with_names(
        ToolCapabilities::all(),
        ["Read"].iter().map(|name| ToolName::new(*name)).collect(),
    );
    let child = ToolProfile::derive_restricted(&parent, ToolCapabilities::all(), None).unwrap();
    assert!(child.allowed_tool_names().is_none());
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
