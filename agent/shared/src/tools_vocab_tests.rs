use super::tools_vocab::*;

#[test]
fn tool_name_preserves_canonical_spelling_and_normalizes_identity() {
    let name = ToolName::new("Grep");
    assert_eq!(name.as_str(), "Grep");
    assert_eq!(name.normalized(), "grep");
    assert_eq!(name, ToolName::new("grep"));
}

#[test]
fn tool_name_serde_remains_a_canonical_string() {
    let name = ToolName::new("Grep");
    let encoded = serde_json::to_string(&name).unwrap();
    assert_eq!(encoded, r#""Grep""#);
    let decoded: ToolName = serde_json::from_str(r#""grep""#).unwrap();
    assert_eq!(decoded.as_str(), "grep");
    assert_eq!(decoded.normalized(), "grep");
}

#[test]
fn tool_name_normalizes_to_lowercase() {
    let name = ToolName::new("Read");
    assert_eq!(name.normalized(), "read");
    assert_eq!(name.as_str(), "Read");
}

#[test]
fn tool_name_preserves_mcp_qualified_name() {
    let name = ToolName::new("mcp__Server__Tool");
    assert_eq!(name.normalized(), "mcp__server__tool");
}

#[test]
fn tool_name_equality_is_case_insensitive() {
    let a = ToolName::new("Bash");
    let b = ToolName::new("BASH");
    assert_eq!(a, b);
}

#[test]
fn tool_name_display() {
    let name = ToolName::new("Grep");
    assert_eq!(format!("{name}"), "Grep");
}

#[test]
fn tool_name_ordering_follows_normalized_key() {
    let mut names = [
        ToolName::new("Write"),
        ToolName::new("bash"),
        ToolName::new("ALL"),
    ];
    names.sort();
    assert_eq!(
        names
            .iter()
            .map(|name| name.normalized())
            .collect::<Vec<_>>(),
        ["all", "bash", "write"]
    );
}

#[test]
fn capabilities_contains_cap() {
    let caps = ToolCapabilities::Read | ToolCapabilities::Write;
    assert!(caps.contains_cap(ToolCapability::Read));
    assert!(caps.contains_cap(ToolCapability::Write));
    assert!(!caps.contains_cap(ToolCapability::Execute));
}

#[test]
fn capabilities_from_caps() {
    let caps = ToolCapabilities::from_caps([ToolCapability::Read, ToolCapability::NetworkAccess]);
    assert!(caps.contains_cap(ToolCapability::Read));
    assert!(caps.contains_cap(ToolCapability::NetworkAccess));
    assert!(!caps.contains_cap(ToolCapability::Write));
}

#[test]
fn capabilities_is_subset_of() {
    let full = ToolCapabilities::all();
    let partial = ToolCapabilities::Read | ToolCapabilities::Write;
    assert!(partial.is_subset_of(full));
    assert!(!full.is_subset_of(partial));
}

#[test]
fn capabilities_empty_is_subset_of_anything() {
    let empty = ToolCapabilities::empty();
    let some = ToolCapabilities::Read;
    assert!(empty.is_subset_of(some));
    assert!(empty.is_subset_of(ToolCapabilities::empty()));
}

#[test]
fn capabilities_serde_transparent_bitset_roundtrip() {
    let caps = ToolCapabilities::Read | ToolCapabilities::Dispatch;
    let encoded = serde_json::to_string(&caps).unwrap();
    let decoded: ToolCapabilities = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, caps);
    assert!(decoded.contains_cap(ToolCapability::Dispatch));
}

#[test]
fn capability_parse_and_display_are_symmetric() {
    for name in [
        "Read",
        "Write",
        "Execute",
        "NetworkAccess",
        "Interact",
        "Dispatch",
        "TaskRead",
        "TaskWrite",
        "WorkspaceControl",
        "All",
    ] {
        let capability = ToolCapability::parse(name).unwrap_or_else(|| {
            panic!("parse 应识别 canonical 变体名 {name}");
        });
        assert_eq!(capability.to_string(), name);
    }
    assert!(ToolCapability::parse("write").is_none());
}

#[test]
fn authorization_context_constants_oppose_each_other() {
    let standard = AuthorizationContext::STANDARD;
    let allow_all = AuthorizationContext::ALLOW_ALL;
    assert!(!standard.allow_outside_workspace);
    assert!(standard.require_read_before_write);
    assert!(!allow_all.require_read_before_write);
    assert!(!allow_all.enforce_bash_safety);
    assert_ne!(standard, allow_all);
}
