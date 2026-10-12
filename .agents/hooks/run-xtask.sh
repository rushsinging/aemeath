#!/usr/bin/env bash
# Hook 入口：优先复用已构建的 xtask 二进制，避免 `cargo run -p xtask`
# 在 Stop/pre-commit 路径上因 workspace dirty / 并发编译触发超时。
#
# 新鲜度策略：从 tools/xtask 出发 BFS 解析生产 path 依赖闭包
# （dependencies / build-dependencies，不含 dev-dependencies 与 crates.io），
# 对闭包内 *.rs + Cargo.toml 做内容指纹，与 debug/xtask.fingerprint 比对。
# 一致则直接 exec；不一致或缺二进制才 cargo build，成功后写 stamp。
# NEVER 用源码 mtime——checkout/rebase touch 会误触发重编。
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-}"
if [ -z "$ROOT" ] || [ ! -d "$ROOT/.agents/hooks" ]; then
  ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fi
cd "$ROOT"

resolve_target_dir() {
  if command -v cargo >/dev/null 2>&1; then
    cargo metadata --format-version 1 --no-deps 2>/dev/null \
      | python3 -c 'import sys, json; print(json.load(sys.stdin)["target_directory"])'
  else
    return 1
  fi
}

compute_fingerprint() {
  python3 - <<'PY'
import hashlib
import re
import sys
from pathlib import Path

root = Path('.').resolve()
start = root / 'tools' / 'xtask'
if not (start / 'Cargo.toml').is_file():
    sys.exit(2)

SECTION_RE = re.compile(r'^\s*\[([^\]]+)\]\s*$')
# Matches both `path = "..."` lines and inline tables: `foo = { path = "..." }`.
PATH_RE = re.compile(r'\bpath\s*=\s*"([^"]+)"')

def section_kind(name):
    # Accept dependencies / build-dependencies, including target.\'cfg(...)\'.* forms.
    # NEVER include dev-dependencies.
    parts = [p.strip() for p in name.split('.')]
    if not parts:
        return None
    leaf = parts[-1]
    if leaf == 'dev-dependencies':
        return None
    if leaf in ('dependencies', 'build-dependencies'):
        return leaf
    return None

def path_deps(cargo_toml):
    text = cargo_toml.read_text(encoding='utf-8')
    current = None
    out = []
    for line in text.splitlines():
        m = SECTION_RE.match(line)
        if m:
            current = section_kind(m.group(1))
            continue
        if current is None:
            continue
        for pm in PATH_RE.finditer(line):
            resolved = (cargo_toml.parent / pm.group(1)).resolve()
            out.append(resolved if resolved.is_dir() else resolved.parent)
    return out

seen = set()
queue = [start]
while queue:
    crate = queue.pop(0)
    if crate in seen:
        continue
    seen.add(crate)
    cargo_toml = crate / 'Cargo.toml'
    if not cargo_toml.is_file():
        continue
    for dep in path_deps(cargo_toml):
        queue.append(dep)

files = []
for crate in seen:
    files.extend(p for p in crate.rglob('*.rs') if p.is_file())
    files.extend(p for p in crate.rglob('Cargo.toml') if p.is_file())
files = sorted({p.resolve() for p in files})

digest = hashlib.sha256()
for path in files:
    rel = path.relative_to(root).as_posix()
    digest.update(rel.encode('utf-8'))
    digest.update(b'\0')
    digest.update(path.read_bytes())
    digest.update(b'\0')
print(digest.hexdigest())
PY
}

TARGET_DIR="$(resolve_target_dir || true)"
if [ -z "${TARGET_DIR:-}" ]; then
  # 门禁 fixture / 无 Cargo workspace：回退 cargo run，便于 PATH stub 拦截。
  exec cargo run --quiet -p xtask -- "$@"
fi

XTASK_BIN="$TARGET_DIR/debug/xtask"
STAMP="$TARGET_DIR/debug/xtask.fingerprint"

CURRENT_FP="$(compute_fingerprint || true)"
STORED_FP=""
if [ -f "$STAMP" ]; then
  STORED_FP="$(tr -d '[:space:]' < "$STAMP" || true)"
fi

need_build=0
if [ ! -x "$XTASK_BIN" ]; then
  need_build=1
elif [ -z "$CURRENT_FP" ]; then
  # 指纹失败时宁可重编，也不要默默复用可能过期的二进制。
  need_build=1
elif [ "$CURRENT_FP" != "$STORED_FP" ]; then
  need_build=1
fi

if [ "$need_build" -eq 1 ]; then
  cargo build --quiet -p xtask
  if [ -n "$CURRENT_FP" ] && [ -x "$XTASK_BIN" ]; then
    printf '%s\n' "$CURRENT_FP" > "$STAMP"
  fi
fi

if [ ! -x "$XTASK_BIN" ]; then
  exec cargo run --quiet -p xtask -- "$@"
fi

exec "$XTASK_BIN" "$@"
