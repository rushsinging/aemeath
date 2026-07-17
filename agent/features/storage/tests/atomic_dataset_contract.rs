//! #983 L3 public contract for `AtomicDatasetPort`.
//!
//! These tests exercise only the crate's published surface. They pin the
//! multi-member dataset transaction behavior: a stable empty revision, first
//! commit, canonical member ordering, complete-replacement deletion of omitted
//! members, consistent full-member reads, expected-revision CAS, explicit
//! previous access without cross-generation fallback, promotion, and
//! dataset-scoped quarantine.

use std::str::FromStr;

use storage::{
    AtomicDatasetPort, DatasetCommitVisibility, DatasetKey, DatasetMember, DatasetReadOutcome,
    Durability, FileSystemDatasetAdapter, Generation, QuarantineOutcome, QuarantineReason,
    SafePathSegment, StorageErrorKind, StorageNamespace, TransactionScope, WriteOptions,
};
use uuid::Uuid;

fn unique_root(case: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("aemeath-dataset-{case}-{}", Uuid::new_v4()))
}

fn adapter(case: &str) -> (FileSystemDatasetAdapter, std::path::PathBuf) {
    let root = unique_root(case);
    let adapter =
        FileSystemDatasetAdapter::new(&root).expect("dataset adapter root should initialize");
    (adapter, root)
}

fn key() -> DatasetKey {
    DatasetKey::new(
        StorageNamespace::Memory,
        vec![SafePathSegment::from_str("conversation-1").expect("valid segment")],
    )
    .expect("valid dataset key")
}

fn name(value: &str) -> SafePathSegment {
    SafePathSegment::from_str(value).expect("valid member name")
}

fn member(value: &str, bytes: &[u8]) -> DatasetMember {
    DatasetMember::new(name(value), bytes.to_vec())
}

fn options() -> WriteOptions {
    WriteOptions::new(Durability::BestEffort)
}

fn member_names(manifest_members: &[SafePathSegment]) -> Vec<&str> {
    manifest_members
        .iter()
        .map(SafePathSegment::as_str)
        .collect()
}

/// Commits the given members as a complete generation replacing whatever the
/// current revision is, and returns the freshly committed revision.
async fn seed_generation(
    port: &dyn AtomicDatasetPort,
    dataset: &DatasetKey,
    members: &[DatasetMember],
) -> storage::DatasetRevision {
    let expected = port
        .read_manifest(dataset)
        .await
        .expect("read_manifest must succeed")
        .revision()
        .clone();
    let receipt = port
        .commit_atomic(dataset, &expected, members, options())
        .await
        .expect("commit_atomic must publish the generation");
    assert_eq!(receipt.visibility(), DatasetCommitVisibility::Visible);
    assert_eq!(receipt.warning(), None);
    receipt.revision().clone()
}

