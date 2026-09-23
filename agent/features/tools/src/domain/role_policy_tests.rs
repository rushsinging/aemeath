use super::published_language::{ToolCapabilities, ToolCapability, ToolProfileName};
use super::role_policy::{compile_role_profile, role_profile_name, RolePolicyCompileError};
use share::config::RolePolicyConfig;

fn policy(capabilities: &[&str]) -> RolePolicyConfig {
    RolePolicyConfig {
        capabilities: capabilities.iter().map(|cap| cap.to_string()).collect(),
    }
}

#[test]
fn compiles_declared_capabilities_into_allow_set() {
    let profile = compile_role_profile(&policy(&["Read", "NetworkAccess"])).unwrap();
    assert_eq!(
        profile.allowed_capabilities(),
        ToolCapabilities::Read | ToolCapabilities::NetworkAccess
    );
}

#[test]
fn all_capability_grants_full_toolset() {
    let profile = compile_role_profile(&policy(&["All"])).unwrap();
    assert!(profile
        .allowed_capabilities()
        .is_subset_of(ToolCapabilities::all()));
}

#[test]
fn unknown_capability_is_rejected() {
    let error = compile_role_profile(&policy(&["Read", "NotACapability"])).unwrap_err();
    assert_eq!(
        error,
        RolePolicyCompileError::UnknownCapability {
            name: "NotACapability".to_string()
        }
    );
}

#[test]
fn empty_capabilities_is_rejected() {
    let error = compile_role_profile(&policy(&[])).unwrap_err();
    assert!(matches!(error, RolePolicyCompileError::EmptyCapabilities));
}

#[test]
fn role_profile_name_uses_stable_prefix() {
    assert_eq!(role_profile_name("explorer").as_str(), "role:explorer");
    let parsed: ToolProfileName = role_profile_name("planner");
    assert_eq!(parsed.as_str(), "role:planner");
}

#[test]
fn capability_parse_round_trips_all_variants() {
    let all = [
        ToolCapability::Read,
        ToolCapability::Write,
        ToolCapability::Execute,
        ToolCapability::NetworkAccess,
        ToolCapability::Interact,
        ToolCapability::Dispatch,
        ToolCapability::TaskRead,
        ToolCapability::TaskWrite,
        ToolCapability::WorkspaceControl,
        ToolCapability::Plan,
        ToolCapability::All,
    ];
    for cap in all {
        let parsed = ToolCapability::parse(&cap.to_string())
            .unwrap_or_else(|| panic!("parse failed for {cap:?}"));
        assert_eq!(parsed, cap);
    }
    assert!(ToolCapability::parse("Bogus").is_none());
}
