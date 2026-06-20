#!/usr/bin/env bash
set -euo pipefail

# ── 用法检查 ──────────────────────────────────────────────────────
if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <tag>"
  echo "  e.g. $0 v0.1.0"
  exit 1
fi

TAG="$1"

# ── 格式校验 ──────────────────────────────────────────────────────
if [[ ! "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Error: tag must match vX.Y.Z (e.g. v0.1.0), got: $TAG" >&2
  exit 1
fi

VERSION="${TAG#v}"
echo "Release version: $VERSION"

# ── 获取 repo 信息 ───────────────────────────────────────────────
REPO=$(gh repo view --json nameWithOwner -q '.nameWithOwner')

# ── 获取远端 main SHA（无需 fetch）──────────────────────────────
ORIGIN_SHA=$(gh api "repos/$REPO/git/ref/heads/main" --jq '.object.sha')
ORIGIN_SHA_SHORT=${ORIGIN_SHA:0:7}

# ── 检查 tag 是否已存在 ──────────────────────────────────────────
if gh api "repos/$REPO/git/ref/tags/$TAG" &>/dev/null; then
  echo "Error: tag $TAG already exists on remote" >&2
  exit 1
fi

# ── 确认 ─────────────────────────────────────────────────────────
echo ""
echo "  Tag:         $TAG"
echo "  Commit:      $ORIGIN_SHA_SHORT (origin/main)"
echo "  Repo:        $REPO"
echo ""

read -rp "Create and push tag $TAG? [y/N] " CONFIRM
if [[ "$CONFIRM" != "y" && "$CONFIRM" != "Y" ]]; then
  echo "Aborted."
  exit 0
fi

# ── 在远端直接创建 tag ──────────────────────────────────────────
gh api "repos/$REPO/git/refs" -f ref="refs/tags/$TAG" -f sha="$ORIGIN_SHA" > /dev/null

echo ""
echo "Done. Tag $TAG created on remote ($ORIGIN_SHA_SHORT). CI release workflow triggered."
echo "Monitor: gh run list --workflow=release.yml"