#[tokio::test]
async fn read_manifest_starts_empty_with_stable_revision() {
    let (adapter, root) = adapter("empty-manifest");
    let key = key();

    let first = adapter
        .read_manifest(&key)
        .await
        .expect("read_manifest must succeed on an absent dataset");
    let second = adapter
        .read_manifest(&key)
        .await
        .expect("read_manifest must succeed again");

    assert!(
        first.members().is_empty(),
        "an uncommitted dataset must expose no members"
    );
    assert_eq!(
        first.revision(),
        second.revision(),
        "the empty dataset must report a stable, deterministic revision"
    );
    assert_eq!(
        adapter.read_consistent(&key, &[]).await.unwrap(),
        DatasetReadOutcome::NotFound,
        "an uncommitted dataset has no consistent snapshot to read"
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn first_commit_publishes_complete_generation() {
    let (adapter, root) = adapter("first-commit");
    let key = key();

    let expected = adapter
        .read_manifest(&key)
        .await
        .unwrap()
        .revision()
        .clone();
    let receipt = adapter
        .commit_atomic(
            &key,
            &expected,
            &[member("active", b"a1"), member("index", b"i1")],
            options(),
        )
        .await
        .expect("first commit must succeed against the empty revision");
    assert_eq!(receipt.visibility(), DatasetCommitVisibility::Visible);
    assert_eq!(receipt.warning(), None);

    let manifest = adapter.read_manifest(&key).await.unwrap();
    assert_eq!(
        manifest.revision(),
        receipt.revision(),
        "read_manifest must report the freshly committed revision"
    );
    assert_eq!(member_names(manifest.members()), ["active", "index"]);

    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn commit_revision_is_independent_of_member_input_order() {
    let (adapter, root) = adapter("input-order");
    let ordered_key = key();
    let scrambled_key =
        DatasetKey::new(StorageNamespace::Memory, vec![name("conversation-2")]).unwrap();

    let ordered = seed_generation(
        &adapter,
        &ordered_key,
        &[
            member("active", b"a"),
            member("archive", b"z"),
            member("index", b"i"),
        ],
    )
    .await;
    let scrambled = seed_generation(
        &adapter,
        &scrambled_key,
        &[
            member("index", b"i"),
            member("active", b"a"),
            member("archive", b"z"),
        ],
    )
    .await;

    assert_eq!(
        ordered, scrambled,
        "revision must depend only on canonical member content, not input order"
    );
    assert_eq!(
        member_names(
            adapter
                .read_manifest(&scrambled_key)
                .await
                .unwrap()
                .members()
        ),
        ["active", "archive", "index"],
        "read-back members must follow canonical name order"
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn complete_replacement_deletes_omitted_members() {
    let (adapter, root) = adapter("replace-omitted");
    let key = key();

    let previous_revision = seed_generation(
        &adapter,
        &key,
        &[
            member("active", b"a1"),
            member("archive", b"z1"),
            member("index", b"i1"),
        ],
    )
    .await;

    // The replacement omits `archive`; a full-generation commit must delete it.
    adapter
        .commit_atomic(
            &key,
            &previous_revision,
            &[member("active", b"a2"), member("index", b"i2")],
            options(),
        )
        .await
        .expect("complete replacement must commit");

    let manifest = adapter.read_manifest(&key).await.unwrap();
    assert_eq!(
        member_names(manifest.members()),
        ["active", "index"],
        "omitted members must disappear from the current generation"
    );
    assert_eq!(
        adapter
            .read_consistent(&key, &[name("archive")])
            .await
            .unwrap(),
        DatasetReadOutcome::NotFound,
        "a deleted member must never resurface from the previous generation"
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn read_consistent_returns_requested_complete_members() {
    let (adapter, root) = adapter("read-consistent");
    let key = key();
    seed_generation(
        &adapter,
        &key,
        &[
            member("active", b"a1"),
            member("archive", b"z1"),
            member("index", b"i1"),
        ],
    )
    .await;

    let DatasetReadOutcome::Found(read) = adapter
        .read_consistent(&key, &[name("active"), name("index")])
        .await
        .expect("read_consistent must succeed")
    else {
        panic!("a committed generation must expose the requested members");
    };
    assert_eq!(
        read.revision(),
        adapter.read_manifest(&key).await.unwrap().revision(),
        "the consistent read must carry the current generation revision"
    );
    let observed: Vec<(&str, &[u8])> = read
        .members()
        .iter()
        .map(|member| (member.name().as_str(), member.bytes()))
        .collect();
    assert_eq!(
        observed,
        vec![("active", b"a1".as_slice()), ("index", b"i1".as_slice())],
        "read_consistent must return exactly the requested members in canonical order"
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn stale_revision_commit_is_rejected_without_changing_current() {
    let (adapter, root) = adapter("stale-cas");
    let key = key();

    let stale_revision = adapter
        .read_manifest(&key)
        .await
        .unwrap()
        .revision()
        .clone();
    let live_revision = seed_generation(
        &adapter,
        &key,
        &[member("active", b"a1"), member("index", b"i1")],
    )
    .await;

    let error = adapter
        .commit_atomic(&key, &stale_revision, &[member("active", b"a2")], options())
        .await
        .expect_err("a commit against a stale revision must be rejected");
    assert_eq!(error.kind(), &StorageErrorKind::ConcurrentWrite);

    let manifest = adapter.read_manifest(&key).await.unwrap();
    assert_eq!(
        manifest.revision(),
        &live_revision,
        "a rejected CAS commit must leave the current revision untouched"
    );
    assert_eq!(
        member_names(manifest.members()),
        ["active", "index"],
        "a rejected CAS commit must leave the current members untouched"
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn read_previous_is_explicit_and_never_auto_fallback() {
    let (adapter, root) = adapter("read-previous");
    let key = key();

    seed_generation(
        &adapter,
        &key,
        &[
            member("active", b"a1"),
            member("archive", b"z1"),
            member("index", b"i1"),
        ],
    )
    .await;
    let current = adapter
        .read_manifest(&key)
        .await
        .unwrap()
        .revision()
        .clone();
    adapter
        .commit_atomic(
            &key,
            &current,
            &[member("active", b"a2"), member("index", b"i2")],
            options(),
        )
        .await
        .expect("replacement must commit and demote the old generation to previous");

    // The current generation never falls back to the previous one.
    assert_eq!(
        adapter
            .read_consistent(&key, &[name("archive")])
            .await
            .unwrap(),
        DatasetReadOutcome::NotFound,
        "read_consistent must never auto-fallback to the previous generation"
    );

    // The previous generation is explicitly readable as a complete member set.
    let DatasetReadOutcome::Found(previous) = adapter
        .read_previous(&key, &[name("active"), name("archive"), name("index")])
        .await
        .expect("read_previous must succeed")
    else {
        panic!("the retained previous generation must be explicitly readable");
    };
    let observed: Vec<(&str, &[u8])> = previous
        .members()
        .iter()
        .map(|member| (member.name().as_str(), member.bytes()))
        .collect();
    assert_eq!(
        observed,
        vec![
            ("active", b"a1".as_slice()),
            ("archive", b"z1".as_slice()),
            ("index", b"i1".as_slice()),
        ],
        "read_previous must return the complete pre-replacement member set"
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn promote_previous_restores_prior_generation() {
    let (adapter, root) = adapter("promote-previous");
    let key = key();

    let original = seed_generation(
        &adapter,
        &key,
        &[member("active", b"a1"), member("archive", b"z1")],
    )
    .await;
    let replacement = adapter
        .read_manifest(&key)
        .await
        .unwrap()
        .revision()
        .clone();
    adapter
        .commit_atomic(&key, &replacement, &[member("active", b"a2")], options())
        .await
        .expect("replacement must commit");

    let receipt = adapter
        .promote_previous(&key)
        .await
        .expect("promote_previous must succeed while a previous generation exists");
    assert_eq!(receipt.visibility(), DatasetCommitVisibility::Visible);
    assert_eq!(
        receipt.revision(),
        &original,
        "promotion must reinstate the original generation revision"
    );

    let manifest = adapter.read_manifest(&key).await.unwrap();
    assert_eq!(manifest.revision(), &original);
    assert_eq!(member_names(manifest.members()), ["active", "archive"]);

    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn quarantine_moves_requested_dataset_generation() {
    let (adapter, root) = adapter("quarantine-dataset");
    let key = key();
    seed_generation(
        &adapter,
        &key,
        &[member("active", b"a1"), member("index", b"i1")],
    )
    .await;

    let outcome = adapter
        .quarantine(
            &key,
            Generation::Primary,
            TransactionScope::Dataset,
            QuarantineReason::DecoderRejected,
        )
        .await
        .expect("quarantine must succeed for the current dataset generation");
    assert!(matches!(outcome, QuarantineOutcome::Moved(_)));
    assert_eq!(outcome.generation(), Generation::Primary);
    assert_eq!(outcome.scope(), TransactionScope::Dataset);
    assert_eq!(outcome.reason(), QuarantineReason::DecoderRejected);

    let manifest = adapter.read_manifest(&key).await.unwrap();
    assert!(
        manifest.members().is_empty(),
        "quarantining the primary generation must leave no live members"
    );

    let absent = adapter
        .quarantine(
            &key,
            Generation::Primary,
            TransactionScope::Dataset,
            QuarantineReason::DecoderRejected,
        )
        .await
        .unwrap();
    assert!(matches!(absent, QuarantineOutcome::AlreadyAbsent { .. }));

    std::fs::remove_dir_all(root).unwrap();
}
