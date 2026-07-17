#!/usr/bin/env bash
set -euo pipefail

# 功能：守护 agent context 所有权重构（project 拥有 WorkspaceState）的架构不变量。
# 作用：固化设计 docs/superpowers/specs/2026-06-07-agent-context-ownership-redesign.md
#       的「架构 Guard」——workspace 真相单一所有者在 project，tools 只用读/控能力，
#       持久化 DTO 留 session 边界，git 收敛在 GitCli。
# 规则：
#   R1 ToolExecutionContext 定义不得含 working_root / path_base / context_stack / cwd 字段。
#   R2 tools 不得引用 PersistedWorkspaceContext / WorkspacePersist（持久化是 session 边界）。
#   R3 仅 project 可定义 struct WorkspaceState；agent/features 内（project 除外）任何 struct
#      不得同时打包 working_root + path_base + (context_stack|stack)（防 WorktreeWorkingContext 复活）。
#   R4 生产代码调 .workspace_control() 仅限 tools 的 bash.rs / worktree.rs。
#   R5 project 内非测试 Command::new("git") 仅限 adapters/git.rs。
#   R6 WorkspacePersist 仅可出现在 project（def/impl）与 runtime；tools 禁用（与 R2 重叠）。
#   R7 ToolExecutionContext / ChatLoopContext 定义不得含 cwd 字段（从 workspace 读取）。
# 例外：测试文件 / #[cfg(test)] 区域对 R4 / R5 / R6 放行。
# 说明（narrowing）：R3 的 triple-bundle 检测限定 agent/features（project 除外），不扫
#   agent/shared（持久化 DTO PersistedWorkspaceContext）与 packages/sdk（WorkspaceContextView 视图），
#   这两者是设计允许的序列化/投影形态，不是运行期可变三元组。

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
# 守卫：如果 AEMEATH_PROJECT_DIR 不含 .agents/hooks，回退到 BASH_SOURCE 推导。
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fi
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import json
import re
import sys

root = Path.cwd()

TOOLS_DIR = "agent/features/tools/"
PROJECT_DIR = "agent/features/project/"
RUNTIME_DIR = "agent/features/runtime/"
FEATURES_DIR = "agent/features/"

TOOL_CTX_FILE = Path("agent/features/tools/src/contract/context.rs")
GIT_OPS_FILE = Path("agent/features/project/src/adapters/git.rs")
# 唯一允许出现生产 .workspace_control() 调用的文件。
WORKSPACE_CONTROL_ALLOWED = {
    Path("agent/features/tools/src/business/bash.rs"),
    Path("agent/features/tools/src/business/worktree.rs"),
}

TRIPLE_FIELDS_REQUIRED = ("working_root", "path_base")
TRIPLE_FIELDS_STACK = ("context_stack", "stack")

field_re = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*:")
struct_open_re = re.compile(r"\bstruct\s+([A-Za-z_][A-Za-z0-9_]*)\b")
workspace_state_re = re.compile(r"\bstruct\s+WorkspaceState\b")
command_git_re = re.compile(r'Command::new\(\s*"git"\s*\)')
workspace_control_call_re = re.compile(r"\.workspace_control\s*\(")
persisted_ctx_re = re.compile(r"\bPersistedWorkspaceContext\b")
workspace_persist_re = re.compile(r"\bWorkspacePersist\b")


def is_test_path(path: Path) -> bool:
    name = path.name
    return name.endswith("_test.rs") or name.endswith("_tests.rs") or "tests" in path.parts


def is_generated(path: Path) -> bool:
    rel = path.as_posix()
    return "/target/" in rel or rel.startswith("target/")


def strip_comment(line: str) -> str:
    return line.split("//", 1)[0]


def iter_struct_blocks(text: str):
    """Yield (struct_name, [field_lines]) for brace-delimited struct defs."""
    lines = text.splitlines()
    i = 0
    n = len(lines)
    while i < n:
        code = strip_comment(lines[i])
        m = struct_open_re.search(code)
        if m and "{" in code[m.end():]:
            name = m.group(1)
            depth = code.count("{") - code.count("}")
            body: list[str] = []
            tail = code[code.index("{") + 1:]
            if tail.strip():
                body.append(tail)
            i += 1
            while i < n and depth > 0:
                cl = strip_comment(lines[i])
                depth += cl.count("{") - cl.count("}")
                if depth > 0:
                    body.append(cl)
                else:
                    head = cl.split("}", 1)[0]
                    if head.strip():
                        body.append(head)
                i += 1
            yield name, body
        else:
            i += 1


