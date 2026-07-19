#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import re
import sys

root = Path.cwd()
violations = []

config = (root / "agent/features/config/src/application.rs").read_text()
order = [
    'let config = share::config::domain::merge::apply_patch(base, override_patch.clone())',
    'let env_patch = EnvAdapter::read(self.env_source.as_ref())',
    'let config = share::config::domain::merge::apply_patch(config, cli_patch)',
]
pos = [config.find(item) for item in order]
if any(value < 0 for value in pos) or pos != sorted(pos):
    violations.append("Config update must apply Local dynamic patch, then Env, then CLI")

policy = (root / "agent/features/policy/src/domain.rs").read_text()
if not re.search(r"Allow\s*\(\s*AuthorizationContext\s*\)", policy):
    violations.append("PolicyDecision::Allow must carry AuthorizationContext")

for path in (root / "agent/features/project/src").rglob("*.rs"):
    if "test" in path.name or "tests" in path.parts:
        continue
    text = path.read_text()
    if re.search(r"\b(?:ConfigReader|PermissionMode|PolicyPort|allow_all)\b", text):
        violations.append(f"{path.relative_to(root)}: Project must not read permission/config state")

for path in (root / "agent/features/tools/src/adapters").rglob("*.rs"):
    if "test" in path.name or "tests" in path.parts:
        continue
    text = path.read_text()
    if re.search(r"\ballow_all\b", text):
        violations.append(f"{path.relative_to(root)}: Tool adapter must consume AuthorizationContext, not allow_all")

legacy = [
    root / "agent/features/runtime/src/domain/state/settings.rs",
]
for path in legacy:
    if path.exists():
        violations.append(f"{path.relative_to(root)}: legacy Runtime permission Settings must stay retired")

tools_types = (root / "agent/features/tools/src/domain/tool_types.rs").read_text()
if re.search(r"enum\s+PolicyDecision\b", tools_types):
    violations.append("Tools duplicate PolicyDecision must stay retired")

if violations:
    print("\n".join(violations), file=sys.stderr)
    sys.exit(2)
print("Unified authorization guard OK.")
PY
