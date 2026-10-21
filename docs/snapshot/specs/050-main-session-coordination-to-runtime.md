# 050：MainSession 协调职责从 context 迁移至 runtime

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/2025
> 状态：设计稿（阶段 0）｜Milestone：v0.2.0

## 1. 背景与根因

`MainSessionWiring`（`agent/features/context/src/application/main_session.rs`）当前是跨 5 个 BC（Project / TaskData / Config / Memory / Context）的依赖捆包：

- composition（`agent/composition/src/runtime.rs:313`）把各 BC port 全部交给 `context::wire_main_session` 打包，runtime client 常驻持有该对象。
- runtime 从它身上取 config（5 个入口）、memory（3 个入口）等一切依赖，scope 被用成 service locator。
- resume 的发起（触发、时序、UI 回执）在 runtime（`runtime/…/client/resume_helper.rs`），但 load canonical、重开 memory corpus、CAS 换 committed holders、gate 独占的**执行**全在 context——编排是 runtime 行为，却由 context 实现并因此持有全部依赖。
- `MainContextFactory::build`（`context/src/ports.rs:24`）签名本就是参数传入式（session / task_persist / workspace_persist / memory / mutation_gate 逐次传入），证明 wiring 常驻持有并转发是多余的 DI 容器行为。

## 2. 目标形态

**不造大 scope**。原 wiring 的职责拆成两块，各自归位：

### 2.1 SessionSwitchCoordinator（runtime，协议编排者）

持有**切换协议的全部编排参与方**——因为 resume 独占区本质是 4-BC prepare/commit 流水线（Project restore → Config prepare → Memory open → Task prepare → 同步 commit + CAS 四元组），协议归位 runtime，参与方随之携带。**MAY 持有一切协议所需依赖，NEVER 暴露任何转发访问器**（config 5 入口 / memory 3 入口 / catalog 等随迁移删除，这是本重构的核心收益）：

```
SessionSwitchCoordinator
  ├ gate: SessionSwitchGate          （shared / exclusive 两级，从 context 迁入，
  │                                    语义等价平移：bind = shared，resume = exclusive，
  │                                    NEVER shared→exclusive 升级）
  ├ mutation_gate: Arc<tokio::sync::Mutex<()>>
  ├ committed: { session: Arc<CanonicalSession>,
  │              memory:   Arc<dyn MemoryPort>,
  │              context:  Arc<dyn ContextPort> }
  │              （Arc<RwLock<…>>，切换时 CAS 替换）
  ├ pending_session_restart_revision: Arc<RwLock<Option<SessionRevision>>>
  ├ session_management: Arc<dyn SessionManagementPort>   （context 原语）
  ├ memory_opener: Box<dyn MemoryOpener>                 （Memory 原语）
  ├ context_factory: Arc<dyn MainContextFactory>         （context 原语）
  ├ workspace_read / workspace_persist                   （Project 原语，协议内 prepare/commit）
  ├ config_reader / config_participant                   （Config 原语，协议内 prepare/commit
  │                                                       + bind 时取 config snapshot）
  └ task_persist: Arc<dyn TaskPersist>                   （TaskData 原语，协议内 prepare/commit）
```

方法面 = 协议方法（MAY 随实施微调）：

- `bind_run()` → shared permit + 原子借出**四元组**（session/memory/context/config snapshot，对齐现 `BoundMainRun`：Run 期 config 固定依赖 shared 区内取快照）
- `resume(session_id)` → 独占区完整协议：load canonical → Project/Config/Memory/Task 逐一 prepare → 全部成功后同步 commit（workspace/task commit + CAS 四元组 + config commit + 清 pending restart）；任一 prepare 失败则 committed 状态不动（对齐现 `resume_prepared` 语义与 `MainSessionError` 分类）
- `mark_session_restart_required(revision)` / `pending_session_restart_revision()`（restart 协议归位）
- `update_active_session_metadata(...)`（session 元数据 CAS 更新，持 mutation_gate）
- `with_shared(f)`（shared permit 内执行读取，对齐现 `with_shared`）

