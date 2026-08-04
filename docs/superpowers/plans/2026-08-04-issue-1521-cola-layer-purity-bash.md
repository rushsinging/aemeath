# Stop Hook 优化：cola-layer-purity 移植为 bash 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 `check-cola-layer-purity.sh` 从 `cargo run -p xtask -- cola-layer-purity`（Stop Hook 瓶颈，45~80s）移植为纯 bash 实现，并退役 xtask 中相关死代码，使 Stop Hook fast 集合总耗时从 46.8s 降至 <10s。

**Architecture:** 保持守卫注册、profile（fast）、编排位置、guard-registry 标记、失败输出 JSON 格式完全不变，仅替换实现载体。bash 版以函数化结构承载原 885 行 Rust 的常量矩阵 + 正则检查 + 目录分层规则，并内建与 xtask `run_sanity()` 等价的 23 项自检。等价性通过"双实现对照"验证：clean 仓库双跑均通过、违规样本库（取自 xtask 单测输入）双实现均捕获且消息一致。

**Tech Stack:** bash 3.2（macOS 系统 bash）、perl（PCRE 正则；所有含 `\b` 的模式 MUST 用 perl，因为 macOS BSD grep 不支持 `\b`）、find/grep/sed 基础工具、python3（仅用于失败时 JSON 转义）。

**关联 Issue:** #1521（milestone v0.1.0）

---

## 背景事实（已实测）

| 项目 | 数值 |
|---|---|
| Stop Hook fast 总耗时（基线） | 46.8s（并发 49 项） |
| `check-cola-layer-purity.sh` 独占 | 45.05s / 65.27s / 79.19s / warm cargo run 40.92s |
| 其余 48 项 fast 守卫 | 全部 < 6s，绝大多数 < 1s |
| xtask lib 单测 | 7 passed（含 cola_layer_purity 4 个） |

关键事实：
- `cola_layer_purity.rs`（885 行）全部为常量矩阵 + `regex` 正则 + `read_dir` 目录遍历 + 手工 brace 计数，**无 syn 语法树、无编译期依赖**。
- `regex` crate 仅被 `cola_layer_purity.rs` 使用（退役后可从 xtask Cargo.toml 移除）。
- xtask `src_dirs` 变量收集后从未被读取（死变量，bash 版不需要）。
- 文档 `01-architecture-guards.md` §5 白名单表登记 `tools/src/business/mcp_manager/connection.rs → core` 例外，但该目录已不存在、xtask 代码无此例外 → 文档 stale，以脚本为准（docs 维护说明），随本计划清理。
- 无调用方的 xtask 子命令：`run-test`（flaky.rs）、`changed-lines`（changed_lines.rs）。`flaky.rs` 的 `run_with_retry` 仅供 run-test 使用。

---

### Task 1: 捕获 xtask 基线行为（对照基准）

**Files:** 无修改；仅记录。

- [ ] **Step 1: 记录 xtask 版在 clean 仓库的输出与退出码**

Run:
```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath
bash .agents/hooks/check-cola-layer-purity.sh
echo "exit=$?"
```
Expected: exit=0，无违规输出（当前仓库 clean）。

- [ ] **Step 2: 确认 xtask 单测全部通过**

Run: `cargo test -p xtask --lib`
Expected: `test result: ok. 7 passed; 0 failed`（含 `cola_layer_purity::tests::sanity_checks_pass`）。

- [ ] **Step 3: 记录耗时基线**

Run:
```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath
TIMEFORMAT='%R'; time bash .agents/hooks/check-cola-layer-purity.sh >/dev/null 2>&1
TIMEFORMAT='%R'; time bash .agents/hooks/check-architecture-guards.sh --fast >/dev/null 2>&1
```
Expected: 单项 40~80s；fast 总耗时 ~46.8s（记录实际值，供 Task 6 对比）。

---

### Task 2: 编写 bash 版守卫（核心移植）

**Files:**
- Rewrite: `.agents/hooks/check-cola-layer-purity.sh`（原 19 行转发壳 → 完整实现）

- [ ] **Step 1: 写入完整 bash 实现**

用 Write 整体覆盖 `.agents/hooks/check-cola-layer-purity.sh`，内容为下面完整代码。移植规则：
- 常量矩阵、正则模式、违规消息文本与 Rust 版**逐字一致**；
- 所有含 `\b` / `(?:)` / 非贪婪的正则 **MUST 用 perl**（macOS BSD grep 不支持）；
- 失败输出 **MUST** 为 stdout JSON `{"decision":"block","reason":...}`（hook 输出分类依赖，见 #1335）；
- 保留两行 `guard-registry:` 标记；
- 保留 `run_sanity()` 自检（23 项断言，与 xtask `run_sanity` 输入一致），在 `check()` 开头执行。

