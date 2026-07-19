#!/usr/bin/env bash
# Initialize aemeath development environment (companion to #1226).
#
# Covers:
#   1. Rust toolchain (rustup / stable / llvm-tools-preview)
#   2. cargo-llvm-cov 0.8.7 (CI coverage.yml pins this version)
#   3. direnv (activates .envrc -> set-target.sh per-branch target isolation)
#   4. sccache (cross-branch compile cache, wired as rustc-wrapper)
#   5. git core.hooksPath = .cargo/hooks (pre-commit)
#   6. Verification summary
#
# Idempotent: re-running only fills gaps, never overwrites existing config.
# User-space only: everything lands under ~/.cargo / ~/.cache, no sudo.
#
# Usage:
#   ./scripts/setup-dev-env.sh           # detect and fill gaps
#   ./scripts/setup-dev-env.sh --check   # check only, no install (CI-friendly)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
readonly COV_VERSION="0.8.7"

CHECK_ONLY=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --check) CHECK_ONLY=1; shift ;;
    -h|--help)
      sed -n '2,18p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 1 ;;
  esac
done

# Color (degrades when not a tty).
if [[ -t 1 ]]; then
  GREEN=$'\033[32m'; YELLOW=$'\033[33m'; RED=$'\033[31m'; DIM=$'\033[2m'; RESET=$'\033[0m'
else
  GREEN=""; YELLOW=""; RED=""; DIM=""; RESET=""
fi

ok()    { echo "${GREEN}[ok]${RESET}    $*"; }
skip()  { echo "${DIM}[skip]${RESET}   $*"; }
inst()  { echo "${YELLOW}[install]${RESET} $*"; }
manual(){ echo "${RED}[manual]${RESET}  $*"; }
header(){ echo; echo "==> $*"; }

# run_or_hint "<check-cmd>" "<install-cmd>" "<desc>"
# Installs only when check-cmd fails and not in --check mode.
run_or_hint() {
  local check_cmd="$1" install_cmd="$2" desc="$3"
  if eval "$check_cmd" >/dev/null 2>&1; then
    skip "$desc (ready)"
    return 0
  fi
  if [[ $CHECK_ONLY -eq 1 ]]; then
    manual "$desc missing. Install: $install_cmd"
    return 1
  fi
  inst "$desc"
  if eval "$install_cmd"; then
    ok "$desc done"
  else
    manual "$desc failed, run manually: $install_cmd"
    return 1
  fi
}

header "1/6 Rust toolchain (rustup / stable)"
if command -v rustup >/dev/null 2>&1; then
  ok "rustup installed"
  # llvm-tools-preview is a hard dependency of cargo-llvm-cov / coverage.
  run_or_hint \
    "rustup component list --installed 2>/dev/null | grep -q '^llvm-tools-preview$'" \
    "rustup component add llvm-tools-preview" \
    "llvm-tools-preview component" || true
else
  manual "rustup not installed. Install Rust first:"
  echo "    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  if [[ $CHECK_ONLY -eq 1 ]]; then exit 1; fi
  echo "  Re-run this script after installing."
  exit 1
fi

header "2/6 cargo-llvm-cov ${COV_VERSION}"
run_or_hint \
  "cargo llvm-cov --version 2>/dev/null | grep -qF '${COV_VERSION}'" \
  "cargo install cargo-llvm-cov --version ${COV_VERSION} --locked" \
  "cargo-llvm-cov ${COV_VERSION}" || true

header "3/6 direnv (activate .envrc -> per-branch target)"
if command -v direnv >/dev/null 2>&1; then
  ok "direnv installed"
else
  if command -v brew >/dev/null 2>&1; then
    run_or_hint "command -v direnv" "brew install direnv" "direnv" || true
  else
    manual "direnv not installed and brew missing. See https://direnv.net/docs/installation.html"
  fi
