//! Anti-corruption adapter for workspace data written by pre-envelope Sessions.
//!
//! Project owns the wire types and deterministic ID derivation (published from `share`).
//! Context only reproduces the read-only repository probe needed to translate old snapshots; it
//! deliberately does not depend on Project's private git port/adapter.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde_json::Value;
use share::session_types::{
    PersistedWorkspaceContext, PersistedWorkspaceFrame, ProjectIdentity, WorkspaceId, WorktreeKind,
};

use crate::domain::session::{DecodedSession, SessionCodec, SessionCodecError};

pub struct LegacySessionDecoder;

impl crate::ports::SessionDecoder for LegacySessionDecoder {
    fn decode(&self, bytes: &[u8]) -> Result<DecodedSession, SessionCodecError> {
        SessionCodec::decode_with_workspace_upgrade(bytes, upgrade)
    }
}

pub fn decode(bytes: &[u8]) -> Result<DecodedSession, SessionCodecError> {
    crate::ports::SessionDecoder::decode(&LegacySessionDecoder, bytes)
}

#[derive(Debug, Deserialize)]
struct LegacyWorkspace {
    #[serde(default)]
    workspace_id: Option<WorkspaceId>,
    #[serde(default)]
    project_identity: Option<ProjectIdentity>,
    path_base: String,
    #[serde(alias = "working_root")]
    workspace_root: String,
    #[serde(default)]
    worktree_kind: Option<WorktreeKind>,
    #[serde(default)]
    context_stack: Vec<LegacyWorkspaceFrame>,
}

#[derive(Debug, Deserialize)]
struct LegacyWorkspaceFrame {
    path_base: String,
    #[serde(alias = "working_root")]
    workspace_root: String,
    #[serde(default)]
    worktree_kind: Option<WorktreeKind>,
}

/// A legacy snapshot that already carries every published field, ready for lightweight validation.
struct CompleteWorkspace {
    workspace_id: WorkspaceId,
    project_identity: ProjectIdentity,
    path_base: String,
    workspace_root: String,
    worktree_kind: WorktreeKind,
    context_stack: Vec<PersistedWorkspaceFrame>,
}

#[derive(Debug)]
struct Probe {
    top_level: PathBuf,
    common_dir: Option<PathBuf>,
    kind: WorktreeKind,
}

/// `git` invocation pinned to the `C` locale so stderr sentinels (e.g. "not a git repository")
/// stay stable regardless of the ambient locale.
fn git_command() -> Command {
    let mut command = Command::new("git");
    command.env("LC_ALL", "C").env("LANG", "C");
    command
}

fn canonicalize(path: &Path) -> Result<PathBuf, SessionCodecError> {
    path.canonicalize().map_err(|error| match error.kind() {
        ErrorKind::NotFound => SessionCodecError::LegacyWorkspacePathNotFound {
            path: path.to_path_buf(),
        },
        ErrorKind::PermissionDenied => SessionCodecError::LegacyWorkspacePermissionDenied {
            path: path.to_path_buf(),
        },
        _ => SessionCodecError::LegacyWorkspaceCanonicalizeFailed {
            path: path.to_path_buf(),
        },
    })
}

/// Canonicalize `raw` and require that the stored value was *already* canonical, so a complete
/// (new-format) DTO cannot smuggle relative, symlinked, or otherwise unnormalized paths.
fn ensure_canonical(raw: &str) -> Result<PathBuf, SessionCodecError> {
    let path = Path::new(raw);
    let canonical = canonicalize(path)?;
    if canonical.as_os_str() != path.as_os_str() {
        return Err(SessionCodecError::LegacyWorkspacePathNotCanonical {
            path: path.to_path_buf(),
        });
    }
    Ok(canonical)
}

fn resolve_git_path(base: &Path, value: &str) -> Result<PathBuf, SessionCodecError> {
    let value = Path::new(value);
    let absolute = if value.is_absolute() {
        value.to_path_buf()
    } else {
        base.join(value)
    };
    canonicalize(&absolute)
}

