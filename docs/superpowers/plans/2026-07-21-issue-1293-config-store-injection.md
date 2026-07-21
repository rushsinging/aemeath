# #1293 Config Override Storage 注入实施计划
# #1293 Config Override Storage 注入实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Config override Storage 的具体 filesystem 构造权上移到 Composition，同时保持 Config 的分层优先级、durable update 和错误语义不变。

**Architecture:** Config 保留 `NativeConfigStore` 的 key、codec、priority 与错误映射；其 `wire_project_config*` 显式接收该 store。Composition 在每个 deployable bootstrap 中构造 `config-overrides` 的 `AtomicBlobPort`，包装为 `NativeConfigStore` 后传入 Config wiring。Runtime、SDK、TUI 仍仅消费已有的 Config 窄视图。

**Tech Stack:** Rust、Tokio、`AtomicBlobPort`、`NativeConfigStore`、Cargo tests、shell architecture guards。

---

## 文件结构

| 文件 | 职责 / 改动 |
|---|---|
| `agent/features/config/src/application.rs` | 将 Config project factory 改为接收 injected `NativeConfigStore`；删除 Config application 内 filesystem backing 选择。 |
| `agent/features/config/src/lib.rs` | 发布更新后的 wiring 函数签名。 |
| `agent/features/config/src/application_tests.rs`（若从现有内联测试迁移）或现有 `application.rs` test section | 覆盖 injected store 驱动的 bootstrap、priority、durable update 和 error mapping。必须遵循仓库 test-file 分离约束；实现时先确认当前模块实际 test 组织。 |
| `agent/composition/src/app.rs` | 提取唯一 `wire_config_override_store()` helper，并让三个 deployable bootstrap 传入同一类 Composition 构造的 store。 |
| `agent/composition/src/app_tests.rs`（若存在）或 `agent/composition/tests/config_wiring.rs` | 覆盖 Composition 构造/forward Config override store，且不泄漏 Config service 到 Runtime。 |
| `.agents/hooks/check-config-adapter-boundary.sh` 或新增 `check-config-store-ownership.sh` | 将 Config application 禁止 `file_system_blob` 的构造权规则机械化；不新增 allow/exclude。 |
| `.agents/hooks/check-architecture-guards.sh`、`.agents/architecture-guard-registry.json` | 注册 guard 及结构化 policy metadata。 |
| `docs/design/02-modules/config/01-config-layer.md` | 更新 Target/实现对齐说明：Composition 构造 store，Config 接收 store。 |
| `docs/design/01-system/03-context-map.md`、`docs/design/03-engineering/{01-architecture-guards,03-migration-governance}.md` | 更新跨 BC 构造权、Guard 索引及 Current→Target 证据。 |

## Task 1：先建立 Config 注入契约失败测试

**Files:**
- Modify: `agent/features/config/src/application.rs` 的现有测试区，或按当前仓库约定创建对应外置测试文件。

- [ ] **Step 1: 读现有 Config test 组织和所有 `wire_project_config*` 消费者**

Run:
```bash
grep -RInE 'wire_project_config(_with_cli)?\(' agent --include='*.rs'
grep -RInE 'ConfigAppService::for_project|file_system_blob\(' agent/features/config/src --include='*.rs'
```

Expected: 确认 production call sites 与已有 test harness，避免创建第二套 fixture。

- [ ] **Step 2: 写失败测试：wiring 使用 injected store 恢复 durable override**

在现有 Config test harness 中构造两个相互隔离的 `FileSystemBlobAdapter`：一个作为 injected override store，另一个作为未使用的目录。通过 `NativeConfigStore::new(injected_blob)` 调用新签名 wiring，先写入 model override，再重建 wiring，断言 override 仍被读取。

Test intent:
```rust
#[tokio::test]
async fn wiring_reads_runtime_override_from_injected_native_store() {
    // project + injected storage root + isolated global config
    // write ConfigUpdate::SetModel through first wiring
    // rebuild with the SAME injected NativeConfigStore
    // assert committed_snapshot().model_name() == "injected/model"
}
```

