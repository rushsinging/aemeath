#!/usr/bin/env bash
# Shared shell helpers for cargo target-dir management.

# Sanitize a git branch name into a safe directory component.
# Usage: sanitized=$(sanitize_branch_name "$branch")
sanitize_branch_name() {
    printf '%s' "$1" | tr '/\\ ' '_' | tr -cd 'A-Za-z0-9_.-'
}