fn probe(path: &Path) -> Result<Probe, SessionCodecError> {
    let output = git_command()
        .args([
            "rev-parse",
            "--show-toplevel",
            "--git-common-dir",
            "--git-dir",
        ])
        .current_dir(path)
        .output()
        .map_err(|error| match error.kind() {
            ErrorKind::NotFound => SessionCodecError::LegacyWorkspaceGitUnavailable,
            ErrorKind::PermissionDenied => {
                SessionCodecError::LegacyWorkspacePermissionDenied { path: path.into() }
            }
            _ => SessionCodecError::LegacyWorkspaceGitProbeFailed {
                path: path.into(),
                exit_code: None,
            },
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
        if stderr.contains("not a git repository") {
            return Ok(Probe {
                top_level: path.into(),
                common_dir: None,
                kind: WorktreeKind::NonGit,
            });
        }
        if stderr.contains("permission denied") {
            return Err(SessionCodecError::LegacyWorkspacePermissionDenied { path: path.into() });
        }
        return Err(SessionCodecError::LegacyWorkspaceGitProbeFailed {
            path: path.into(),
            exit_code: output.status.code(),
        });
    }

    let stdout = std::str::from_utf8(&output.stdout)
        .map_err(|_| SessionCodecError::LegacyWorkspaceInvalidGitOutput { path: path.into() })?;
    let mut lines = stdout.lines().map(str::trim);
    let top = lines
        .next()
        .filter(|line| !line.is_empty())
        .ok_or_else(|| SessionCodecError::LegacyWorkspaceInvalidGitOutput { path: path.into() })?;
    let common = lines
        .next()
        .filter(|line| !line.is_empty())
        .ok_or_else(|| SessionCodecError::LegacyWorkspaceInvalidGitOutput { path: path.into() })?;
    let git_dir = lines
        .next()
        .filter(|line| !line.is_empty())
        .ok_or_else(|| SessionCodecError::LegacyWorkspaceInvalidGitOutput { path: path.into() })?;
    if lines.next().is_some() {
        return Err(SessionCodecError::LegacyWorkspaceInvalidGitOutput { path: path.into() });
    }

    let top_level = canonicalize(Path::new(top))?;
    let common_dir = resolve_git_path(path, common)?;
    let git_dir = resolve_git_path(path, git_dir)?;
    let kind = if git_dir == common_dir {
        WorktreeKind::Primary
    } else {
        WorktreeKind::Linked
    };
    Ok(Probe {
        top_level,
        common_dir: Some(common_dir),
        kind,
    })
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn ensure_same_project(
    identity_probe: &Probe,
    location_probe: &Probe,
    path: &Path,
) -> Result<(), SessionCodecError> {
    if identity_probe.common_dir != location_probe.common_dir {
        return Err(SessionCodecError::LegacyWorkspaceRepositoryMismatch { path: path.into() });
    }
    Ok(())
}

/// Extract a snapshot that already carries every published field.
///
/// Returns `None` (falling through to the reconstructing backfill path) when any required field is
/// absent, so the caller never needs an input-derived `expect`.
fn take_complete(legacy: &LegacyWorkspace) -> Option<CompleteWorkspace> {
    let workspace_id = legacy
        .workspace_id
        .clone()
        .filter(|id| !id.as_str().is_empty())?;
    let project_identity = legacy
        .project_identity
        .clone()
        .filter(|identity| !identity.initial_cwd.is_empty())?;
    let worktree_kind = legacy.worktree_kind?;

    let mut context_stack = Vec::with_capacity(legacy.context_stack.len());
    for frame in &legacy.context_stack {
        let worktree_kind = frame.worktree_kind?;
        context_stack.push(PersistedWorkspaceFrame {
            path_base: frame.path_base.clone(),
            workspace_root: frame.workspace_root.clone(),
            worktree_kind,
        });
    }

    Some(CompleteWorkspace {
        workspace_id,
        project_identity,
        path_base: legacy.path_base.clone(),
        workspace_root: legacy.workspace_root.clone(),
        worktree_kind,
        context_stack,
    })
}

/// Validate a self-describing snapshot without repeating Project's full restore.
///
/// The checks are deliberately lightweight: cwd/identity agreement, deterministic id derivation,
/// and canonical (already-normalized, on-disk) paths. Repository topology re-verification stays the
/// responsibility of Project's `prepare_restore`.
fn validate_complete(
    complete: &CompleteWorkspace,
    cwd: Option<&str>,
) -> Result<(), SessionCodecError> {
    // (1) cwd identity agreement — pure string comparison first so an explicit conflict is
    // surfaced before any filesystem probing.
    if let Some(cwd) = cwd {
        let identity_cwd = complete.project_identity.initial_cwd.as_str();
        let canonical_cwd = Path::new(cwd).canonicalize().ok();
        let canonical_identity_cwd = Path::new(identity_cwd).canonicalize().ok();
        let same_location = cwd == identity_cwd
            || (canonical_cwd.is_some() && canonical_cwd == canonical_identity_cwd);
        if !same_location {
            return Err(SessionCodecError::LegacyCwdIdentityConflict {
                cwd: cwd.to_owned(),
                identity_cwd: identity_cwd.to_owned(),
            });
        }
    }

    // (2) deterministic id derivation consistency — reject fabricated / stale ids.
    let expected_id = WorkspaceId::derive(&complete.project_identity, &complete.workspace_root);
    if complete.workspace_id != expected_id {
        return Err(SessionCodecError::LegacyWorkspaceIdMismatch);
    }

    // (3) canonical, on-disk paths.
    ensure_canonical(&complete.project_identity.initial_cwd)?;
    ensure_canonical(&complete.workspace_root)?;
    ensure_canonical(&complete.path_base)?;
    for frame in &complete.context_stack {
        ensure_canonical(&frame.workspace_root)?;
        ensure_canonical(&frame.path_base)?;
    }

    Ok(())
}

/// Reconstruct a full DTO from a legacy snapshot that only carries the old `path_base` /
/// `workspace_root` fields, probing git to recover identity, id, and worktree kind.
fn backfill(
    cwd: Option<&str>,
    legacy: LegacyWorkspace,
) -> Result<PersistedWorkspaceContext, SessionCodecError> {
    let identity_source = cwd
        .or_else(|| {
            legacy
                .project_identity
                .as_ref()
                .map(|identity| identity.initial_cwd.as_str())
                .filter(|cwd| !cwd.is_empty())
        })
        .unwrap_or(&legacy.workspace_root);
    let initial_cwd = canonicalize(Path::new(identity_source))?;
    let identity_probe = probe(&initial_cwd)?;
    let workspace_root_input = canonicalize(Path::new(&legacy.workspace_root))?;
    let root_probe = probe(&workspace_root_input)?;
    let path_base = canonicalize(Path::new(&legacy.path_base))?;

    if root_probe.kind == WorktreeKind::NonGit {
        // NonGit projects have no repository to anchor identity, so a mismatch between cwd and the
        // captured root (or any nesting) cannot be reconciled — reject it with a typed error rather
        // than silently accepting an unrelated directory.
        if identity_probe.common_dir.is_some() {
            return Err(SessionCodecError::LegacyWorkspaceRepositoryMismatch {
                path: workspace_root_input,
            });
        }
        if initial_cwd != workspace_root_input {
            return Err(SessionCodecError::LegacyWorkspaceInvalidNonGitLayout {
                path: workspace_root_input,
            });
        }
        if !legacy.context_stack.is_empty() {
            return Err(SessionCodecError::LegacyWorkspaceInvalidNonGitLayout {
                path: workspace_root_input,
            });
        }
        if !path_base.starts_with(&workspace_root_input) {
            return Err(SessionCodecError::LegacyWorkspaceInvalidNonGitLayout { path: path_base });
        }

        let identity = ProjectIdentity {
            initial_cwd: path_text(&initial_cwd),
            git_common_dir: None,
        };
        let root = path_text(&workspace_root_input);
        return Ok(PersistedWorkspaceContext {
            workspace_id: WorkspaceId::derive(&identity, &root),
            project_identity: identity,
            path_base: path_text(&path_base),
            workspace_root: root,
            worktree_kind: WorktreeKind::NonGit,
            context_stack: Vec::new(),
        });
    }

    // Git-backed: anchor everything to the same repository and normalize each root to the probed
    // top-level so the derived id keys off the real workspace root, not the raw legacy string.
    ensure_same_project(&identity_probe, &root_probe, &workspace_root_input)?;
    ensure_same_project(&identity_probe, &probe(&path_base)?, &path_base)?;
    let workspace_root = root_probe.top_level.clone();

    let mut context_stack = Vec::with_capacity(legacy.context_stack.len());
    for frame in &legacy.context_stack {
        let frame_root_input = canonicalize(Path::new(&frame.workspace_root))?;
        let frame_probe = probe(&frame_root_input)?;
        ensure_same_project(&identity_probe, &frame_probe, &frame_root_input)?;
        let frame_base = canonicalize(Path::new(&frame.path_base))?;
        ensure_same_project(&identity_probe, &probe(&frame_base)?, &frame_base)?;
        context_stack.push(PersistedWorkspaceFrame {
            path_base: path_text(&frame_base),
            workspace_root: path_text(&frame_probe.top_level),
            worktree_kind: frame_probe.kind,
        });
    }

    let identity = ProjectIdentity {
        initial_cwd: path_text(&initial_cwd),
        git_common_dir: identity_probe.common_dir.as_deref().map(path_text),
    };
    let root = path_text(&workspace_root);
    Ok(PersistedWorkspaceContext {
        workspace_id: WorkspaceId::derive(&identity, &root),
        project_identity: identity,
        path_base: path_text(&path_base),
        workspace_root: root,
        worktree_kind: root_probe.kind,
        context_stack,
    })
}

/// Translate optional legacy `cwd`/workspace fields into the complete published DTO.
///
/// The boolean says whether workspace capture semantics were present, and is used by the envelope
/// ACL to distinguish old bare Sessions from snapshots whose absent task list means captured-empty.
pub(crate) fn upgrade(
    cwd: Option<String>,
    workspace: Option<Value>,
) -> Result<(Option<PersistedWorkspaceContext>, bool), SessionCodecError> {
    let Some(workspace_value) = workspace else {
        let Some(cwd) = cwd else {
            return Ok((None, false));
        };
        let initial_cwd = canonicalize(Path::new(&cwd))?;
        let repository = probe(&initial_cwd)?;
        let workspace_root = repository.top_level.clone();
        let identity = ProjectIdentity {
            initial_cwd: path_text(&initial_cwd),
            git_common_dir: repository.common_dir.as_deref().map(path_text),
        };
        let root = path_text(&workspace_root);
        return Ok((
            Some(PersistedWorkspaceContext {
                workspace_id: WorkspaceId::derive(&identity, &root),
                project_identity: identity,
                path_base: path_text(&initial_cwd),
                workspace_root: root,
                worktree_kind: repository.kind,
                context_stack: Vec::new(),
            }),
            true,
        ));
    };

    let legacy: LegacyWorkspace = serde_json::from_value(workspace_value)
        .map_err(|error| SessionCodecError::InvalidJson(error.to_string()))?;

    if let Some(complete) = take_complete(&legacy) {
        validate_complete(&complete, cwd.as_deref())?;
        return Ok((
            Some(PersistedWorkspaceContext {
                workspace_id: complete.workspace_id,
                project_identity: complete.project_identity,
                path_base: complete.path_base,
                workspace_root: complete.workspace_root,
                worktree_kind: complete.worktree_kind,
                context_stack: complete.context_stack,
            }),
            true,
        ));
    }

    let context = backfill(cwd.as_deref(), legacy)?;
    Ok((Some(context), true))
}
