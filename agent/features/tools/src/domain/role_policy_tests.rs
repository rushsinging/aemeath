use super::published_language::{ToolCapabilities, ToolCapability, ToolName, ToolProfileName};
use super::role_policy::{compile_role_profile, role_profile_name, RolePolicyCompileError};
use super::scope_profile::ToolProfile;
use share::config::RolePolicyConfig;

fn policy(allowed_tools: &[&str], capabilities: &[&str]) -> RolePolicyConfig {
    RolePolicyConfig {
        allowed_tools: allowed_tools.iter().map(|tool| tool.to_string()).collect(),
        capabilities: capabilities.iter().map(|cap| cap.to_string()).collect(),
    }
}

/// Registry lookup stub: Read/Grep 需要 ReadWorkspace，Write 需要 WriteWorkspace。
fn caps_of(tool: &str) -> Option<ToolCapabilities> {
    match tool.to_ascii_lowercase().as_str() {
        "read" | "grep" => Some(ToolCapabilities::ReadWorkspace),
        "write" => Some(ToolCapabilities::WriteWorkspace),
        _ => None,
    }
}

#[test]
fn compiles_allowlist_and_derives_capabilities_from_registered_tools() {
    let profile = compile_role_profile(&policy(&["Read", "Grep"], &[]), &caps_of).unwrap();
    let names = profile.allowed_tool_names().expect("allowlist compiled");
    assert!(names.contains(&ToolName::new("Read")));
    assert!(names.contains(&ToolName::new("Grep")));
    assert!(!names.contains(&ToolName::new("Write")));
    // capability 位 = 名单内工具 required capabilities 的并集
    assert_eq!(
        profile.allowed_capabilities(),
        ToolCapabilities::ReadWorkspace
    );
}

#[test]
fn declared_capabilities_intersect_with_derived_bits() {
    let profile =
        compile_role_profile(&policy(&["Read", "Write"], &["ReadWorkspace"]), &caps_of).unwrap();
    assert_eq!(
        profile.allowed_capabilities(),
        ToolCapabilities::ReadWorkspace
    );
    // capability 收缩不影响名单；授权由 capability 子集 + 名单共同决定
    assert!(profile
        .allowed_tool_names()
        .unwrap()
        .contains(&ToolName::new("Write")));
}

#[test]
fn unknown_tool_name_is_rejected() {
    let error = compile_role_profile(&policy(&["NoSuchTool"], &[]), &caps_of).unwrap_err();
    assert_eq!(
        error,
        RolePolicyCompileError::UnknownToolName {
            name: "NoSuchTool".to_string()
        }
    );
}

#[test]
fn unknown_capability_is_rejected() {
    let error =
        compile_role_profile(&policy(&["Read"], &["NotACapability"]), &caps_of).unwrap_err();
    assert_eq!(
        error,
        RolePolicyCompileError::UnknownCapability {
            name: "NotACapability".to_string()
        }
    );
}

#[test]
fn empty_allowlist_is_rejected() {
    let error = compile_role_policy_empty().unwrap_err();
    assert!(matches!(error, RolePolicyCompileError::EmptyAllowlist));
}

fn compile_role_policy_empty() -> Result<ToolProfile, RolePolicyCompileError> {
    compile_role_profile(&policy(&[], &[]), &caps_of)
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
        ToolCapability::ReadWorkspace,
        ToolCapability::WriteWorkspace,
        ToolCapability::ExecuteProcess,
        ToolCapability::NetworkAccess,
        ToolCapability::UserInteraction,
        ToolCapability::AgentDispatch,
        ToolCapability::TaskMutation,
        ToolCapability::WorkspaceControl,
        ToolCapability::PlanControl,
        ToolCapability::TaskRead,
    ];
    for cap in all {
        let parsed = ToolCapability::parse(&cap.to_string())
            .unwrap_or_else(|| panic!("parse failed for {cap:?}"));
        assert_eq!(parsed, cap);
    }
    assert!(ToolCapability::parse("Bogus").is_none());
}
