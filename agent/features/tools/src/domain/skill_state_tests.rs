//! Skill 加载状态 Published Language 测试。

use super::skill_state::{SkillLoadMutation, SkillLoadScope, SkillLoadStateError};

#[test]
fn main_scope_is_stable_and_subagent_scope_uses_instance_identity() {
    assert_eq!(SkillLoadScope::main(), SkillLoadScope::main());
    assert_eq!(
        SkillLoadScope::subagent("agent-1").unwrap(),
        SkillLoadScope::subagent("agent-1").unwrap()
    );
    assert_ne!(
        SkillLoadScope::subagent("agent-1").unwrap(),
        SkillLoadScope::subagent("agent-2").unwrap()
    );
}

#[test]
fn subagent_scope_rejects_blank_instance_identity() {
    assert_eq!(
        SkillLoadScope::subagent("  ").unwrap_err(),
        SkillLoadStateError::InvalidInstanceId
    );
}

#[test]
fn mutation_preserves_scope_and_canonical_skill_revision() {
    let scope = SkillLoadScope::subagent("agent-1").unwrap();
    let mutation = SkillLoadMutation::new("session-1", scope.clone(), "review", "r1").unwrap();

    assert_eq!(mutation.session_id(), "session-1");
    assert_eq!(mutation.scope(), &scope);
    assert_eq!(mutation.skill_name(), "review");
    assert_eq!(mutation.revision(), "r1");
}

#[test]
fn mutation_rejects_blank_identity_fields() {
    assert_eq!(
        SkillLoadMutation::new(" ", SkillLoadScope::main(), "review", "r1").unwrap_err(),
        SkillLoadStateError::InvalidSessionId
    );
    assert_eq!(
        SkillLoadMutation::new("session", SkillLoadScope::main(), " ", "r1").unwrap_err(),
        SkillLoadStateError::InvalidSkillName
    );
    assert_eq!(
        SkillLoadMutation::new("session", SkillLoadScope::main(), "review", " ").unwrap_err(),
        SkillLoadStateError::InvalidRevision
    );
}
