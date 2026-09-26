use crate::domain::{PreparedTaskRestoreData, TaskSnapshotData};

/// Narrow, TaskData-owned persistence capability: capture a coherent image,
/// validate a candidate, then install it.
///
/// This port is the sole published boundary for TaskData BC persistence. It never
/// exposes the backing store, aggregate state, or the crate-private
/// capture/prepare/install plumbing. All methods are synchronous because the
/// in-memory transaction contains no I/O; implementations must release any
/// state guard before returning.
///
/// The three steps are deliberately separated so a caller can validate a
/// candidate ([`prepare_restore`](Self::prepare_restore)) without touching live
/// state and only later, infallibly, install it
/// ([`commit_restore`](Self::commit_restore)). The [`PreparedTaskRestoreData`] token
/// is opaque and non-`Clone`, so it can be committed at most once.
pub trait TaskPersist: Send + Sync {
    /// Captures one coherent persistence image of the current aggregate. Deleted
    /// tombstones and runtime-only reverse indexes are excluded.
    fn collect_snapshot(&self) -> TaskSnapshotData;

    /// Validates `snapshot` against every aggregate invariant and, only on
    /// success, produces a single-use [`PreparedTaskRestoreData`] token. The live
    /// backing is never read or mutated: the snapshot is cloned before
    /// validation, so a rejected candidate leaves both the snapshot and the
    /// store untouched.
    fn prepare_restore(
        &self,
        snapshot: &TaskSnapshotData,
    ) -> Result<PreparedTaskRestoreData, share::error::DomainError>;

    /// Installs an already validated candidate, replacing the whole aggregate in
    /// one infallible step. Consuming the token by value guarantees a prepared
    /// candidate is committed at most once.
    fn commit_restore(&self, token: PreparedTaskRestoreData);
}
