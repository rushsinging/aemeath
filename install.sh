#!/usr/bin/env bash
set -euo pipefail

# aemeath 安装脚本：从 GitHub Releases 下载对应平台的二进制并安装。
# 用法:
#   ./install.sh                      # 安装最新版本到 ~/.local/bin
#   ./install.sh --version v0.0.22    # 安装指定版本
#   ./install.sh --to /usr/local/bin  # 安装到指定目录
# 一键安装:
#   curl -fsSL https://raw.githubusercontent.com/rushsinging/aemeath/main/install.sh | bash

REPO="rushsinging/aemeath"
DEFAULT_INSTALL_DIR="$HOME/.local/bin"
BASE_URL="https://github.com/${REPO}/releases/download"

release_tag=""
install_dir="$DEFAULT_INSTALL_DIR"

die() {
  echo "Error: $*" >&2
  exit 1
}

usage() {
  cat <<'EOF'
Usage: install.sh [--version vX.Y.Z] [--to <dir>]

Options:
  --version vX.Y.Z   Install a specific version (default: latest release)
  --to <dir>         Install directory (default: ~/.local/bin)
  -h, --help         Show this help
EOF
}

# ── 参数解析 ─────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      [[ $# -ge 2 ]] || die "--version requires a value"
      release_tag="$2"
      shift 2
      ;;
    --to)
      [[ $# -ge 2 ]] || die "--to requires a value"
      install_dir="$2"
      shift 2
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      usage
      die "unknown argument: $1"
      ;;
  esac
done

command -v curl >/dev/null 2>&1 || die "curl is required but not found"

# ── 平台检测 ─────────────────────────────────────────────────────
os_type="$(uname -s)"
machine_arch="$(uname -m)"
case "${os_type}-${machine_arch}" in
  Darwin-arm64) target_triple="aarch64-apple-darwin" ;;
  Darwin-x86_64) target_triple="x86_64-apple-darwin" ;;
  Linux-aarch64 | Linux-arm64) target_triple="aarch64-unknown-linux-gnu" ;;
  Linux-x86_64 | Linux-amd64) target_triple="x86_64-unknown-linux-gnu" ;;
  *) die "unsupported platform: ${os_type}-${machine_arch}" ;;
esac

# ── 版本解析 ─────────────────────────────────────────────────────
if [[ -z "$release_tag" ]]; then
  release_tag="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" |
    sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)"
  [[ -n "$release_tag" ]] || die "failed to resolve latest release tag"
fi

version_number="${release_tag#v}"
[[ "$version_number" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "invalid version: ${release_tag}"

# ── 下载与校验 ───────────────────────────────────────────────────
asset_name="aemeath-${version_number}-${target_triple}.tar.gz"
download_dir="$(mktemp -d)"
trap 'rm -rf "$download_dir"' EXIT

echo "==> Platform:  ${os_type}-${machine_arch} (${target_triple})"
echo "==> Version:   v${version_number}"
echo "==> Target:    ${install_dir}/aemeath"
echo ""

echo "==> Downloading ${asset_name} ..."
curl -fsSL -o "${download_dir}/${asset_name}" "${BASE_URL}/v${version_number}/${asset_name}"
curl -fsSL -o "${download_dir}/checksums.txt" "${BASE_URL}/v${version_number}/checksums.txt"

case "$os_type" in
  Darwin) hash_tool="shasum -a 256" ;;
  Linux) hash_tool="sha256sum" ;;
esac

actual_hash="$($hash_tool "${download_dir}/${asset_name}" | awk '{print $1}')"
expected_hash="$(awk -v asset="$asset_name" '{name=$2; gsub(/^\*/, "", name); if (name == asset) print $1}' "${download_dir}/checksums.txt")"
[[ -n "$expected_hash" ]] || die "no checksum entry for ${asset_name} in checksums.txt"
[[ "$actual_hash" = "$expected_hash" ]] || die "checksum mismatch: expected ${expected_hash}, got ${actual_hash}"
echo "==> Checksum OK"

# ── 解包安装 ─────────────────────────────────────────────────────
tar xzf "${download_dir}/${asset_name}" -C "$download_dir"
binary_source="${download_dir}/aemeath-${version_number}-${target_triple}/aemeath"
[[ -x "$binary_source" ]] || die "binary not found in tarball: ${binary_source}"

mkdir -p "$install_dir"
install -m 755 "$binary_source" "${install_dir}/aemeath"

echo "==> Installed: ${install_dir}/aemeath"

# ── PATH 检测 ────────────────────────────────────────────────────
case ":${PATH}:" in
  *":${install_dir}:"*) ;;
  *)
    echo ""
    echo "WARNING: ${install_dir} is not in your PATH."
    echo "Add this line to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
    echo "  export PATH=\"${install_dir}:\$PATH\""
    ;;
esac

echo ""
echo "==> Run '${install_dir}/aemeath --version' to verify."
