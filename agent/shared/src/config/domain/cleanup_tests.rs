//! `cleanup.*` 配置测试。

use super::*;

fn config_from_json(json: &str) -> CleanupConfig {
    serde_json::from_str(json).expect("deserialize CleanupConfig")
}

#[test]
fn default_uses_thirty_days_for_all_faces() {
    let config = CleanupConfig::default();
    assert_eq!(config.retention_days.default, 30);
    for face in [
        CleanupFace::Sessions,
        CleanupFace::Audit,
        CleanupFace::Tmp,
        CleanupFace::Legacy,
    ] {
        assert!(
            config.retention_days.effective_days(face) == Some(30),
            "face {face:?}"
        );
    }
}

#[test]
fn face_override_replaces_default() {
    let config =
        config_from_json(r#"{"retention_days": {"default": 30, "sessions": 7, "audit": 90}}"#);
    assert_eq!(
        config.retention_days.effective_days(CleanupFace::Sessions),
        Some(7)
    );
    assert_eq!(
        config.retention_days.effective_days(CleanupFace::Audit),
        Some(90)
    );
    assert_eq!(
        config.retention_days.effective_days(CleanupFace::Tmp),
        Some(30)
    );
    assert_eq!(
        config.retention_days.effective_days(CleanupFace::Legacy),
        Some(30)
    );
}

#[test]
fn zero_disables_the_face() {
    let config = config_from_json(r#"{"retention_days": {"sessions": 0}}"#);
    assert_eq!(
        config.retention_days.effective_days(CleanupFace::Sessions),
        None
    );
    assert_eq!(
        config.retention_days.effective_days(CleanupFace::Audit),
        Some(30)
    );
}

#[test]
fn zero_default_disables_uncovered_faces() {
    let config = config_from_json(r#"{"retention_days": {"default": 0, "audit": 14}}"#);
    assert_eq!(
        config.retention_days.effective_days(CleanupFace::Audit),
        Some(14)
    );
    assert_eq!(
        config.retention_days.effective_days(CleanupFace::Sessions),
        None
    );
    assert_eq!(config.retention_days.effective_days(CleanupFace::Tmp), None);
    assert_eq!(
        config.retention_days.effective_days(CleanupFace::Legacy),
        None
    );
}

#[test]
fn empty_json_object_falls_back_to_default() {
    let config = config_from_json("{}");
    assert_eq!(config.retention_days, CleanupRetentionDaysConfig::default());
}

#[test]
fn face_keys_match_config_field_names() {
    assert_eq!(CleanupFace::Sessions.key(), "sessions");
    assert_eq!(CleanupFace::Audit.key(), "audit");
    assert_eq!(CleanupFace::Tmp.key(), "tmp");
    assert_eq!(CleanupFace::Legacy.key(), "legacy");
}
