# #1296 Composition 构造权 Guard 与 #950 验收实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 建立零例外的 Composition-only 跨 BC concrete-construction 聚合 Guard，并完成 #950 的四个 leaf 验收、文档与预算回填。

**Architecture:** 不复制 #1292–#1295 的业务构造规则。新增的聚合 Guard 只验证四个 leaf ownership Guard 均已注册至总编排、registry 均为 active target policy、且 Runtime/Config/Context 三侧的代表性回流均会由对应 leaf Guard 阻断。四个 leaf Guard 继续是各自 concrete constructor 的唯一可执行真相；聚合 Guard 是 #950 的验收闸门。

**Tech Stack:** Rust workspace、Bash、Python 标准库、Guard Registry xtask、GitHub Issues。

---

## 最终差异矩阵

| Leaf | 已完成边界 | 当前可执行证据 | #1296 需要补足 |
|---|---|---|---|
| #1292 | Composition 创建 Session backing；Context/Runtime 接收同一 `SessionManagementPort` | `check-session-management-ownership.sh`、`check-session-project-scope.sh` | 注册/编排/负例聚合证明，#950 回填 |
| #1293 | Composition 创建 override blob；Config 接收 `NativeConfigStore` | `check-config-store-ownership.sh` | 注册/编排/负例聚合证明，#950 回填 |
| #1294 | Composition 创建 Tool/Skill/Tool Result/ActiveRun | `check-runtime-tool-assembly-ownership.sh` | 注册/编排/负例聚合证明，#950 回填 |
| #1295 | Composition 创建 Hook dispatcher；Runtime Main/Sub 复用 `HookPort` | `check-runtime-hook-assembly-ownership.sh` | 注册/编排/负例聚合证明，#950 回填 |

MCP Ready、连接/断连、动态 Catalog revision 与稳定身份均不属于 #1296，继续唯一归 #1327。聚合 Guard 仅确保 Runtime 不恢复 #1294 已删除的私有 MCP seam，不构造或验证 MCP manager。

## 文件职责

| 文件 | 责任 |
|---|---|
| `.agents/hooks/check-composition-construction-ownership.sh` | 聚合检查四个 leaf Guard 是否存在、已在总编排注册、已在 Guard Registry 作为 active target policy 登记；以四个真实代表性 source fixture 验证 leaf Guard 拦截语义 |
| `.agents/hooks/check-composition-construction-ownership-tests.sh` | 构建临时仓库，逐项移除 leaf Guard 注册/registry policy 或制造 Runtime/Config/Context 回流，验证聚合 Guard exit 2 和诊断 |
| `.agents/hooks/check-architecture-guards.sh` | 将聚合 Guard 加入 fast 编排，保持 leaf Guard 先后顺序与现有 full 行为 |
| `.agents/architecture-guard-registry.json` | 登记 #1296 的 `policy.composition.cross-bc-construction-ownership` target policy，例外为零 |
| `docs/design/03-engineering/01-architecture-guards.md` | 记录聚合 Guard 的职责、依赖 leaf 与失败语义 |
| `docs/design/03-engineering/03-migration-governance.md` | 更新 O2：#950 四个 leaf 已完成，#1296 负责最终聚合验收；明确 MCP 仍属 #1327 |
| `docs/design/02-modules/runtime/06-ports-and-adapters.md` | 追加 #1296 验收记录，明确聚合 Guard 不承担 MCP lifecycle |
| `docs/design/02-modules/context-management/01-session.md` | 追加 #1296 构造权最终验收记录，链接 Session leaf 与聚合 Guard |

## Task 1：为聚合 Guard 建立红灯负例

**Files:**
- Create: `.agents/hooks/check-composition-construction-ownership-tests.sh`

- [ ] **Step 1: 写临时合规仓库 fixture**

fixture 必须具备：
- 四个 leaf Guard 脚本；
- `check-architecture-guards.sh` 内四个 leaf 调用；
- registry 中四个 active target policy；
- Runtime `from_args.rs`、Config `application.rs`、Context Session adapter 的合规最小源文件。

- [ ] **Step 2: 写四类故意违规场景**

1. 删除 Runtime Tool Guard 的编排调用；
2. 删除 Config Store Guard 的 registry entry；
3. 向 Runtime fixture 插入 `FileSystemBlobAdapter::new()`；
4. 向 Config fixture 插入 `file_system_blob()`；
5. 向 Context fixture 插入 `file_system_blob()`；
6. 向 Runtime fixture 插入 `hook::build_dispatcher()`。

每个场景都必须断言 exit code 为 `2`，并匹配专属中文或英文诊断片段。

- [ ] **Step 3: 运行脚本并确认 RED**

Run: `bash .agents/hooks/check-composition-construction-ownership-tests.sh`

Expected: 因聚合 Guard 尚不存在而失败，或因目标 Guard 未注册而失败；不得因 fixture 缺失、Shell 语法或临时目录错误失败。

## Task 2：实现 Composition-only 聚合 Guard

**Files:**
- Create: `.agents/hooks/check-composition-construction-ownership.sh`
- Modify: `.agents/hooks/check-architecture-guards.sh`
- Modify: `.agents/architecture-guard-registry.json`

- [ ] **Step 1: 实现 registry/编排一致性检查**

Guard 必须验证以下四个 leaf Guard：

```text
check-session-management-ownership.sh
check-config-store-ownership.sh
check-runtime-tool-assembly-ownership.sh
check-runtime-hook-assembly-ownership.sh
```

