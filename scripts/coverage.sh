#!/usr/bin/env bash
set -euo pipefail

readonly REQUIRED_LLVM_COV_VERSION="0.8.7"
readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$ROOT"

if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
    echo "error: cargo-llvm-cov $REQUIRED_LLVM_COV_VERSION is required" >&2
    echo "install: cargo install cargo-llvm-cov --version $REQUIRED_LLVM_COV_VERSION --locked" >&2
    exit 1
fi

installed_version="$(cargo llvm-cov --version | awk '{print $2}')"
if [[ "$installed_version" != "$REQUIRED_LLVM_COV_VERSION" ]]; then
    echo "error: cargo-llvm-cov $REQUIRED_LLVM_COV_VERSION is required (found $installed_version)" >&2
    exit 1
fi

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target/coverage}"
export CARGO_LLVM_COV_TARGET_DIR="${CARGO_LLVM_COV_TARGET_DIR:-$CARGO_TARGET_DIR/llvm-cov-target}"
export CARGO_LLVM_COV_BUILD_DIR="${CARGO_LLVM_COV_BUILD_DIR:-$CARGO_TARGET_DIR/llvm-cov-build}"

report_json="$(mktemp "${TMPDIR:-/tmp}/aemeath-coverage.XXXXXX.json")"
trap 'rm -f "$report_json"' EXIT

# 逐个 crate 串行跑 llvm-cov 并合并报告。
# 背景：llvm-cov 一次性 --workspace 时，其测试执行模型会让全部 crate 测试
# 同时启动（-j/--jobs 与 CARGO_BUILD_JOBS 均不影响其测试并行），CI runner
# 在测试运行中段（实测 tools 启动瞬间）取消 job（SIGTERM + "The operation
# was canceled"，本地不现、单/双 crate 不现、全量必现）。逐 crate 串行使任一
# 时刻只有一个测试进程，消除资源峰值；增量编译使重复编译成本可控。
crater_dir="$(mktemp -d "${TMPDIR:-/tmp}/aemeath-cov.XXXXXX")"
python3 - "$crater_dir" <<'PY'
import json, pathlib, subprocess, sys
out_dir = pathlib.Path(sys.argv[1])
metadata = json.loads(subprocess.run(
    ["cargo", "metadata", "--no-deps", "--format-version", "1"],
    capture_output=True, check=True).stdout)
crates = sorted(p["name"] for p in metadata["packages"] if p["name"] != "xtask")
for name in crates:
    print(name, flush=True)
(out_dir / "crates.txt").write_text("\n".join(crates) + "\n")
PY
while IFS= read -r crate; do
    echo "[coverage] running $crate ..." >&2
    echo "[coverage] disk: $(df -h / | awk 'NR==2 {print $4" free / "$2}') mem: $(free -m | awk '/Mem:/ {print $7" MB avail"}') procs: $(ps -e | wc -l | tr -d ' ')" >&2
    cargo llvm-cov -p "$crate" --quiet --json --summary-only \
        --output-path "$crater_dir/$crate.json" || exit 1
done < "$crater_dir/crates.txt"

python3 - "$crater_dir" "$report_json" <<'PY'
import json, pathlib, sys
crater_dir, report_json = pathlib.Path(sys.argv[1]), sys.argv[2]
merged_files = []
for path in sorted(crater_dir.glob("*.json")):
    if path.name == "crates.txt":
        continue
    report = json.loads(path.read_text())
    for data in report.get("data", []):
        merged_files.extend(data.get("files", []))
merged = {
    "data": [{"files": merged_files}],
    "type": "llvm.coverage.json.export",
    "version": "3.1.0",
}
pathlib.Path(report_json).write_text(json.dumps(merged))
PY

cargo run --quiet -p xtask -- coverage-summary "$report_json" "$ROOT"