```bash
#!/bin/bash
# COLA 分层纯度守卫：检查未迁移 feature 的 COLA 分层，并锁定已迁移 feature 的目标目录。
# 从 xtask cola-layer-purity 移植为纯 bash（issue 1521）：原 cargo run -p xtask 实现
# 在 Stop Hook 每次触发时产生 40~80s 编译/运行成本，纯 bash 版消除 fast 集合 Cargo 依赖。
# 语义与原实现一致：Runtime 使用 domain/application/ports/adapters/shared；
# Workflow 使用 domain；Storage 使用 domain/ports/adapters；
# Project/Tools/Task 使用 domain/adapters（domain 不得依赖 adapters）；
# Audit 仅允许随真实 Usage 交付增量建立的 Hexagonal 层。
# 例外：RUNTIME_LAYER_MIGRATION_EXCEPTIONS 为空集合（当前无迁移期层级倒置）。
# guard-registry:policy.hexagonal.current-layer-matrix
# guard-registry:policy.task.target-layout
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fi

violations=()

# ---------- 常量矩阵（与 xtask cola_layer_purity.rs 一致） ----------
FEATURE_LAYERS=(contract gateway core business utils)
RUNTIME_HEX_LAYERS=(domain application ports adapters shared)
WORKFLOW_HEX_LAYERS=(domain)
PROVIDER_HEX_LAYERS=(domain adapters)
MEMORY_HEX_LAYERS=(domain application ports adapters)
PROVIDER_LEGACY_LAYERS=(api business contract core gateway)
POLICY_HEX_LAYERS=(domain adapters)
POLICY_ALLOWED_TOP_LEVEL_FILES=(lib.rs domain.rs adapters.rs)
POLICY_LEGACY_LAYERS=(api business contract core gateway capabilities)
STORAGE_HEX_LAYERS=(domain ports adapters)
STORAGE_LEGACY_LAYERS=(api business contract gateway memory_store task_store)
PROJECT_HEX_LAYERS=(domain adapters)
PROJECT_ALLOWED_TOP_LEVEL_FILES=(lib.rs domain.rs adapters.rs)
PROJECT_LEGACY_LAYERS=(api business contract core gateway capabilities)
TOOLS_HEX_LAYERS=(domain adapters)
TOOLS_ALLOWED_TOP_LEVEL_FILES=(lib.rs domain.rs adapters.rs)
TOOLS_LEGACY_LAYERS=(api business contract core gateway)
TASK_HEX_LAYERS=(domain adapters)
TASK_ALLOWED_TOP_LEVEL_FILES=(lib.rs domain.rs adapters.rs)
TASK_LEGACY_LAYERS=(api business contract core gateway ports capabilities)
AUDIT_HEX_LAYERS=(domain application ports adapters)
AUDIT_ALLOWED_TOP_LEVEL_FILES=(lib.rs domain.rs application.rs ports.rs adapters.rs)
AUDIT_LEGACY_LAYERS=(api business contract core gateway capabilities)
HOOK_HEX_LAYERS=(domain ports adapters)
HOOK_ALLOWED_TOP_LEVEL_FILES=(lib.rs domain.rs ports.rs adapters.rs capabilities.rs)
HOOK_LEGACY_LAYERS=(api business contract core gateway capabilities)
CONTEXT_HEX_LAYERS=(domain application ports adapters)
TOOL_PROFILE_PUBLIC_API=(baseline derive_restricted allowed_capabilities)
POLICY_FORBIDDEN_ADAPTER_TYPES='\b(?:struct|enum)\s+(?:Deny|Approval|RequireApproval)\w*Policy\b'

# ---------- 基础工具 ----------

contains() { # contains <item> <array...>：数组包含判断
  local item="$1"
  shift
  local candidate
  for candidate in "$@"; do
    [ "$candidate" = "$item" ] && return 0
  done
  return 1
}

# forbidden_layer_deps <current_layer>：输出当前层禁止依赖的层（空格分隔）
forbidden_layer_deps() {
  case "$1" in
    business)   echo "core gateway contract" ;;
    utils)      echo "business core gateway contract" ;;
    contract)   echo "business core gateway utils" ;;
    gateway)    echo "business utils" ;;
    domain)     echo "application ports adapters" ;;
    ports)      echo "application adapters" ;;
    application) echo "adapters" ;;
    shared)     echo "domain application ports adapters" ;;
    *)          echo "" ;;
  esac
}

# strip_rust_comments：stdin -> stdout（去块注释与行注释；与 Rust regex 一致：. 不跨行）
strip_rust_comments() {
  perl -pe 's{/\*.*?\*/}{}g; s{//.*$}{}g'
}

# named_block <header_regex>：从 stdin 读取源码，输出 header 匹配后首个 { ... } 块体（不含外层花括号）
named_block() {
  local header="$1"
  perl -0777 -e '
    my $header = shift @ARGV;
    local $/;
    my $source = <STDIN>;
    sub extract_block {
      my ($src, $hdr) = @_;
      return "" unless $src =~ /$hdr/s;
      my $start = $+[0];
      my $opening = index($src, "{", $start);
      return "" if $opening < 0;
      my $depth = 0;
      for (my $i = $opening; $i < length($src); $i++) {
        my $ch = substr($src, $i, 1);
        $depth++ if $ch eq "{";
        $depth-- if $ch eq "}";
        return substr($src, $opening + 1, $i - $opening - 1) if $depth == 0;
      }
      return "";
    }
    print extract_block($source, $header);
  ' "$header"
}

# ---------- 违规检查函数（与 xtask 同名函数等价） ----------

# line_layer_violations <current_layer> <line>：输出违规消息（每行一条）
line_layer_violations() {
  local current_layer="$1" line="$2"
  local stripped
  stripped="$(printf '%s' "$line" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"
  if [ -z "$stripped" ] || [[ "$stripped" == //* || "$stripped" == '*'* ]]; then
    return 0
  fi
  local forbiddens target
  forbiddens="$(forbidden_layer_deps "$current_layer")"
  [ -z "$forbiddens" ] && return 0
  while IFS= read -r target; do
    [ -z "$target" ] && continue
    if contains "$target" $forbiddens; then
      printf 'feature layer %s must not depend on crate::%s\n' "$current_layer" "$target"
    fi
  done < <(printf '%s' "$line" | perl -ne 'while (/\b(?:use\s+)?crate::([A-Za-z_][A-Za-z0-9_]*)/g) { print "$1\n" }')
}

# tool_profile_violations <source>：输出违规消息
tool_profile_violations() {
  local source="$1"
  local stripped body count has_pub
  stripped="$(printf '%s' "$source" | strip_rust_comments)"
  body="$(printf '%s' "$stripped" | named_block '\bpub\s+struct\s+ToolProfile\b')"
  if [ -n "$body" ]; then
    has_pub="$(printf '%s' "$body" | perl -ne 'while (/(?:^|,)\s*(pub(?:\([^)]*\))?\s+)?allowed_capabilities\s*:/g) { print "$&\n" }' | grep -c 'pub' || true)"
    count="$(printf '%s' "$body" | perl -ne 'while (/(?:^|,)\s*(pub(?:\([^)]*\))?\s+)?allowed_capabilities\s*:/g) { print "$&\n" }' | grep -c . || true)"
    if [ "$count" -ne 1 ] || [ "$has_pub" -ne 0 ]; then
      echo "ToolProfile.allowed_capabilities must remain a private field"
    fi
  fi
  body="$(printf '%s' "$stripped" | named_block '\bimpl\s+ToolProfile\b')"
  if [ -n "$body" ]; then
    local method expansion=()
    while IFS= read -r method; do
      [ -z "$method" ] && continue
      contains "$method" "${TOOL_PROFILE_PUBLIC_API[@]}" || expansion+=("$method")
    done < <(printf '%s' "$body" | perl -ne 'while (/\bpub\s+fn\s+([A-Za-z_][A-Za-z0-9_]*)/g) { print "$1\n" }')
    if [ "${#expansion[@]}" -gt 0 ]; then
      local names
      names="$(printf '%s\n' "${expansion[@]}" | sort | perl -e 'chomp(my @l = <STDIN>); print join(", ", @l)')"
      echo "ToolProfile must not expose capability-expanding mutation API: $names"
    fi
    if printf '%s' "$body" | perl -ne 'exit 1 unless /\bfn\s+\w+\s*\([^)]*&mut\s+self/'; then
      echo "ToolProfile must not expose in-place mutation"
    fi
    if printf '%s' "$body" | perl -ne 'exit 1 unless /self\.allowed_capabilities\s*(?:\|=|&=|\^=|=)|self\.allowed_capabilities\s*\.\s*(?:insert|extend|union)/'; then
      echo "ToolProfile.allowed_capabilities must not be mutated"
    fi
  fi
}

# tools_authorization_violations <source>：输出违规消息
tools_authorization_violations() {
  local source="$1"
  local stripped name_based
  stripped="$(printf '%s' "$source" | strip_rust_comments)"
  if printf '%s' "$stripped" | perl -ne 'exit 1 unless /\b(?:ToolProfile|is_authorized|authoriz\w*|exclud\w*|denylist|blacklist)\b/'
     && printf '%s' "$stripped" | perl -ne 'exit 1 unless /\bexcludes\b/'; then
    echo "ToolProfile::excludes/name blacklist authorization is forbidden"
  fi
  name_based="$(printf '%s' "$stripped" | perl -0777 -ne 'if (/\b(?:exclud\w*|denylist|blacklist)\b[\s\S]{0,500}/) { print "$&\n" }')"
  if [ -n "$name_based" ]; then
    if printf '%s' "$name_based" | perl -0777 -ne 'exit 1 unless /(?:\bmatch\s+[^{}]*?(?:\btool_?name\b|\.name\b)|\bmatches!\s*\([^,]*?(?:\bToolName\b|\btool_?name\b|\.name\b))/'; then
      echo "authorization must not match on ToolName; use declared capabilities"
    fi
  fi
}

# tools_boundary_violations <rel_s> <source>：输出违规消息
tools_boundary_violations() {
  local rel_s="$1" source="$2"
  local stripped
  stripped="$(printf '%s' "$source" | strip_rust_comments)"
  if [ "$rel_s" = "agent/features/tools/src/lib.rs" ]; then
    if printf '%s' "$stripped" | perl -ne 'exit 1 unless /\bpub\s+(?:use\b[^;]*\b|(?:struct|enum|type)\s+)(?:RegistryScopeBuilder|RegistryScope)\b/'; then
      echo "RegistryScopeBuilder/RegistryScope must not enter the tools crate-root facade"
    fi
  fi
  if [[ "$rel_s" == agent/features/tools/src/domain* ]]; then
    if printf '%s' "$stripped" | perl -ne 'exit 1 unless /\bToolRegistry\b/'; then
      echo "ToolRegistry is an adapter and must not enter tools domain"
    fi
  fi
}

# is_test_path <path>：测试路径判断
is_test_path() {
  local path="$1" name stem
  name="$(basename "$path")"
  stem="${name%.rs}"
  case "$name" in
    *_test.rs|*_tests.rs) return 0 ;;
  esac
  [ "$stem" = "tests" ] && return 0
  case "$path" in
    */tests/*) return 0 ;;
  esac
  return 1
}

# feature_layer_for <path>：输出 "feature layer"（空格分隔）；无归属返回 1
feature_layer_for() {
  local path="$1"
  local rel="${path#"$ROOT"/}"
  [[ "$rel" != agent/features/* ]] && return 1
  local rest="${rel#agent/features/}"
  local feature="${rest%%/*}"
  local rest2="${rest#*/}"
  [[ "$rest2" != src/* ]] && return 1
  local layer="${rest2#src/}"
  layer="${layer%%/*}"
  layer="${layer%.rs}"
  case "$feature" in
    runtime)  contains "$layer" "${RUNTIME_HEX_LAYERS[@]}" && { echo "$feature $layer"; return 0; } ;;
    workflow) contains "$layer" "${WORKFLOW_HEX_LAYERS[@]}" && { echo "$feature $layer"; return 0; } ;;
    provider) contains "$layer" "${PROVIDER_HEX_LAYERS[@]}" && { echo "$feature $layer"; return 0; } ;;
    memory)   contains "$layer" "${MEMORY_HEX_LAYERS[@]}" && { echo "$feature $layer"; return 0; } ;;
    context)  contains "$layer" "${CONTEXT_HEX_LAYERS[@]}" && { echo "$feature $layer"; return 0; } ;;
    policy)   contains "$layer" "${POLICY_HEX_LAYERS[@]}" && { echo "$feature $layer"; return 0; } ;;
    project)  contains "$layer" "${PROJECT_HEX_LAYERS[@]}" && { echo "$feature $layer"; return 0; } ;;
    tools)    contains "$layer" "${TOOLS_HEX_LAYERS[@]}" && { echo "$feature $layer"; return 0; } ;;
    task)     contains "$layer" "${TASK_HEX_LAYERS[@]}" && { echo "$feature $layer"; return 0; } ;;
    audit)    contains "$layer" "${AUDIT_HEX_LAYERS[@]}" && { echo "$feature $layer"; return 0; } ;;
    hook)     contains "$layer" "${HOOK_HEX_LAYERS[@]}" && { echo "$feature $layer"; return 0; } ;;
    storage)  return 1 ;;
  esac
  contains "$layer" "${FEATURE_LAYERS[@]}" && { echo "$feature $layer"; return 0; }
  return 1
}

# ---------- 目录分层检查（与 xtask check() 的 src_dirs 循环等价；src_dirs 本身是死变量，不移植） ----------
check_src_layout() {
  local features_root="$ROOT/agent/features"
  [ -d "$features_root" ] || return 0
  local crate_dir crate_name
  while IFS= read -r crate_dir; do
    [ -n "$crate_dir" ] || continue
    [ -d "$crate_dir/src" ] || continue
    crate_name="$(basename "$crate_dir")"
    local src="$crate_dir/src" child name rel
    local entries
    entries="$(find "$src" -mindepth 1 -maxdepth 1 ! -name '.*' | sort)"
    while IFS= read -r child; do
      [ -n "$child" ] || continue
      name="$(basename "$child")"
      rel="${child#"$ROOT"/}"
      case "$crate_name" in
        runtime)
          if [ -d "$child" ] && contains "$name" "${FEATURE_LAYERS[@]}"; then
            violations+=("$rel: Runtime legacy COLA directory is forbidden; use domain, application, ports, adapters, shared")
            continue
          fi
          ;;
        workflow)
          if [ -d "$child" ]; then
            if ! contains "$name" "${WORKFLOW_HEX_LAYERS[@]}"; then
              violations+=("$rel: Workflow source directories must be domain")
            fi
          elif [ "$name" != "lib.rs" ] && [ "$name" != "domain.rs" ]; then
            violations+=("$rel: Workflow top-level source files must be lib.rs or domain.rs")
          fi
          continue
          ;;
        provider)
          if contains "$name" "${PROVIDER_LEGACY_LAYERS[@]}"; then
            violations+=("$rel: Provider legacy fixed layer is forbidden; use domain/ports/adapters")
            continue
          fi
          if [ -d "$child" ] && ! contains "$name" "${PROVIDER_HEX_LAYERS[@]}"; then
            violations+=("$rel: Provider source directories must be domain, adapters")
          fi
          continue
          ;;
        policy)
          if contains "$name" "${POLICY_LEGACY_LAYERS[@]}"; then
            violations+=("$rel: Policy legacy fixed layer is forbidden; use domain, adapters")
          elif [ -d "$child" ] && ! contains "$name" "${POLICY_HEX_LAYERS[@]}"; then
            violations+=("$rel: Policy source directories must be domain, adapters")
          elif [ ! -d "$child" ] && ! contains "$name" "${POLICY_ALLOWED_TOP_LEVEL_FILES[@]}"; then
            violations+=("$rel: Policy top-level source files must be lib.rs, domain.rs, adapters.rs")
          fi
          continue
          ;;
        project)
          if contains "$name" "${PROJECT_LEGACY_LAYERS[@]}"; then
            violations+=("$rel: Project legacy fixed layer is forbidden; use PROJECT_HEX_LAYERS")
          elif [ -d "$child" ] && ! contains "$name" "${PROJECT_HEX_LAYERS[@]}"; then
            violations+=("$rel: Project source directories must be domain, adapters")
          elif [ ! -d "$child" ] && ! contains "$name" "${PROJECT_ALLOWED_TOP_LEVEL_FILES[@]}"; then
            violations+=("$rel: Project top-level source files must be lib.rs, domain.rs, adapters.rs")
          fi
          continue
          ;;
        audit)
          if contains "$name" "${AUDIT_LEGACY_LAYERS[@]}"; then
            violations+=("$rel: Audit empty or legacy fixed layer is forbidden; use evidence-backed domain, application, ports, adapters")
          elif [ -d "$child" ] && ! contains "$name" "${AUDIT_HEX_LAYERS[@]}"; then
            violations+=("$rel: Audit source directories must be evidence-backed layers domain, application, ports, adapters")
          elif [ ! -d "$child" ] && ! contains "$name" "${AUDIT_ALLOWED_TOP_LEVEL_FILES[@]}"; then
            violations+=("$rel: Audit top-level source files must be lib.rs, domain.rs, application.rs, ports.rs, adapters.rs")
          fi
          continue
          ;;
        hook)
          if contains "$name" "${HOOK_LEGACY_LAYERS[@]}"; then
            violations+=("$rel: Hook legacy fixed layer is forbidden; use domain, ports, adapters")
          elif [ -d "$child" ] && ! contains "$name" "${HOOK_HEX_LAYERS[@]}"; then
            violations+=("$rel: Hook source directories must be domain, ports, adapters")
          elif [ ! -d "$child" ] && ! contains "$name" "${HOOK_ALLOWED_TOP_LEVEL_FILES[@]}"; then
            violations+=("$rel: Hook top-level source files must be lib.rs, domain.rs, ports.rs, adapters.rs, capabilities.rs")
          fi
          continue
          ;;
        tools)
          if contains "$name" "${TOOLS_LEGACY_LAYERS[@]}"; then
            violations+=("$rel: tools legacy fixed layer is forbidden; use domain, adapters")
          elif [ -d "$child" ] && ! contains "$name" "${TOOLS_HEX_LAYERS[@]}"; then
            violations+=("$rel: tools source directories must be domain, adapters")
          elif [ ! -d "$child" ] && ! contains "$name" "${TOOLS_ALLOWED_TOP_LEVEL_FILES[@]}"; then
            violations+=("$rel: tools top-level source files must be lib.rs, domain.rs, adapters.rs")
          fi
          continue
          ;;
        task)
          if contains "$name" "${TASK_LEGACY_LAYERS[@]}"; then
            violations+=("$rel: Task legacy fixed layer is forbidden; use domain, adapters")
          elif [ -d "$child" ] && ! contains "$name" "${TASK_HEX_LAYERS[@]}"; then
            violations+=("$rel: Task source directories must be domain, adapters")
          elif [ ! -d "$child" ] && ! contains "$name" "${TASK_ALLOWED_TOP_LEVEL_FILES[@]}"; then
            violations+=("$rel: Task top-level source files must be lib.rs, domain.rs, adapters.rs")
          fi
          continue
          ;;
        storage)
          if contains "$name" "${STORAGE_LEGACY_LAYERS[@]}"; then
            violations+=("$rel: Storage legacy fixed layer is forbidden; use domain, ports, adapters")
          elif [ -d "$child" ] && ! contains "$name" "${STORAGE_HEX_LAYERS[@]}"; then
            violations+=("$rel: Storage directory must be a hexagonal layer domain, ports, adapters or registered transitional module")
          fi
          continue
          ;;
        memory)
          if [ -d "$child" ] && contains "$name" "${MEMORY_HEX_LAYERS[@]}"; then
            continue
          fi
          ;;
      esac
      if [ -d "$child" ] && ! contains "$name" "${FEATURE_LAYERS[@]}"; then
        if [ "$crate_name" = "runtime" ] && contains "$name" "${RUNTIME_HEX_LAYERS[@]}"; then
          continue
        fi
        if [ "$crate_name" = "context" ] && contains "$name" "${CONTEXT_HEX_LAYERS[@]}"; then
          continue
        fi
        violations+=("$rel: feature src directories must be COLA layers contract, gateway, core, business, utils")
      fi
    done < <(printf '%s\n' "$entries")
  done < <(find "$features_root" -mindepth 1 -maxdepth 1 -type d | sort)
}

# ---------- 文件级检查 ----------
check_files() {
  local features_root="$ROOT/agent/features"
  [ -d "$features_root" ] || return 0
  local path rel_s source feature_layer feature layer violation
  while IFS= read -r path; do
    [ -n "$path" ] || continue
    is_test_path "$path" && continue
    rel_s="${path#"$ROOT"/}"
    source="$(cat "$path")"
    if [[ "$rel_s" == agent/features/tools/src/* ]]; then
      while IFS= read -r violation; do
        [ -n "$violation" ] && violations+=("$rel_s: $violation")
      done < <(tools_authorization_violations "$source")
      while IFS= read -r violation; do
        [ -n "$violation" ] && violations+=("$rel_s: $violation")
      done < <(tools_boundary_violations "$rel_s" "$source")
      if printf '%s' "$source" | strip_rust_comments | perl -ne 'exit 1 unless /\bpub\s+struct\s+ToolProfile\b/'; then
        while IFS= read -r violation; do
          [ -n "$violation" ] && violations+=("$rel_s: $violation")
        done < <(tool_profile_violations "$source")
      fi
    fi
    if { [[ "$rel_s" == agent/features/storage/src/domain/* ]] || [ "$rel_s" = "agent/features/storage/src/domain.rs" ]; } \
       && printf '%s' "$source" | perl -ne 'exit 1 unless /\b(?:std|tokio)::fs::|\bPathBuf\b|\bcrate::adapters\b/'; then
      violations+=("$rel_s: Storage domain must not perform physical I/O, own PathBuf, or depend on adapters")
    fi
    feature_layer="$(feature_layer_for "$path")" || continue
    feature="${feature_layer%% *}"
    layer="${feature_layer#* }"
    if [ "$layer" = "domain" ] \
       && [[ "$rel_s" == agent/features/project/src/* ]] \
       && printf '%s' "$source" | perl -0777 -ne 'exit 1 unless /\bcrate\s*::\s*(?:adapters\b|\{[^}]*\badapters\s*::)/'; then
      violations+=("$rel_s: Project domain must not depend on crate::adapters")
    fi
    local lineno=0 line
    while IFS= read -r line; do
      lineno=$((lineno + 1))
      while IFS= read -r violation; do
        [ -n "$violation" ] && violations+=("$rel_s:$lineno: $violation: $(printf '%s' "$line" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')")
      done < <(line_layer_violations "$layer" "$line")
    done <<< "$source"
  done < <(find "$features_root" -name '*.rs' -type f | sort)
}

# ---------- 自检（与 xtask run_sanity() 的 23 项断言等价） ----------
run_sanity() {
  local fail=0
  # line_layer_violations 断言
  line_layer_violations "business" "use crate::core::port::ToolPort;" | grep -q . || { echo "sanity: business->core 应被阻断"; fail=1; }
  line_layer_violations "utils" "let _ = crate::business::Policy::default();" | grep -q . || { echo "sanity: utils->business 应被阻断"; fail=1; }
  line_layer_violations "core" "use crate::business::TaskState;" | grep -q . && { echo "sanity: core->business 应放行"; fail=1; }
  line_layer_violations "domain" "use crate::application::Agent;" | grep -q . || { echo "sanity: domain->application 应被阻断"; fail=1; }
  line_layer_violations "application" "use crate::adapters::SdkProjection;" | grep -q . || { echo "sanity: application->adapters 应被阻断"; fail=1; }
  line_layer_violations "application" "use crate::domain::Run;" | grep -q . && { echo "sanity: application->domain 应放行"; fail=1; }
  line_layer_violations "adapters" "use crate::ports::EventSink;" | grep -q . && { echo "sanity: adapters->ports 应放行"; fail=1; }
  line_layer_violations "business" "use crate::utils::normalize_path;" | grep -q . && { echo "sanity: business->utils 应放行"; fail=1; }
  line_layer_violations "domain" "use crate::adapters::git::GitCli;" | grep -q . || { echo "sanity: domain->adapters 应被阻断"; fail=1; }
  line_layer_violations "adapters" "use crate::domain::git::GitWorktreeOps;" | grep -q . && { echo "sanity: adapters->domain 应放行"; fail=1; }
  # project_domain_adapter_pattern 断言（多行）
  printf 'use crate::{\n adapters::git::GitCli,\n};' | perl -0777 -ne 'exit 1 unless /\bcrate\s*::\s*(?:adapters\b|\{[^}]*\badapters\s*::)/' || { echo "sanity: 多行 braced Project domain 依赖应被阻断"; fail=1; }
  printf 'use crate::{\n domain::types::WorkspaceRead,\n adapters::git::GitCli,\n};' | perl -0777 -ne 'exit 1 unless /\bcrate\s*::\s*(?:adapters\b|\{[^}]*\badapters\s*::)/' || { echo "sanity: 非首个 braced Project domain 依赖应被阻断"; fail=1; }
  printf 'use crate::\n adapters::git::GitCli;' | perl -0777 -ne 'exit 1 unless /\bcrate\s*::\s*(?:adapters\b|\{[^}]*\badapters\s*::)/' || { echo "sanity: 多行 Project domain 依赖应被阻断"; fail=1; }
  # tool_profile_violations 断言
  local safe_profile='pub struct ToolProfile { allowed_capabilities: ToolCapabilities }
impl ToolProfile {
    pub fn baseline(value: ToolCapabilities) -> Self { Self { allowed_capabilities: value } }
    pub fn derive_restricted(parent: &Self, requested: ToolCapabilities) -> Self {
        Self::baseline(requested & parent.allowed_capabilities)
    }
    pub fn allowed_capabilities(&self) -> ToolCapabilities { self.allowed_capabilities }
}'
  tool_profile_violations "$safe_profile" | grep -q . && { echo "sanity: 私有 shrink-only ToolProfile 应放行"; fail=1; }
  tool_profile_violations "pub struct ToolProfile { pub allowed_capabilities: ToolCapabilities }" | grep -q . || { echo "sanity: 公开 ToolProfile 字段应被阻断"; fail=1; }
  tool_profile_violations "${safe_profile/pub fn allowed_capabilities/pub fn insert(\&mut self, value: ToolCapabilities) { self.allowed_capabilities |= value; } pub fn allowed_capabilities}" | grep -q . || { echo "sanity: capability-expanding ToolProfile API 应被阻断"; fail=1; }
  # tools_authorization_violations 断言
  tools_authorization_violations "impl ToolProfile { fn excludes(&self, name: &ToolName) -> bool { matches!(name, ToolName::Bash) } }" | grep -q . || { echo "sanity: ToolProfile name blacklist 应被阻断"; fail=1; }
  tools_authorization_violations "fn is_authorized(required: Caps, profile: ToolProfile) -> bool { required.is_subset_of(profile.allowed_capabilities()) }" | grep -q . && { echo "sanity: capability authorization 应放行"; fail=1; }
  # tools_boundary_violations 断言
  tools_boundary_violations "agent/features/tools/src/lib.rs" "pub use domain::RegistryScopeBuilder;" | grep -q . || { echo "sanity: RegistryScopeBuilder 进 crate-root facade 应被阻断"; fail=1; }
  tools_boundary_violations "agent/features/tools/src/domain/catalog.rs" "use crate::adapters::ToolRegistry;" | grep -q . || { echo "sanity: ToolRegistry 进 domain 应被阻断"; fail=1; }
  tools_boundary_violations "agent/features/tools/src/adapters/catalog.rs" "use super::ToolRegistry;" | grep -q . && { echo "sanity: ToolRegistry 在 adapters 应放行"; fail=1; }
  if [ "$fail" -ne 0 ]; then
    echo "check-cola-layer-purity: sanity 自检失败" >&2
    exit 1
  fi
}

# ---------- 主流程（与 xtask check() 等价） ----------
check() {
  run_sanity

  # policy/src/adapters.rs 生产 adapter 必须 AllowAll-only
  local policy_adapter="$ROOT/agent/features/policy/src/adapters.rs"
  if [ -f "$policy_adapter" ]; then
    if cat "$policy_adapter" | strip_rust_comments | perl -ne "exit 1 unless /$POLICY_FORBIDDEN_ADAPTER_TYPES/"; then
      violations+=("agent/features/policy/src/adapters.rs: v0.1.0 production adapter must be AllowAll-only")
    fi
  fi

  # 旧目录必须不存在
  local old_path
  for old_path in agent/runtime agent/provider agent/tools; do
    if [ -e "$ROOT/$old_path" ]; then
      violations+=("$old_path: runtime/provider/tools must live under agent/features/*")
    fi
  done

  check_src_layout
  check_files

  if [ "${#violations[@]}" -gt 0 ]; then
    local reason
    reason="$(printf 'COLA layer purity guard FAILED:\n%s' "$(printf '%s\n' "${violations[@]}")")"
    printf '%s' "$reason" | python3 -c 'import json,sys; print(json.dumps({"decision":"block","reason":sys.stdin.read()}))'
    echo "$reason" >&2
    exit 1
  fi
}

check
```