`GateAwareConfigReader`（shared permit 后读 config 的 façade）随协议迁 runtime，供 config snapshot 语义使用。

### 2.2 依赖平铺注入（runtime client 既有字段）

config / workspace / task 等 port 在**协议之外**的消费不再经任何捆包：composition 直接注入 runtime client 的既有字段；runtime 要用 config 就用自己的字段。coordinator 上不提供任何此类转发入口，`committed_config()` / `config_query()` / `config_writer()` 等 5 个转发入口随迁移删除。`project_identity()` 单发查询由 runtime 经自己持有的 workspace port 直查。

### 2.3 context 瘦身后的 PL（本 BC 保留原语）

- `SessionManagementPort`（load / save / list / update metadata）
- `MainContextFactory` + `ContextPort`（build window 的契约不变）
- `CanonicalSession` 及其值对象族（数据类型宿主不变）
- 删除：`MainSessionWiring` / `MainSessionWiringBuilder` / `MainSessionDependencies` / `wire_main_session` / `OwnedSessionSharedPermit`（permit 类型随 gate 迁 runtime）/ `SessionSwitchGate` 整族

`canonical 交换槽`（`Arc<RwLock<Arc<CanonicalSession>>>`）归 coordinator 持有，context 不再持可变运行时状态。

## 3. 原子切换协议

复刻现网语义、纯归位不改行为：

```
bind_run（shared permit，Run 生命周期内持有）:
  持 shared permit → 原子捕获四元组（session/memory/context/config snapshot）
  → BoundRun { permit, session, memory, context, config }

resume（exclusive permit + mutation_gate）:
  独占 → load canonical
       → Project prepare（跨项目校验、路径消失回退当前工作区）
       → Config prepare → Memory derive key + open
       → Task prepare
       ── 全部成功后同步 commit（无 await）──
       workspace.commit + task.commit + CAS 四元组
       + config.commit + 清 pending restart → 释放独占
```

约束：

- NEVER shared→exclusive 升级：需要 resume 的调用方先 drop shared permit（对齐现 `main_session.rs` 文档语义）。
- resume 中任一 prepare 失败：committed 四元组保持原值，错误按 `MainSessionError` 各阶段分类返回。
- 项目切换（switch project）按同协议执行；coordinator 只管 session 协议本身的原子性。

## 4. 交付（单 PR 分 commits）

| Commit | 内容 | 验证 |
|---|---|---|
| C1 | runtime 新建 `SessionSwitchCoordinator` + gate 迁入 + 并发/回滚测试（TDD 先行） | `cargo test -p <runtime>` 新增测试 |
| C2 | runtime client / resume_helper / run_launch 改用 coordinator；composition 改为逐 BC 平铺注入 | composition wiring 测试 + runtime 全量 |
| C3 | 删 context `MainSession*` 族（`application/main_session.rs` 拆解迁移）；根面 PL 收窄；lib.rs docs 更新 | `cargo test -p <context>` + guard |

## 5. 验收

- [ ] 单元：coordinator 并发 bind / resume 互斥、失败回滚（迁移现 `main_session` 相关测试语义）
- [ ] 装配：composition wiring 测试改写后全绿
- [ ] 行为：resume / 项目切换场景测试与 PTY e2e 不回归
- [ ] context 根面不再导出 `MainSession*` 族；`cargo clippy -p context -p runtime` 零警告
- [ ] #1863 白名单重建在其后重启时按新根面执行（`MainSession*` 自然消失）

## 6. 涉及路径

- `agent/features/runtime/src/application/…`（coordinator 新建、client / resume_helper / run_launch 改造）
- `agent/features/context/src/application/main_session.rs`（删除/迁移）
- `agent/features/context/src/ports.rs`、`src/lib.rs`（PL 收窄）
- `agent/composition/src/runtime.rs`（装配改写）