- [ ] **Step 3: 运行定向测试，确认在 API 尚未改造前失败**

Run:
```bash
cargo test -p config wiring_reads_runtime_override_from_injected_native_store -- --nocapture
```

Expected: 编译失败或断言失败，原因是 wiring 尚不能接收 injected store；不得因环境路径碰巧命中而通过。

## Task 2：Config 接收 store，退役 application 内 Storage 构造

**Files:**
- Modify: `agent/features/config/src/application.rs`
- Modify: `agent/features/config/src/lib.rs`
- Test: Task 1 的测试文件

- [ ] **Step 1: 将 `wire_project_config_with_cli` 显式增加 store 参数**

目标签名：
```rust
pub async fn wire_project_config_with_cli(
    project_dir: &Path,
    native_store: NativeConfigStore,
    cli: CliConfigInput,
) -> Result<ConfigWiring, ConfigError>
```

- [ ] **Step 2: 将无 CLI 的 wiring 增加 store 参数**

目标签名：
```rust
pub async fn wire_project_config(
    project_dir: &Path,
    native_store: NativeConfigStore,
) -> Result<ConfigWiring, ConfigError>
```

- [ ] **Step 3: 收缩 `ConfigAppService::for_project` 为 Config-only initializer**

将其改为接收 `NativeConfigStore`，只负责 canonical project location、active location 与 service assembly：
```rust
fn for_project(
    project_dir: &Path,
    native_store: NativeConfigStore,
) -> Result<Self, ConfigError>
```

删除其中的 `storage::api::file_system_blob(...)` 与 `NativeConfigStore::new(...)`。保留 `ConfigError::InvalidLocation` 和 `ConfigError::Load` 的既有映射语义。

- [ ] **Step 4: 更新 crate-root forwarding signature**

`config/src/lib.rs` 的 public forwarding 函数同步要求 `NativeConfigStore`；不得保留带默认 filesystem backing 的兼容重载。

- [ ] **Step 5: 运行 Task 1 测试并确认通过**

Run:
```bash
cargo test -p config wiring_reads_runtime_override_from_injected_native_store -- --nocapture
```

Expected: PASS；重建后由同一 injected store 恢复 override。

- [ ] **Step 6: 覆盖 priority、durable update 与错误映射回归**

复用现有 `ConfigUpdate` 测试，增加/调整断言：
- CLI patch 仍覆盖 injected native override；
- `prepare_update` 在 `commit_update` 前不发布；
- Storage failure 仍映射为既有 `ConfigPersistError` / `ConfigUpdateError`，不得变成 panic 或 generic error。

Run:
```bash
cargo test -p config --lib -- --nocapture
```

Expected: PASS。

## Task 3：Composition 唯一构造 override store

**Files:**
- Modify: `agent/composition/src/app.rs`
- Modify: `agent/composition/src/app_tests.rs` 或 Create: `agent/composition/tests/config_wiring.rs`

- [ ] **Step 1: 写失败测试：Composition source 负责唯一构造 `config-overrides` store**

测试必须检查真实 Composition helper，而不是 mock Config internals。断言三个 deployable bootstrap 都通过同一 helper 取得 Config store；Config crate 不出现在 helper 内的 filesystem construction 责任中。

- [ ] **Step 2: 增加 Composition-private helper**

目标形状：
```rust
fn wire_config_override_store() -> Result<config::NativeConfigStore, SdkError> {
    let blob = storage::api::file_system_blob(
        share::config::paths::global_agents_dir().join("config-overrides"),
    )?;
    Ok(config::NativeConfigStore::new(blob))
}
```

错误应映射为既有 bootstrap `SdkError::Init` 语义；不得在 Config application 中恢复 filesystem selection。