- [ ] **Step 2: 语法检查与自检**

Run:
```bash
bash -n <worktree>/.agents/hooks/check-cola-layer-purity.sh
bash <worktree>/.agents/hooks/check-cola-layer-purity.sh
echo "exit=$?"
```
Expected: `bash -n` 无输出；完整运行 exit=0（clean 仓库，自检 + 全部检查通过）。

- [ ] **Step 3: 提交**

```bash
git add .agents/hooks/check-cola-layer-purity.sh
git commit -m "feat(hook): #1521 cola-layer-purity 守卫移植为纯 bash，消除 Stop Hook fast 集合 Cargo 依赖"
```
（如 Task 3 未完成则先不提交，保持 working tree 便于对比调试。）

---

### Task 3: 等价性验证（bash vs xtask 双实现对照）

**Files:** 无仓库修改；使用 `/tmp/cpl-verify/` 构造对照样本。

- [ ] **Step 1: clean 仓库双跑对照**

Run（`<worktree>` 替换为实际 worktree 路径）:
```bash
cd <worktree>
bash .agents/hooks/check-cola-layer-purity.sh > /tmp/cpl-bash.out 2>&1; echo "bash exit=$?" >> /tmp/cpl-bash.out
cargo run --quiet -p xtask -- cola-layer-purity "$PWD" > /tmp/cpl-xtask.out 2>&1; echo "xtask exit=$?" >> /tmp/cpl-xtask.out
diff /tmp/cpl-bash.out /tmp/cpl-xtask.out && echo "CLEAN REPO: 一致"
```
Expected: 两个实现输出一致（clean 均无违规、exit 0）。

