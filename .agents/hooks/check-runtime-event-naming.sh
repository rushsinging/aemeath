#!/bin/bash
# guard-registry:policy.runtime.event-naming
set -euo pipefail

ROOT="${AEMEATH_GUARD_ROOT:-${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}}"
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import json
import re
import sys

root = Path.cwd()
baseline_path = root / ".agents/runtime-event-naming-baseline.json"
if not baseline_path.is_file():
    print("[event-naming] missing structured baseline: .agents/runtime-event-naming-baseline.json", file=sys.stderr)
    raise SystemExit(2)

baseline = json.loads(baseline_path.read_text())
violations: list[str] = []


def enum_body(source: str, enum_name: str) -> str | None:
    declaration = re.search(
        rf"\bpub(?:\([^)]*\))?\s+enum\s+{re.escape(enum_name)}\s*\{{",
        source,
    )
    if declaration is None:
        return None
    body_start = declaration.end()
    depth = 1
    cursor = body_start
    while cursor < len(source) and depth > 0:
        character = source[cursor]
        if character == "{":
            depth += 1
        elif character == "}":
            depth -= 1
        cursor += 1
    return source[body_start : cursor - 1] if depth == 0 else None


def top_level_variants(body: str) -> list[str]:
    variants: list[str] = []
    depth = 0
    for line in body.splitlines():
        if depth == 0:
            variant = re.match(
                r"\s*(?:#\[[^]]+\]\s*)*([A-Z][A-Za-z0-9_]*)\s*(?:\{|\(|,)",
                line,
            )
            if variant is not None:
                variants.append(variant.group(1))
        depth += line.count("{") - line.count("}")
    return variants


actual_by_layer: dict[str, set[str]] = {}
for layer_name, layer in baseline["layers"].items():
    source_path = root / layer["path"]
    if not source_path.is_file():
        violations.append(f"missing event source for {layer_name}: {layer['path']}")
        continue
    body = enum_body(source_path.read_text(), layer["enum"])
    if body is None:
        violations.append(f"missing enum {layer['enum']} in {layer['path']}")
        continue
    actual = set(top_level_variants(body))
    expected = set(layer["variants"])
    actual_by_layer[layer_name] = actual
    added = sorted(actual - expected)
    removed = sorted(expected - actual)
    if added:
        violations.append(
            f"unregistered {layer_name} event variants {added}; update the event index and structured baseline"
        )
    if removed:
        violations.append(
            f"stale {layer_name} event baseline {removed}; record rename/removal in the event index"
        )

index_path = root / baseline["index"]
if not index_path.is_file():
    violations.append(f"missing authoritative event index: {baseline['index']}")
    index_text = ""
else:
    index_text = index_path.read_text()

tui_container_only = set(baseline["tui_container_only_variants"])
for layer_name, layer in baseline["layers"].items():
    for variant in layer["variants"]:
        if layer_name == "tui" and variant in tui_container_only:
            continue
        if f"`{variant}`" not in index_text:
            violations.append(
                f"{layer_name} event {variant} is absent from {baseline['index']}"
            )

for fact in baseline["current_cross_layer_facts"]:
    for layer_name in ("runtime_stream", "sdk", "tui"):
        event_name = fact[{"runtime_stream": "runtime", "sdk": "sdk", "tui": "tui"}[layer_name]]
        if event_name not in actual_by_layer.get(layer_name, set()):
            violations.append(
                f"Current cross-layer fact drift: {layer_name} must retain {event_name}"
            )

compatibility_variants = set(baseline["sdk_compatibility_variants"])
sdk_actual = actual_by_layer.get("sdk", set())
runtime_actual = actual_by_layer.get("runtime_stream", set())
tui_actual = actual_by_layer.get("tui", set())
for event_name in sorted(compatibility_variants):
    if event_name not in sdk_actual:
        violations.append(f"stale SDK compatibility variant: {event_name}")
    if event_name in runtime_actual or event_name in tui_actual:
        violations.append(
            f"SDK compatibility variant must not remain a Runtime producer or TUI fact: {event_name}"
        )

all_actual = set().union(*actual_by_layer.values()) if actual_by_layer else set()
legacy_broad_names = set(baseline["legacy_broad_names"])
broad_name = re.compile(r"(?:Updated|Info|Data|Notification|ProgressUpdated)$")
for event_name in sorted(all_actual):
    if broad_name.search(event_name) and event_name not in legacy_broad_names:
        violations.append(
            f"new broad event name {event_name} is forbidden; use a controlled Subject + Fact suffix"
        )

for retired_symbol in baseline["retired_symbols"]:
    if retired_symbol in all_actual:
        violations.append(
            f"retired event {retired_symbol} must not return; use the canonical replacement recorded in the event index"
        )

ack_terminal_compatibility = set(baseline["ack_terminal_compatibility_names"])
ack_terminal_patterns = [re.compile(pattern) for pattern in baseline["ack_terminal_patterns"]]
for event_name in sorted(all_actual):
    if event_name in ack_terminal_compatibility:
        continue
    if any(pattern.search(event_name) for pattern in ack_terminal_patterns):
        violations.append(
            f"command ACK must not use lifecycle terminal naming: {event_name}"
        )

if violations:
    print("[architecture] Runtime event naming guard failed:", file=sys.stderr)
    for violation in violations:
        print(f"  - {violation}", file=sys.stderr)
    raise SystemExit(2)

print("Runtime event naming guard passed.")
PY
