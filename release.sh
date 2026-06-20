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

# ── 拉取远端最新 ─────────────────────────────────────────────────
echo "Fetching origin..."
git fetch origin

# ── 检查 tag 是否已存在 ──────────────────────────────────────────
if git rev-parse "$TAG" >/dev/null 2>&1; then
  echo "Error: tag $TAG already exists (commit $(git rev-parse --short "$TAG"))" >&2
  exit 1
fi

# ── 确认 ─────────────────────────────────────────────────────────
ORIGIN_SHA=$(git rev-parse --short origin/main)
echo ""
echo "  Tag:         $TAG"
echo "  Commit:      $ORIGIN_SHA (origin/main)"
echo ""

read -rp "Create and push tag $TAG? [y/N] " CONFIRM
if [[ "$CONFIRM" != "y" && "$CONFIRM" != "Y" ]]; then
  echo "Aborted."
  exit 0
fi

# ── 在 origin/main 上打 tag 并推送 ──────────────────────────────
git tag "$TAG" origin/main
git push origin "$TAG"

echo ""
echo "Done. Tag $TAG pushed. CI release workflow triggered."
echo "Monitor: gh run list --workflow=release.yml"