- [ ] **Step 3: 在所有 deployable bootstrap 调用点注入 store**

`build_agent_client`、test bootstrap（若实际走 production helper）和 `build_agent_bootstrap` 都在 Config wiring 调用前取得 store，并传入 `wire_project_config_with_cli`。同一 bootstrap 只调用一次 helper。

- [ ] **Step 4: 运行 Composition 定向测试**

Run:
```bash
cargo test -p composition config -- --nocapture
```

Expected: PASS；Composition 负责 construction，Runtime 仅收到 `ConfigWiring`。

## Task 4：构造权 Guard 与故意违规证据

**Files:**
- Modify: `.agents/hooks/check-config-adapter-boundary.sh` 或 Create: `.agents/hooks/check-config-store-ownership.sh`
- Modify: `.agents/hooks/check-architecture-guards.sh`
- Modify: `.agents/architecture-guard-registry.json`
- Create: 对应 `*-tests.sh` guard sanity script

- [ ] **Step 1: 写 Guard 负例 fixture**

至少构造两种违规：
1. `agent/features/config/src/application.rs` 中加入 `storage::api::file_system_blob(...)`；
2. 非 Composition production 文件中加入 Config override filesystem construction。

每种必须以 exit 2 和稳定诊断失败；恢复 fixture 后单 guard 通过。

- [ ] **Step 2: 实现结构化 Guard**

规则：
- Config application production source 禁止 `storage::api::file_system_blob` 和 `FileSystemBlobAdapter::new`；
- Composition `app.rs` 必须含唯一 override store helper 并将其传给 Config wiring；
- 不新增 path/file/line allow、exclude 或 skip。

- [ ] **Step 3: 注册 guard**

在 full/fast 合适 profile 中加入 `check-architecture-guards.sh`，在 registry 以 `target_capability_policy` 登记 `#1293`、owner、scope、exit condition；同步 Architecture Guards 文档。

- [ ] **Step 4: 运行 negative evidence 与总编排**

Run:
```bash
.agents/hooks/check-config-store-ownership-tests.sh
bash .agents/hooks/check-architecture-guards.sh --full
```

Expected: 所有故意违规均 exit 2；恢复后完整编排 PASS。

## Task 5：文档、Issue 门禁与完整验证

**Files:**
- Modify: `docs/design/02-modules/config/01-config-layer.md`
- Modify: `docs/design/01-system/03-context-map.md`
- Modify: `docs/design/03-engineering/01-architecture-guards.md`
- Modify: `docs/design/03-engineering/03-migration-governance.md`
- Modify: GitHub Issue #1293 body/comment

- [ ] **Step 1: 更新 Target/Current 文档**

明确：Composition 构造 `config-overrides` AtomicBlob 和 Config-owned `NativeConfigStore`；Config wiring 消费 injected store；Config 仍独占 key/codec/priority/active state；Runtime/TUI/CLI 不获得 store 或 Storage Port。

- [ ] **Step 2: 回填 #1293 差异表与门禁**

记录 production source 零 Config filesystem construction、测试层证据、Guard negative evidence、无白名单净增；未创建 PR 前不得关闭 Issue。

- [ ] **Step 3: 运行完整验证**

Run:
```bash
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
bash .agents/hooks/check-architecture-guards.sh --full
```

Expected: 全部 PASS。

- [ ] **Step 4: 检查退役路径**

Run:
```bash
grep -RInE 'storage::api::file_system_blob|FileSystemBlobAdapter::new' \
  agent/features/config/src --include='*.rs'
```

Expected: Config production source 无匹配；测试文件仅允许 injected fake/adapter fixture，不得重建 production factory。

- [ ] **Step 5: Commit**

```bash
git add agent/features/config agent/composition .agents docs/design
git commit -m "refactor(config): #1293 注入 override Storage"
```

Commit body MUST include `Refs #1293`, `Refs #950`, and the repository-standard AI co-author trailer.