- [ ] **Step 2: 构造违规样本仓库（/tmp/cpl-verify/）**

在 `/tmp/cpl-verify/<样本>/agent/features/` 下按样本创建最小目录结构：

```bash
# 样本 A：runtime domain 依赖 application（line_layer_violations）
mkdir -p /tmp/cpl-verify/A/agent/features/runtime/src/domain
printf 'use crate::application::Agent;\n' > /tmp/cpl-verify/A/agent/features/runtime/src/domain/run.rs

# 样本 B：tools domain 出现 ToolRegistry（tools_boundary_violations）
mkdir -p /tmp/cpl-verify/B/agent/features/tools/src/domain
printf 'use crate::adapters::ToolRegistry;\n' > /tmp/cpl-verify/B/agent/features/tools/src/domain/catalog.rs

# 样本 C：tools lib.rs 暴露 RegistryScopeBuilder（tools_boundary_violations）
mkdir -p /tmp/cpl-verify/C/agent/features/tools/src
printf 'pub use domain::RegistryScopeBuilder;\n' > /tmp/cpl-verify/C/agent/features/tools/src/lib.rs

# 样本 D：ToolProfile 公开 allowed_capabilities 字段（tool_profile_violations）
mkdir -p /tmp/cpl-verify/D/agent/features/tools/src/domain
printf 'pub struct ToolProfile { pub allowed_capabilities: ToolCapabilities }\n' > /tmp/cpl-verify/D/agent/features/tools/src/domain/profile.rs

# 样本 E：ToolProfile excludes 黑名单（tools_authorization_violations）
mkdir -p /tmp/cpl-verify/E/agent/features/tools/src/domain
printf 'impl ToolProfile { fn excludes(&self, name: &ToolName) -> bool { matches!(name, ToolName::Bash) } }\n' > /tmp/cpl-verify/E/agent/features/tools/src/domain/profile.rs

# 样本 F：Project domain 依赖 adapters（project_domain_adapter_pattern）
mkdir -p /tmp/cpl-verify/F/agent/features/project/src/domain
printf 'use crate::\n adapters::git::GitCli;\n' > /tmp/cpl-verify/F/agent/features/project/src/domain/workspace.rs

# 样本 G：Storage domain 使用 std::fs（storage_domain_io_pattern）
mkdir -p /tmp/cpl-verify/G/agent/features/storage/src/domain
printf 'use std::fs;\n' > /tmp/cpl-verify/G/agent/features/storage/src/domain/store.rs

# 样本 H：Task 顶层出现 business.rs（目录分层）
mkdir -p /tmp/cpl-verify/H/agent/features/task/src
printf '// legacy\n' > /tmp/cpl-verify/H/agent/features/task/src/business.rs

# 样本 I：Runtime 顶层出现 core/（目录分层 legacy）
mkdir -p /tmp/cpl-verify/I/agent/features/runtime/src/core
printf '// legacy\n' > /tmp/cpl-verify/I/agent/features/runtime/src/core/mod.rs
```

