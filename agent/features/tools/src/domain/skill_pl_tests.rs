//! Tests for the dynamic Skill Published Language.

use super::skill_pl::*;
use std::collections::BTreeSet;
use std::path::PathBuf;

#[test]
fn metadata_snapshot_revision_ignores_source_and_tracks_metadata() {
    fn descriptor(description: &str, source: &str) -> SkillDescriptor {
        SkillDescriptor::new(
            "release",
            description,
            SkillSource::file(SkillSourceKind::ProjectAgents, source),
            vec!["ship".to_string()],
            Some("release".to_string()),
            vec!["rel".to_string()],
            Some("[version]".to_string()),
        )
    }
    let first = SkillCatalogSnapshot::from_descriptors(vec![descriptor("release", "/a")]);
    let source_only = SkillCatalogSnapshot::from_descriptors(vec![descriptor("release", "/b")]);
    assert_eq!(first.revision, source_only.revision);
    assert_eq!(first.slash_routes[0].skill, "release");
    assert_eq!(first.slash_routes[0].aliases, ["rel"]);

    let changed = SkillCatalogSnapshot::from_descriptors(vec![descriptor("changed", "/a")]);
    assert_ne!(first.revision, changed.revision);
}

#[test]
fn descriptor_contains_metadata_but_no_body() {
    let descriptor = SkillDescriptor::new(
        "review",
        "Review code",
        SkillSource::file(SkillSourceKind::ProjectAgents, "/p/review/SKILL.md"),
        vec!["code-review".into()],
        Some("review".into()),
        vec!["cr".into()],
        Some("[scope]".into()),
    );
    assert_eq!(descriptor.name(), "review");
    assert_eq!(descriptor.description(), "Review code");
    assert_eq!(descriptor.aliases(), &["code-review"]);
    assert_eq!(descriptor.slash_command(), Some("review"));
    assert_eq!(descriptor.slash_aliases(), &["cr"]);
    assert_eq!(descriptor.argument_hint(), Some("[scope]"));
}

#[test]
fn load_query_preserves_identity_and_frozen_values() {
    let query = SkillLoadQuery::new(
        "release",
        PathBuf::from("/project"),
        vec![PathBuf::from("/extra")],
        BTreeSet::from(["Bash".to_string()]),
    )
    .expect("valid identity");
    assert_eq!(query.identity(), "release");
    assert_eq!(query.project_root, PathBuf::from("/project"));
    assert_eq!(query.extra_dirs, vec![PathBuf::from("/extra")]);
    assert_eq!(query.available_tools, BTreeSet::from(["Bash".to_string()]));
}

#[test]
fn load_query_rejects_empty_identity() {
    let error = SkillLoadQuery::new("  ", PathBuf::new(), Vec::new(), BTreeSet::new())
        .expect_err("empty identity must fail");
    assert!(matches!(error, SkillError::InvalidIdentity { .. }));
}

#[test]
fn load_query_normalizes_slash_and_case() {
    let query = SkillLoadQuery::new(" /Release ", PathBuf::new(), Vec::new(), BTreeSet::new())
        .expect("valid alias");
    assert_eq!(query.identity(), "release");
}

#[test]
fn loaded_skill_exposes_single_body_and_revision() {
    let loaded = LoadedSkill::new(
        "release",
        "body",
        SkillSource::builtin("aemeath-builtin://release"),
        "revision",
    );
    assert_eq!(loaded.name(), "release");
    assert_eq!(loaded.content(), "body");
    assert_eq!(loaded.revision(), "revision");
}

#[test]
fn not_found_is_typed() {
    let error = SkillError::not_found("missing");
    assert!(matches!(error, SkillError::NotFound { ref identity } if identity == "missing"));
}
