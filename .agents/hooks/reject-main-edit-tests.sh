#!/bin/bash
# Regression tests for reject-main-edit.sh.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HOOK="$SCRIPT_DIR/reject-main-edit.sh"

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

run_hook() {
    local file_path="$1"
    printf '{"tool_name":"Write","tool_input":{"file_path":"%s"}}' "$file_path" | "$HOOK" 2>&1
}

assert_blocks_without_creating_file() {
    local repo="$1"
    local target="$2"
    local output status
    set +e
    output="$(cd "$repo" && run_hook "$target")"
    status=$?
    set -e
    [ "$status" -eq 2 ] || fail "expected hook to block with exit 2, got $status; output=$output"
    [ ! -e "$target" ] || fail "hook must not create target file before blocking: $target"
    printf '%s' "$output" | grep -q '\[Hook blocked\]' || fail "blocked output should contain hook blocked marker: $output"
    if printf '%s' "$output" | grep -q 'No such file or directory'; then
        fail "hook output must not leak shell cd errors: $output"
    fi
}

main() {
    local tmp repo linked target output status
    tmp="$(mktemp -d)"
    trap "rm -rf '$tmp'" EXIT

    repo="$tmp/repo"
    mkdir -p "$repo/existing"
    git -C "$repo" init -q
    git -C "$repo" config user.email test@example.com
    git -C "$repo" config user.name Test
    touch "$repo/.keep"
    git -C "$repo" add .keep
    git -C "$repo" commit -q -m init

    target="$repo/existing/new-file.txt"
    assert_blocks_without_creating_file "$repo" "$target"

    target="$repo/missing-parent/new-file.txt"
    assert_blocks_without_creating_file "$repo" "$target"

    linked="$repo/.worktrees/linked"
    git -C "$repo" worktree add -q "$linked" -b linked-test
    target="$linked/missing-parent/new-file.txt"
    set +e
    output="$(cd "$repo" && run_hook "$target")"
    status=$?
    set -e
    [ "$status" -eq 0 ] || fail "linked worktree target with missing parent should be allowed, got $status; output=$output"
    if printf '%s' "$output" | grep -q 'No such file or directory'; then
        fail "allowed worktree path must not leak shell cd errors: $output"
    fi

    echo "reject-main-edit hook regression tests passed"
}

main "$@"