- [ ] **Step 3: 对每个样本执行双实现对照**

```bash
cd /tmp/cpl-verify
for sample in A B C D E F G H I; do
  AEMEATH_PROJECT_DIR=/tmp/cpl-verify/$sample bash <worktree>/.agents/hooks/check-cola-layer-purity.sh > /tmp/cpl-bash.out 2>&1; echo "bash exit=$?" >> /tmp/cpl-bash.out
  (cd /tmp/cpl-verify/$sample && cargo run --quiet -p xtask -- cola-layer-purity "$PWD") > /tmp/cpl-xtask.out 2>&1; echo "xtask exit=$?" >> /tmp/cpl-xtask.out
  if diff /tmp/cpl-bash.out /tmp/cpl-xtask.out >/dev/null; then
    echo "样本 $sample: 一致"
  else
    echo "样本 $sample: 不一致"; diff /tmp/cpl-bash.out /tmp/cpl-xtask.out | head -20
  fi
done
```
Expected: 每个样本 bash 版与 xtask 版输出等价（违规消息逐字一致，退出码一致）。

- [ ] **Step 4: 修正差异（如有）**

若某样本行为不一致：先对比 xtask 单测输入与 bash 实现，定位翻译差异（正则转义、perl 标志、路径处理），修改 `.agents/hooks/check-cola-layer-purity.sh` 后重跑 Step 3。**MUST** 以 xtask 行为为准。

