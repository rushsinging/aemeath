#!/usr/bin/env python3
import re
import sys
from pathlib import Path

path = Path(sys.argv[1])
text = path.read_text()
lines = text.splitlines()
forbidden = re.compile(
    r"tokio\s*::\s*fs|std\s*::\s*(?:\{[^}]*\bfs\b|fs)|"
    r"\bread_to_string\s*\(|serde_json\s*::\s*(?:from_|to_)|"
    r"use\s+serde_json\s*::[^;]*(?:from_|to_)"
)
violations = []
test_next = False
test_depth = None
depth = 0
for lineno, line in enumerate(lines, 1):
    stripped = line.strip()
    if stripped == "#[cfg(test)]":
        test_next = True
        continue
    if test_depth is None and test_next:
        if re.search(r"\bmod\s+\w+\s*\{", line):
            test_depth = depth
            test_next = False
        elif stripped and not stripped.startswith("#"):
            test_next = False
    in_test = test_depth is not None
    if not in_test and forbidden.search(line):
        violations.append(f"{lineno}:{line}")
    depth += line.count("{") - line.count("}")
    if test_depth is not None and depth <= test_depth:
        test_depth = None
for violation in violations:
    print(violation)
sys.exit(2 if violations else 0)