要求：
- 每个脚本存在且可执行；
- 每个脚本在 `check-architecture-guards.sh` 中以 `run_guard fast` 注册；
- 每个脚本在 registry 存在 `classification: target_capability_policy`、`status: active` 的 entry；
- registry 不得新增 migration exception、`grep -v`、allowlist 或排除路径。

- [ ] **Step 2: 实现三侧生产源聚合检查**

聚合 Guard 仅做“代表性回流面”验证，具体模式仍由 leaf Guard 负责：

| Source | 必须由对应 leaf 阻止的模式 |
|---|---|
| Runtime `application/client/from_args.rs` | `FileSystemBlobAdapter::new`、`tools::composition::wire_`、`hook::build_dispatcher` |
| Config `application.rs` | `file_system_blob`、`FileSystemBlobAdapter::new` |
| Context `adapters/atomic_blob_session_management.rs` | `file_system_blob` |

每个命中输出该 source、被破坏的 leaf boundary 与修复方向；exit code 为 2。

- [ ] **Step 3: 在总编排与 registry 注册**

在所有四个 leaf Guard 后增加：

```bash
run_guard fast "$HOOKS_DIR/check-composition-construction-ownership.sh"
```

registry 新增单一 target policy：
- id: `policy.composition.cross-bc-construction-ownership`
- guard: `check-composition-construction-ownership.sh`
- module: `composition`
- tracking issue: `1296`
- scope: `agent/{composition,features}/{runtime,config,context}` 的可审计 path prefix 或等价结构化范围；
- exit condition: 只允许由等价聚合验收 policy 替代。

- [ ] **Step 4: 运行聚合 Guard 并确认 GREEN**

Run:

```bash
.agents/hooks/check-composition-construction-ownership.sh
bash .agents/hooks/check-composition-construction-ownership-tests.sh
cargo run -p xtask -- guard-registry check
```

Expected: 三条命令均成功；负例脚本内部每种违规均观察到 exit 2 后恢复合规。

## Task 3：补齐聚合 Guard 的可执行文档

**Files:**
- Modify: `docs/design/03-engineering/01-architecture-guards.md`
- Modify: `docs/design/03-engineering/03-migration-governance.md`
- Modify: `docs/design/02-modules/runtime/06-ports-and-adapters.md`
- Modify: `docs/design/02-modules/context-management/01-session.md`

- [ ] **Step 1: 更新 Architecture Guards 索引**

新增聚合 Guard 条目，明确它：
- 不重复 leaf Guard 的 concrete pattern；
- 验证 leaf 注册、编排、registry active policy 与三侧代表性回流；
- 不构造或验证 MCP lifecycle，MCP 仍由 #1327 所有。

- [ ] **Step 2: 更新 Migration Governance O2**

将 #950 四个 leaf 标为完成，写明 #1296 的最终完成条件：聚合 Guard、无例外 registry、四类负例、完整编排和全量验证。移除任何“#950 尚待全部 adapter 上移”的 Current 描述。保留 #1022 的 capability-first 正式 boundary 后续责任。

- [ ] **Step 3: 修正 Runtime/Context Target 历史记录**

Runtime Target 添加 #1296 历史记录：Composition-only concrete-construction 聚合验收完成，MCP lifecycle 保持 #1327。Context Session 文档添加同等记录：Session backing 由 #1292 实现、#1296 只提供聚合验收，Port 契约不被 Guard 替代。

- [ ] **Step 4: 文档交叉核对**

Run:

```bash
grep -R "MCP manager\|CatalogExecutionWiring\|#950.*adapter" -n docs/design .agents | head -80
grep -R "#1296" -n docs/design .agents
```

Expected: 不存在将 MCP lifecycle 重新归给 #950/#1296 的目标描述；#1296 引用均说明其为最终 Guard/验收 leaf。

## Task 4：回填 #950/#1296 和最终验证

**Files:**
- Modify: GitHub Issue #950 body/comment
- Modify: GitHub Issue #1296 body/comment

- [ ] **Step 1: 回填 #950 parent completion evidence**

在 #950 追加或更新完成表：
- #1292 PR #1307；
- #1293 PR #1314；
- #1294 PR #1329；
- #1295 PR #1360；
- #1296 本 PR 的聚合 Guard、负例、registry 与验证证据；
- MCP Ready/lifecycle 明确外置至 #1327。

所有过期的 RuntimeAssembly MCP manager 表述必须修正为历史计划，不能与 target 相冲突。

- [ ] **Step 2: 回填 #1296 验收与预算**

勾选验收前必须记录：
- repository migration debt 仍为 6，Runtime 5、TUI 1；
- 新增 target policy 1 个，migration exception/allow/exclude 净增 0；
- 四个 leaf Guard + 聚合 Guard 的负例均 exit 2；
- 完整架构 Guard、fmt、production reachability、clippy 和 workspace tests 结果。

- [ ] **Step 3: 执行最终验证**

Run:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
.agents/hooks/check-architecture-guards.sh --full
cargo run -p xtask -- guard-registry check
git diff --check
```

Expected: 所有命令 exit 0；若任一失败，停止并按失败来源修复，不以重跑覆盖。

- [ ] **Step 4: 提交**

```bash
git add .agents docs
# 仅在确认本次 diff 不含无关变更后提交
git commit -m "refactor(composition): #1296 锁定跨 BC 构造权"
```

提交前重读 #1296 验收清单；未完成项必须在 Issue/PR 记录原因与承接，不能声称完成。