- [ ] **Step 5: 提交**

```bash
git add .agents/hooks/check-cola-layer-purity.sh
git commit -m "feat(hook): #1521 cola-layer-purity 移植为纯 bash（等价性对照通过）"
```

---

### Task 4: 退役 xtask cola-layer-purity 与死代码清理

**Files:**
- Delete: `tools/xtask/src/cola_layer_purity.rs`
- Delete: `tools/xtask/src/flaky.rs`
- Delete: `tools/xtask/src/changed_lines.rs`
- Modify: `tools/xtask/src/main.rs`（删除 changed-lines / run-test / cola-layer-purity 三个分支）
- Modify: `tools/xtask/src/lib.rs`（删除三个 mod 声明）
- Modify: `tools/xtask/Cargo.toml`（删除 `regex` 依赖；确认无其他引用后）

- [ ] **Step 1: 删除三个源文件**

```bash
cd <worktree>
git rm tools/xtask/src/cola_layer_purity.rs tools/xtask/src/flaky.rs tools/xtask/src/changed_lines.rs
```

- [ ] **Step 2: 更新 main.rs**

删除三个分支（第 10-19 行 `changed-lines`、第 51-60 行 `run-test`、第 106-109 行 `cola-layer-purity`），并更新 usage 字符串（第 111 行）为：
```
用法: cargo run -p xtask -- <coverage-summary <report.json> <root>|production-reachability [root]|guard-registry <check|report> [root] [output]|source-guard [root] [public-surface-output]|sdk-wire-schema <write|check> [output]>
```

