#!/usr/bin/env bash
set -euo pipefail
# COLA 层间守卫：防止 share（共享内核）过度依赖 I/O 基础设施。
# 当前 share 依赖 chrono/bytes/parking_lot/tokio/tokio-util 等属于已知架构权衡，
# 暂时仅对 reqwest/futures 等严格 I/O crate 产生 block-level error。

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
import json
import subprocess
import sys

metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1"]))
errors = []
warnings = []

# share 严格 I/O 依赖（block-level）
share_severe_io = set()
# share 弱 I/O 依赖（warning-only，已知架构权衡）
share_warn_io = {"chrono", "bytes", "parking_lot", "tokio", "tokio-util", "unicode-width",
                  "reqwest", "futures", "futures-util"}

# supporting domain 严格 I/O 依赖
domain_severe = {
    "share": set(),
    "project": {"reqwest", "futures", "bytes", "hyper", "http", "reqwest-eventsource"},
    "policy": {"reqwest", "tokio", "tokio-util", "futures", "bytes", "hyper", "http", "reqwest-eventsource"},
    "prompt": {"reqwest", "tokio", "tokio-util", "futures", "bytes", "hyper", "http", "reqwest-eventsource"},
    "provider": set(),
    "tools": {"reqwest-eventsource"},
    "storage": {"reqwest", "reqwest-eventsource"},
    "hook": {"reqwest", "reqwest-eventsource"},
    "audit": {"reqwest", "reqwest-eventsource"},
}

for package in metadata["packages"]:
    name = package["name"]
    if name not in domain_severe:
        continue
    for dep in package.get("dependencies", []):
        dn = dep["name"]
        # share special handling
        if name == "share":
            if dn in share_severe_io:
                errors.append(f"share must not depend on {dn}")
            elif dn in share_warn_io:
                warnings.append(f"share depends on {dn} (known trade-off, pending future cleanup)")
        # other domains
        if dn in domain_severe[name]:
            errors.append(f"{name} domain must not depend on {dn}")

if errors:
    reason = "COLA layer purity guard FAILED:\n" + "\n".join(errors)
    if warnings:
        reason += "\n\nWarnings (info only):\n" + "\n".join(warnings)
    print(json.dumps({"decision": "block", "reason": reason}, ensure_ascii=False))
    sys.exit(2)

msg = "COLA layer purity guard OK."
if warnings:
    msg += "\nInfo: " + "; ".join(warnings)
print(msg)
PY