def struct_field_names(body: list[str]) -> set[str]:
    names: set[str] = set()
    for line in body:
        # 同一行可能有逗号分隔的多个字段（紧凑写法），逐段匹配。
        for segment in line.replace("}", "").split(","):
            fm = field_re.match(segment)
            if fm:
                names.add(fm.group(1))
    return names


def in_test_region(text: str) -> set[int]:
    """Return set of 1-based line numbers that fall inside a #[cfg(test)] mod/region.

    Heuristic: once a `#[cfg(test)]` attribute is seen, the following item (and its
    braced body) is treated as test-only; brace counting bounds the region.
    """
    test_lines: set[int] = set()
    lines = text.splitlines()
    n = len(lines)
    i = 0
    while i < n:
        if re.search(r"#\[\s*cfg\s*\(\s*test\s*\)\s*\]", lines[i]):
            # advance to the start of the guarded item's body
            j = i + 1
            # skip further attributes / blank lines
            while j < n and (lines[j].strip().startswith("#[") or not lines[j].strip()):
                j += 1
            # find opening brace of the item
            while j < n and "{" not in lines[j]:
                test_lines.add(j + 1)
                j += 1
            if j < n:
                depth = lines[j].count("{") - lines[j].count("}")
                test_lines.add(j + 1)
                j += 1
                while j < n and depth > 0:
                    depth += lines[j].count("{") - lines[j].count("}")
                    test_lines.add(j + 1)
                    j += 1
            i = j
        else:
            i += 1
    return test_lines


def check_r1(violations: list[str]) -> None:
    path = root / TOOL_CTX_FILE
    if not path.exists():
        return
    text = path.read_text()
    for name, body in iter_struct_blocks(text):
        if name != "ToolExecutionContext":
            continue
        fields = struct_field_names(body)
        for forbidden in ("working_root", "path_base", "context_stack", "cwd"):
            if forbidden in fields:
                violations.append(
                    f"{TOOL_CTX_FILE}: [R1] ToolExecutionContext must not carry workspace field "
                    f"`{forbidden}`; hold Arc<WorkspaceService> and read via workspace_read()/workspace_control()."
                )


def check_r7(violations: list[str]) -> None:
    """R7: ToolExecutionContext / ChatLoopContext must not carry `cwd` field."""
    targets = {
        Path("agent/features/tools/src/contract/context.rs"): "ToolExecutionContext",
        Path("agent/features/runtime/src/application/chat/looping/loop_runner.rs"): "ChatLoopContext",
    }
    for path, target_name in targets.items():
        full = root / path
        if not full.exists():
            continue
        text = full.read_text()
        for name, body in iter_struct_blocks(text):
            if name != target_name:
                continue
            fields = struct_field_names(body)
            if "cwd" in fields:
                violations.append(
                    f"{path}: [R7] {target_name} must not carry `cwd` field; "
                    f"read from workspace_read().current_workspace_root() instead."
                )


def check_r2_r6(rel: Path, lineno: int, code: str, is_test: bool, violations: list[str]) -> None:
    rel_s = rel.as_posix()
    # R2: tools must not reference the persistence DTO or persist port (session-boundary only).
    if rel_s.startswith(TOOLS_DIR) and not is_test:
        if persisted_ctx_re.search(code):
            violations.append(
                f"{rel_s}:{lineno}: [R2] tools must not reference PersistedWorkspaceContext "
                f"(persistence is a session boundary; use WorkspaceRead/WorkspaceControl)."
            )
        if workspace_persist_re.search(code):
            violations.append(
                f"{rel_s}:{lineno}: [R2/R6] tools must not reference WorkspacePersist "
                f"(persistence belongs to runtime session boundary, not tools)."
            )
    # R6: WorkspacePersist allowed only in project (def/impl) and runtime.
    if workspace_persist_re.search(code) and not is_test:
        allowed = rel_s.startswith(PROJECT_DIR) or rel_s.startswith(RUNTIME_DIR)
        if not allowed:
            violations.append(
                f"{rel_s}:{lineno}: [R6] WorkspacePersist may only appear in project (def/impl) "
                f"or runtime; found outside both."
            )


def check_r4(rel: Path, lineno: int, code: str, is_test: bool, violations: list[str]) -> None:
    if is_test:
        return
    if workspace_control_call_re.search(code) and rel not in WORKSPACE_CONTROL_ALLOWED:
        violations.append(
            f"{rel.as_posix()}:{lineno}: [R4] production .workspace_control() calls are restricted to "
            f"tools/src/business/bash.rs and worktree.rs."
        )


