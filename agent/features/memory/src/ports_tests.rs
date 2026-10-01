use super::*;

#[test]
fn envelope_preserves_mode_metadata_and_relevance() {
    let entry = MemoryEntry::new(
        MemoryId::now_v7(),
        10,
        MemoryLayer::Project,
        MemoryCategory::Fact,
        "legacy fact",
        MemorySource::User,
    )
    .unwrap();
    let result = MemorySearchResult {
        mode: MemoryRetrievalMode::ExplicitSearch,
        hits: vec![MemorySearchHit {
            entry,
            location: MemoryLocation::Archive,
            outdated: true,
            ttl_expired: true,
            superseded_by: None,
            relevance: Some(0.75),
        }],
    };
    assert_eq!(result.mode, MemoryRetrievalMode::ExplicitSearch);
    assert_eq!(result.hits[0].location, MemoryLocation::Archive);
    assert_eq!(result.hits[0].relevance, Some(0.75));
}
