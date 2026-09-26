#!/usr/bin/env bash
set -euo pipefail

export AEMEATH_LOG_LEVEL="${AEMEATH_LOG_LEVEL:-debug}"

# 本地 dev run 也带源码 revision 标识：未显式设置 AEMEATH_VERSION 时使用
# 0.0.0-<short commit>，与 build_cli.sh 的本地构建口径一致。`share::version()`
# 优先读运行时环境变量，因此这里注入无需重新编译。
if [[ -z "${AEMEATH_VERSION:-}" ]]; then
    commit="$(git rev-parse --short=8 HEAD 2>/dev/null || true)"
    export AEMEATH_VERSION="0.0.0-${commit:-unknown}"
    echo ">>> AEMEATH_VERSION 未设置 → 运行期版本号 = ${AEMEATH_VERSION}（dev run）"
fi

exec cargo run --bin aemeath -- "$@"