def check_r5(rel: Path, lineno: int, code: str, is_test: bool, violations: list[str]) -> None:
    rel_s = rel.as_posix()
    if not rel_s.startswith(PROJECT_DIR) or is_test:
        return
    if command_git_re.search(code) and rel != GIT_OPS_FILE:
        violations.append(
            f"{rel_s}:{lineno}: [R5] within project, Command::new(\"git\") is allowed only in "
            f"adapters/git.rs (GitCli adapter); route git through GitWorktreeOps."
        )


def check_r3_struct(rel: Path, text: str, violations: list[str]) -> None:
    rel_s = rel.as_posix()
    in_features = rel_s.startswith(FEATURES_DIR)
    in_project = rel_s.startswith(PROJECT_DIR)
    for name, body in iter_struct_blocks(text):
        # R3a: struct WorkspaceState may be defined only under project (within agent/features).
        if name == "WorkspaceState" and in_features and not in_project:
            violations.append(
                f"{rel_s}: [R3] struct WorkspaceState may be defined only under agent/features/project/."
            )
        # R3b: no triple-bundle struct (working_root + path_base + stack/context_stack)
        #      anywhere in agent/features except project.
        if in_features and not in_project:
            fields = struct_field_names(body)
            if all(f in fields for f in TRIPLE_FIELDS_REQUIRED) and any(
                f in fields for f in TRIPLE_FIELDS_STACK
            ):
                violations.append(
                    f"{rel_s}: [R3] struct `{name}` re-bundles working_root + path_base + stack; "
                    f"workspace truth must live only in project::WorkspaceState."
                )


def run_sanity() -> None:
    # R1 struct field parsing (multi-line, like real code)
    blocks = dict(iter_struct_blocks(
        "pub struct X {\n    pub a: u8,\n    pub working_root: PathBuf,\n}\n"
    ))
    assert "working_root" in struct_field_names(blocks.get("X", [])), "sanity R1 field parse"
    # compact single-line struct field parsing
    one = dict(iter_struct_blocks("struct Y { a: u8, path_base: PathBuf }\n"))
    assert "path_base" in struct_field_names(one["Y"]), "sanity R1 compact field parse"
    # triple detection
    triple_blocks = dict(iter_struct_blocks(
        "struct Bad {\n  path_base: PathBuf,\n  working_root: PathBuf,\n  stack: Vec<u8>,\n}\n"
    ))
    bad_fields = struct_field_names(triple_blocks["Bad"])
    assert all(f in bad_fields for f in TRIPLE_FIELDS_REQUIRED) and any(
        f in bad_fields for f in TRIPLE_FIELDS_STACK
    ), "sanity R3 triple parse"
    # DTO-style without stack must not be flagged as triple
    dto_blocks = dict(iter_struct_blocks(
        "struct View {\n  path_base: String,\n  working_root: String,\n}\n"
    ))
    dto_fields = struct_field_names(dto_blocks["View"])
    assert not (all(f in dto_fields for f in TRIPLE_FIELDS_REQUIRED) and any(
        f in dto_fields for f in TRIPLE_FIELDS_STACK
    )), "sanity R3 non-triple"
    # regex sanity
    assert workspace_control_call_re.search("ctx.workspace_control().set_cwd(p)")
    assert command_git_re.search('std::process::Command::new("git")')
    assert persisted_ctx_re.search("use share::session_types::PersistedWorkspaceContext;")
    assert workspace_persist_re.search("project::api::WorkspacePersist::snapshot(x)")
    # test-region heuristic
    tr = in_test_region("#[cfg(test)]\nmod tests {\n  fn a() {}\n}\nfn prod() {}\n")
    assert 3 in tr and 5 not in tr, "sanity test-region"


run_sanity()

violations: list[str] = []
check_r1(violations)
check_r7(violations)

for path in sorted((root / "agent" / "features").rglob("*.rs")):
    if is_generated(path):
        continue
    rel = path.relative_to(root)
    text = path.read_text()
    test_path = is_test_path(path)
    test_lines = set() if test_path else in_test_region(text)
    # struct-level rule (R3) — skip test files entirely.
    if not test_path:
        check_r3_struct(rel, text, violations)
    for lineno, raw in enumerate(text.splitlines(), 1):
        code = strip_comment(raw)
        if not code.strip():
            continue
        line_is_test = test_path or (lineno in test_lines)
        check_r2_r6(rel, lineno, code, line_is_test, violations)
        check_r4(rel, lineno, code, line_is_test, violations)
        check_r5(rel, lineno, code, line_is_test, violations)

if violations:
    reason = "Context architecture guard FAILED:\n" + "\n".join(violations[:100])
    if len(violations) > 100:
        reason += f"\n... and {len(violations) - 100} more"
    print(json.dumps({"decision": "block", "reason": reason}, ensure_ascii=False))
    sys.exit(2)

print("Context architecture guard OK.")
PY
