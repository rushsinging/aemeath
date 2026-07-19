//! Tests for the independent Skill Published Language (`skill_pl`).
//!
//! These tests pin the DTO shape and the content-derived revision contract
//! before the adapter is wired. See `specs/tools.md` (Issue #912) and
//! `docs/design/02-modules/tools/02-ports-and-lifecycle.md` §6.

use super::skill_pl::*;

// ── PromptFragment ─────────────────────────────────────────────────────

#[test]
fn prompt_fragment_carries_stable_key_content_source_and_cache_hint() {
    let frag = PromptFragment::new(
        "commit",
        "do a commit",
        SkillSource::builtin("aemeath-builtin://commit"),
        CacheHint::Stable,
    );
    assert_eq!(frag.stable_key(), "commit");
    assert_eq!(frag.content(), "do a commit");
    assert_eq!(frag.source().kind, SkillSourceKind::Builtin);
    assert_eq!(frag.source().path, "aemeath-builtin://commit");
    assert_eq!(frag.cache_hint(), CacheHint::Stable);
}

#[test]
fn prompt_fragment_is_clone_and_debug() {
    let frag = sample_fragment("alpha", "body-alpha");
    let cloned = frag.clone();
    assert_eq!(cloned.stable_key(), frag.stable_key());
    let _ = format!("{frag:?}");
}

#[test]
fn skill_source_distinguishes_origin_kinds() {
    let file_src = SkillSource::file(SkillSourceKind::ProjectClaude, "/p/.claude/skills/x.md");
    assert_eq!(file_src.kind, SkillSourceKind::ProjectClaude);
    assert_eq!(file_src.path, "/p/.claude/skills/x.md");

    let builtin_src = SkillSource::builtin("aemeath-builtin://commit");
    assert_eq!(builtin_src.kind, SkillSourceKind::Builtin);
}

// ── SkillDescriptor ────────────────────────────────────────────────────

#[test]
fn skill_descriptor_exposes_name_description_source_and_aliases() {
    let desc = SkillDescriptor::new(
        "review",
        "code review skill",
        SkillSource::file(
            SkillSourceKind::ProjectAgents,
            "/p/.agents/skills/review.md",
        ),
        vec!["cr".to_string()],
    );
    assert_eq!(desc.name(), "review");
    assert_eq!(desc.description(), "code review skill");
    assert_eq!(desc.source().kind, SkillSourceKind::ProjectAgents);
    assert_eq!(desc.aliases(), &["cr".to_string()]);
}

// ── SkillMaterializationRevision (content-derived) ─────────────────────

#[test]
fn revision_is_stable_for_identical_content() {
    let frags = sample_fragments();
    let a = SkillMaterializationRevision::from_fragments(&frags);
    let b = SkillMaterializationRevision::from_fragments(&frags);
    assert_eq!(a, b);
    assert_eq!(a.as_str(), b.as_str());
}

#[test]
fn revision_changes_when_content_changes() {
    let mut frags = sample_fragments();
    let before = SkillMaterializationRevision::from_fragments(&frags);
    frags[0].mut_content().push_str(" (edited)");
    let after = SkillMaterializationRevision::from_fragments(&frags);
    assert_ne!(before, after);
}

#[test]
fn revision_changes_when_stable_key_changes() {
    let mut frags = sample_fragments();
    let before = SkillMaterializationRevision::from_fragments(&frags);
    frags[0] = PromptFragment::new(
        "renamed",
        frags[0].content(),
        frags[0].source().clone(),
        frags[0].cache_hint(),
    );
    let after = SkillMaterializationRevision::from_fragments(&frags);
    assert_ne!(before, after);
}

#[test]
fn revision_is_order_independent_for_the_same_content_set() {
    // The revision is derived from the content set, so reordering the same
    // fragments must not change it. This keeps the revision stable even if
    // discovery iteration order varies.
    let mut frags = sample_fragments();
    let ordered = SkillMaterializationRevision::from_fragments(&frags);
    frags.reverse();
    let reversed = SkillMaterializationRevision::from_fragments(&frags);
    assert_eq!(ordered, reversed);
}

#[test]
fn revision_of_empty_set_is_still_a_nonempty_value() {
    let rev = SkillMaterializationRevision::from_fragments(&[]);
    assert!(!rev.as_str().is_empty());
}

#[test]
fn revision_serializes_as_a_string() {
    let rev = SkillMaterializationRevision::from_fragments(&sample_fragments());
    let json = serde_json::to_string(&rev).expect("serialize");
    assert!(json.starts_with('"') && json.ends_with('"'));
    let back: SkillMaterializationRevision = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, rev);
}

// ── SkillMaterializationSnapshot ───────────────────────────────────────

#[test]
fn snapshot_holds_fragments_and_a_revision() {
    let frags = sample_fragments();
    let snap = SkillMaterializationSnapshot::from_fragments(frags.clone());
    assert_eq!(snap.fragments().len(), frags.len());
    assert_eq!(snap.revision().as_str().len(), 32);
}

#[test]
fn snapshot_revision_matches_from_fragments_for_same_content() {
    let frags = sample_fragments();
    let snap = SkillMaterializationSnapshot::from_fragments(frags.clone());
    let direct = SkillMaterializationRevision::from_fragments(&frags);
    assert_eq!(snap.revision(), &direct);
}

// ── SkillError (typed) ─────────────────────────────────────────────────

#[test]
fn skill_error_read_failed_carries_path_and_reason() {
    let err = SkillError::read_failed("/missing.md", "No such file");
    let msg = err.to_string();
    assert!(msg.contains("/missing.md"), "message must name path");
    assert!(msg.contains("No such file"), "message must name reason");
}

#[test]
fn skill_error_parse_failed_carries_path_and_reason() {
    let err = SkillError::parse_failed("/bad.md", "bad yaml");
    let msg = err.to_string();
    assert!(msg.contains("/bad.md"));
    assert!(msg.contains("bad yaml"));
}

#[test]
fn skill_error_clone_roundtrips() {
    let err = SkillError::read_failed("/missing.md", "io");
    let cloned = err.clone();
    assert_eq!(err.to_string(), cloned.to_string());
}

// ── helpers ────────────────────────────────────────────────────────────

fn sample_fragment(key: &str, body: &str) -> PromptFragment {
    PromptFragment::new(
        key,
        body,
        SkillSource::file(SkillSourceKind::ProjectAgents, format!("/p/{key}.md")),
        CacheHint::Stable,
    )
}

fn sample_fragments() -> Vec<PromptFragment> {
    vec![
        sample_fragment("alpha", "body-alpha"),
        sample_fragment("beta", "body-beta"),
        sample_fragment("commit", "builtin commit body"),
    ]
}
