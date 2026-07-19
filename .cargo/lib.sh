#!/usr/bin/env bash
# Shared shell helpers for cargo target-dir management.

# Sanitize a label into a safe directory component.
# Usage: sanitized=$(sanitize_cache_label "$label")
sanitize_cache_label() {
    printf '%s' "$1" | tr '/\\ ' '_' | tr -cd 'A-Za-z0-9_.-'
}

# Return a stable cache key for one checkout/worktree.
# The readable branch label is diagnostic only; the canonical top-level path hash
# prevents collisions between same-branch and detached worktrees.
worktree_cache_key() {
    local top branch label digest
    top="${1:-$(git rev-parse --show-toplevel 2>/dev/null)}" || return 1
    branch="$(git -C "$top" branch --show-current 2>/dev/null || true)"
    label="$(sanitize_cache_label "${branch:-detached}")"
    [[ -n "$label" && "$label" != "." && "$label" != ".." ]] || label="detached"

    if command -v shasum >/dev/null 2>&1; then
        digest="$(printf '%s' "$top" | shasum -a 256 | awk '{print substr($1,1,16)}')"
    elif command -v sha256sum >/dev/null 2>&1; then
        digest="$(printf '%s' "$top" | sha256sum | awk '{print substr($1,1,16)}')"
    else
        echo "sha256 tool not found (need shasum or sha256sum)" >&2
        return 1
    fi

    printf '%s-%s\n' "$label" "$digest"
}