fi
if [[ -f "$ROOT/.envrc" ]]; then
  echo "${DIM}Note: ensure direnv shell hook is loaded (zsh: eval \"\$(direnv hook zsh)\"),"
  echo "      then run 'direnv allow' at repo root.${RESET}"
fi

header "4/6 sccache (cross-branch compile cache)"
if command -v sccache >/dev/null 2>&1; then
  ok "sccache installed"
else
  if command -v brew >/dev/null 2>&1; then
    run_or_hint "command -v sccache" "brew install sccache" "sccache" || true
  else
    manual "sccache not installed and brew missing. See https://github.com/mozilla/sccache#installing"
  fi
fi

header "5/6 wire sccache as rustc-wrapper (~/.cargo/config.toml)"
# 只在 sccache 实际可用时才配 wrapper，否则会让 cargo 调用全部失败。
if ! command -v sccache >/dev/null 2>&1; then
  manual "sccache not installed; skipping rustc-wrapper config (would break cargo)."
  echo "${DIM}      Install sccache first (step 4), then re-run.${RESET}"
else
  if [[ $CHECK_ONLY -eq 1 ]]; then
    skip "--check mode: not modifying ~/.cargo/config.toml"
  else
    mkdir -p "$HOME/.cargo"
    cfg="$HOME/.cargo/config.toml"
    touch "$cfg"
    if grep -qE 'rustc-wrapper[[:space:]]*=[[:space:]]*"sccache"' "$cfg" 2>/dev/null; then
      skip "rustc-wrapper = sccache (already configured)"
    else
      inst "appending [build] rustc-wrapper = \"sccache\" to $cfg"
      if grep -qE '^\[build\]' "$cfg"; then
        # Existing [build] section without rustc-wrapper: insert right after the section header.
        tmp="$(mktemp)"
        awk -v line='rustc-wrapper = "sccache"' '
          /^\[build\]/ && !done { print; print line; done=1; next }
          { print }
        ' "$cfg" > "$tmp"
        mv "$tmp" "$cfg"
      else
        printf '\n[build]\nrustc-wrapper = "sccache"\n' >> "$cfg"
      fi
      ok "configured rustc-wrapper = sccache"
    fi
    echo "${DIM}Note: sccache server starts lazily on first build. Hit rate: sccache --show-stats"
    echo "      LRU default cap 10G, auto-evicts oldest. Tune via SCCACHE_DIR / SCCACHE_CACHE_SIZE.${RESET}"
  fi
fi

header "6/6 git pre-commit hook (core.hooksPath = .cargo/hooks)"
cd "$ROOT"
current_hooks="$(git config core.hooksPath 2>/dev/null || true)"
# worktree shares main repo config; relative path resolves at repo root.
if [[ "$current_hooks" == ".cargo/hooks" || "$current_hooks" == */.cargo/hooks ]]; then
  skip "core.hooksPath = .cargo/hooks (configured: $current_hooks)"
else
  if [[ $CHECK_ONLY -eq 1 ]]; then
    manual "core.hooksPath does not point to .cargo/hooks"
  else
    git config core.hooksPath .cargo/hooks
    ok "core.hooksPath = .cargo/hooks (pre-commit: fmt + source-guard)"
  fi
fi

header "Verification summary"
echo "rustc:          $(rustc --version 2>/dev/null || echo MISSING)"
echo "cargo:          $(cargo --version 2>/dev/null || echo MISSING)"
echo "cargo-llvm-cov: $(cargo llvm-cov --version 2>/dev/null || echo MISSING)"
echo "direnv:         $(direnv --version 2>/dev/null || echo MISSING)"
echo "sccache:        $(sccache --version 2>/dev/null | head -1 || echo MISSING)"
echo "hooksPath:      $(git config core.hooksPath 2>/dev/null || echo unset)"

echo
echo "${GREEN}Dev environment ready.${RESET}"
echo "Next steps:"
echo "  - direnv allow (first time entering a worktree)"
echo "  - clean target bloat: ./scripts/clean-worktree-targets.sh --dry-run"