- [ ] **Step 3: 更新 lib.rs**

删除三行 mod 声明：`pub mod changed_lines;` / `pub mod cola_layer_purity;` / `pub mod flaky;`

- [ ] **Step 4: 确认 regex 依赖可移除并更新 Cargo.toml**

```bash
grep -rn 'regex' tools/xtask/src/ --include='*.rs' | grep -v cola_layer_purity
```
Expected: 无输出（仅 cola_layer_purity 使用 regex）。然后从 `tools/xtask/Cargo.toml` 删除 `regex = "1"` 行。

- [ ] **Step 5: 编译与测试验证**

```bash
cd <worktree>
cargo check -p xtask
cargo test -p xtask --lib
cargo clippy -p xtask -- -D warnings
```
Expected: 全部通过（单测从 7 个降为 3 个：sdk_wire_schema_tests 2 个 + 无 cola 单测；确认剩余测试通过）。

- [ ] **Step 6: 提交**

```bash
git add -A tools/xtask/
git commit -m "refactor(xtask): #1521 退役 cola-layer-purity 子命令与无调用方的 run-test/changed-lines/flaky"
```

---

### Task 5: 文档同步

**Files:**
- Modify: `docs/design/03-engineering/01-architecture-guards.md`

- [ ] **Step 1: 更新 §5 实现载体说明**

