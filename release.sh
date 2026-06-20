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

# ── 确保在 main 分支 ─────────────────────────────────────────────
BRANCH=$(git rev-parse --abbrev-ref HEAD)
if [[ "$BRANCH" != "main" ]]; then
  echo "Error: must be on main branch, currently on: $BRANCH" >&2
  exit 1
fi

# ── 检查工作区干净 ────────────────────────────────────────────────
if [[ -n "$(git status --porcelain)" ]]; then
  echo "Error: working tree has uncommitted changes:" >&2
  git status --short
  exit 1
fi

# ── 同步远端 ──────────────────────────────────────────────────────
echo "Pulling latest from origin/main..."
git pull origin main

# ── 检查 tag 是否已存在 ──────────────────────────────────────────
if git rev-parse "$TAG" >/dev/null 2>&1; then
  echo "Error: tag $TAG already exists (commit $(git rev-parse --short "$TAG"))" >&2
  exit 1
fi

# ── 确认 ─────────────────────────────────────────────────────────
LOCAL_SHA=$(git rev-parse --short HEAD)
ORIGIN_SHA=$(git rev-parse --short origin/main)
echo ""
echo "  Tag:     $TAG"
echo "  Commit:  $LOCAL_SHA (origin/main: $ORIGIN_SHA)"
echo ""

read -rp "Create and push tag $TAG? [y/N] " CONFIRM
if [[ "$CONFIRM" != "y" && "$CONFIRM" != "Y" ]]; then
  echo "Aborted."
  exit 0
fi

# ── 打 tag 并推送 ────────────────────────────────────────────────
git tag "$TAG"
git push origin "$TAG"

echo ""
echo "Done. Tag $TAG pushed. CI release workflow triggered."
echo "Monitor: gh run list --workflow=release.yml"