在 `## 5. check-cola-layer-purity.sh`（第 205 行起）小节末尾追加：

```markdown
- **实现载体**：纯 bash（`.agents/hooks/check-cola-layer-purity.sh` 内联实现，含与语义等价的启动自检）。原 `cargo run -p xtask -- cola-layer-purity` 实现因 Stop Hook 每次触发的编译/运行成本（实测 40~80s，占 fast 总耗时 96%+）于 #1521 退役；xtask 不再提供 `cola-layer-purity` 子命令。
```

- [ ] **Step 2: 清理 stale 白名单表**

删除 §5 中"白名单（`LAYER_MIGRATION_EXCEPTIONS`）"表格（第 236-241 行）：`tools/src/business/mcp_manager/connection.rs → core` 例外对应的 `agent/features/tools/src/business/` 目录已不存在，xtask 实现亦无此例外（文档与脚本不一致时以脚本为准）。替换为：

```markdown
- **白名单（`LAYER_MIGRATION_EXCEPTIONS`）**：无。tools 已完成迁移（`agent/features/tools/src/business/` 已不存在），历史 business→core 例外记录已清理。
```

- [ ] **Step 3: 修改历史表追加一行**

在"修改历史"表（第 718 行起）顶部追加：

```markdown
| 2026-08-04 | #1521 cola-layer-purity 守卫从 xtask 移植为纯 bash（Stop Hook fast 瓶颈 40~80s → <1s）；退役 xtask `cola-layer-purity` / `run-test` / `changed-lines` 子命令与 `flaky.rs` / `changed_lines.rs`；清理 §5 stale 白名单表 | [#1521](https://github.com/rushsinging/aemeath/issues/1521) |
```

- [ ] **Step 4: 提交**

```bash
git add docs/design/03-engineering/01-architecture-guards.md
git commit -m "docs(guard): #1521 同步 cola-layer-purity 纯 bash 实现载体与 stale 白名单清理"
```

---

### Task 6: 全量门禁验证与计时对比

**Files:** 无修改。

- [ ] **Step 1: 单项与 fast 总耗时复测**

Run:
```bash
cd <worktree>
TIMEFORMAT='%R'; time bash .agents/hooks/check-cola-layer-purity.sh >/dev/null 2>&1
TIMEFORMAT='%R'; time bash .agents/hooks/check-architecture-guards.sh --fast >/dev/null 2>&1
```
Expected: 单项 < 1s（基线 40~80s）；fast 总耗时 < 10s（基线 46.8s）。记录实际值写入 PR Test plan。

- [ ] **Step 2: full 集合与 guard-registry 校验**

```bash
bash .agents/hooks/check-architecture-guards.sh --full
```
Expected: 全部通过（含 `check-guard-registry.sh` 对 guard-registry 标记的引用校验、`check-production-reachability.sh` 的 source-guard）。

- [ ] **Step 3: workspace 单元测试（guard 相关 crate 不受影响）**

```bash
cargo test --workspace --lib
```
Expected: 全部通过（本改动仅触及 .agents/hooks + tools/xtask，workspace 生产 crate 无变化）。

- [ ] **Step 4: pre-commit 链路抽查**

```bash
cd <worktree>
git add -A
bash .cargo/hooks/pre-commit
```
Expected: 通过（source-guard 仍可用；本改动文件均为 .agents/ 与 tools/xtask/ 前缀，触发 source guard 路径）。

- [ ] **Step 5: 更新 Issue #1521 checklist 并创建 PR**

```bash
gh issue view 1521 --repo rushsinging/aemeath
# 逐项勾选完成；PR 模板：Summary / Refs (#1521) / Breaking change（xtask 子命令移除）/ Test plan（含 Task 1/3/6 实测数据）
```

---

## Self-Review 记录

- **Issue 门禁覆盖**：#1521 的 7 项 AC 全部映射到任务：AC1（Task 2）、AC2（Task 3）、AC3（Task 4）、AC4（Task 4）、AC5（Task 5）、AC6（Task 6 Step 1）、AC7（Task 6 Step 2）。
- **占位符扫描**：无 TBD/TODO；所有 bash 代码完整给出。
- **类型/名称一致性**：函数名与 xtask 同名（`line_layer_violations` / `tool_profile_violations` / `tools_authorization_violations` / `tools_boundary_violations` / `feature_layer_for` / `is_test_path` / `named_block` / `strip_rust_comments`），消息文本逐字一致。
- **已知风险**：
  1. perl 与 Rust regex 的细微差异（如 `\b` 对 Unicode 的处理）——通过 Task 3 双实现对照兜底；
  2. `named_block` 的 brace 计数对字符串字面量中的 `{` 不敏感（与 Rust 版行为一致，非回归）；
  3. `done <<< "$source"` 处理以换行结尾的大文件时末尾多一空行迭代——`line_layer_violations` 对空行直接返回，无影响。
