# Issue #1055 执行计划

> 按 TDD 与 L0～L5 测试分层执行；本计划 PR 只固化执行路径，不修改业务代码。对应 Issue：[#1055](https://github.com/rushsinging/aemeath/issues/1055)，父 Issue：[#762](https://github.com/rushsinging/aemeath/issues/762)。

## 目标

依据 `docs/design/03-engineering/04-testing-and-coverage.md`，审查 #762 及其直接执行叶子的测试完整性、分层合理性与可追溯性；补齐关键缺口，记录覆盖率与生产可达性证据，并给出父项级验收结论。

## 基线与范围

#762 当前直接叶子为 #868、#869、#870、#871、#872、#994、#1055；除本 Issue 外均已关闭。#1055 的原生 `blocked-by` #869、#871、#872 已解除。实施前 MUST 再查询直接叶子；后续新增行为叶子 MUST 纳入依赖和矩阵。

本 Issue 只负责：

1. 建立「行为 / 风险 → 必要测试层 → 现有证据 → 缺口 → 承接 Issue」矩阵。
2. 补齐缺失测试和必要的测试基础设施。
3. 运行 L0～L5 适用门禁、覆盖率与生产可达性审查。
4. 回写 Migration Governance、#1055 与 #762 的验收结论。

若测试暴露业务实现或 Target 文档错误，MUST 创建或关联独立业务 Issue 并设置阻断；NEVER 用适配错误现状的测试固化行为。纯测试移动 MUST 独立 PR。

## 执行步骤

### 1. 建立审查基线

1. 从最新 `origin/main` 创建独立 worktree，记录基线 commit、工具链与 hooks 状态。
2. 查询 #762 的全部直接叶子、原生依赖和最终交付说明。
3. 核对 Context Management README、Session、Compact、Token Budget、Prompt/Guidance、Memory Injection、Runtime Recovery、Ubiquitous Language 与 Migration Governance。
4. 将能力拆为 ContextPort PL、window 组装、finalized Step、Session Envelope、AtomicBlob 恢复、compact/revision、Main Session gate、跨 BC restore、Runtime 边界、Composition 装配、Session 管理 façade、Guard/退役十二组。
5. 在 Migration Governance 的 Context 验收区域建立矩阵；每行记录行为、风险、设计章节、叶子 Issue、必要层级、测试路径、覆盖分支、缺口类型、承接项与结论。

### 2. L0 架构与生产可达性

1. 盘点 Context crate façade、Runtime→Context、Session/ChatChain、legacy writer、AtomicBlob、Workspace ownership 相关 Guard 的真实扫描范围。
2. 用临时探针验证以下违规均被单 Guard 和总编排阻断，恢复后 clean pass：
   - Runtime 引用 `ChatChain`；
   - Runtime 恢复索引式消息归属；
   - Runtime 恢复 Session writer / save callback；
   - 外部穿透 Context 私有层；
   - canonical writer 输出 top-level `messages` / `cwd`。
3. 执行 production-only check/clippy、all-targets clippy、production reachability、public surface/source guard。
4. 检查 test-only API、`allow(dead_code)`、旧兼容入口和仅测试可达生产代码；覆盖率与生产可达性分别判定。

### 3. L1～L3 finalized Step 与字段契约

按 TDD 在 `agent/features/runtime/src/application/context_coordination_tests.rs` 与 `ports/context_port_tests.rs` 补齐：

1. `AppendReceipt` 的 `run_id`、`step_id`、revision、fingerprint 回传。
2. `source_request_id`、`expected_revision` 与 frozen request 的字段完整性。
3. `Completed`、`UserCancelledStep`、`RunTerminated` 三种 finalize cause。
4. 非空 Tool/Agent receipts、artifact、possible side effect、unfinished call 与 API token usage。
5. revision/content conflict typed error，且无隐藏重试。
6. 非默认 `ContextRequest` 全字段和 `ContextWindow` 全字段透传。
7. fingerprint 确定性及关键字段敏感性；相同输入稳定、不同 finalized 事实不可碰撞为同一业务 fingerprint。

### 4. L1～L3 compact、Session 与持久化契约

1. 补 `CompactRequest.source_revision`、stale revision、append→compact→append 单调性。
2. 覆盖 `ResumeProtection`、`HookBlocked`、`CircuitBreakerOpen`，以及 clear 后重新 append。
3. 核验 compact recent tail 不拆分 finalized Step，成功事实和 Tool 协议顺序不丢失。
4. 复核 `session_envelope_codec.rs` 的 current/legacy/future、missing/empty/captured、ledger、workspace identity 与 canonical writer 矩阵。
5. 复核 `session_persistence_service.rs` 的 primary、previous/promote、双代 quarantine 与 future bytes preservation。
6. 复核 `session_snapshot_store_contract.rs` 的 key/generation/reason/error ACL；相同契约若存在多个 adapter，MUST 一次定义并用 factory 复用。

### 5. L2～L4 Main Session 恢复原子性

在 `main_session_wiring.rs`、`main_session_gate.rs`、`main_session_config_facade.rs` 中逐项证明：

1. Session load/decode、Project、Config、Memory、Task 每个 fallible prepare 点失败时，全 live state 保持旧版。
2. 失败断言覆盖 Session identity/history/revision、Workspace、Task、Memory binding、Config active state/watch。
3. shared holder 未退出时 exclusive resume 不提交；bind/query 在切换完成前阻塞。
4. 并发观察者只见完整旧版或完整新版，prepare token 不可见。
5. Memory 安装早于 Config watch；durable handoff 后 caller drop 不取消提交。
6. captured empty 与 legacy missing Task 均清空旧 Task；Workspace missing/empty 返回 typed error。
7. 启动 `--resume` 与运行期 `/resume` 使用相同 fixture，最终 Session、Workspace、Task、Memory、Config 和 Runtime projection 等价。

### 6. L4 生产 Composition 场景

扩展 `agent/composition/tests/main_session_wiring.rs`：

1. 用真实 Composition 创建 Main Session wiring。
2. 驱动 Context window 构建与一次 finalized Step 提交。
3. 证明 canonical Session writer 实际落盘，而非只证明类型接线存在。
4. drop 后重新装配并 resume，断言 Session ID、revision、history 一致。
5. 断言恢复后的 Memory、Workspace、Task、Config 使用目标 project identity。
6. 保留现有生产 Memory 持久化与 Session ID 不漂移证据，避免重复断言。

### 7. 确定性与组织审查

1. 时间、ID、随机源固定或注入；异步推进有上限。
2. 文件测试使用每测试唯一临时目录；不修改全局 cwd，不访问真实用户目录。
3. env 测试串行隔离；NEVER 用短 `sleep` 或重跑成功掩盖 flaky。
4. fixture/Fake/Scripted Driver 归真实 owning layer；不新增万能 `test_utils`、`mod.rs` 或 `include!`。
5. 测试名表达行为、条件与结果；失败信息包含关键不变量和上下文。

### 8. 覆盖率与最终门禁

依次运行并保留首次结果：

1. Context 定向测试。
2. Runtime Context coordination 定向测试。
3. Composition Main Session wiring 测试。
4. `cargo test --workspace`。
5. `cargo fmt --check`。
6. `cargo run -p xtask -- production-reachability .`。
7. `cargo clippy --workspace --all-targets -- -D warnings`。
8. 相关单 Guard、负例探针与 `.agents/hooks/check-architecture-guards.sh`。
9. 适用的 slow-test matrix。
10. `./scripts/coverage.sh`。

记录 workspace、Context、Runtime、Composition 的 line/region/function 与 changed-lines 信号，并解释未覆盖关键分支。百分比只作风险信号，NEVER 替代行为矩阵。

### 9. L5 适用性

Context/Session 核心职责可由进程内 L2～L4 Harness 覆盖，默认不新增 Context 专属 L5。运行现有 CLI 启动/退出 smoke；只有真实跨进程文件锁或进程重启无法由 Harness 证明时，才增加一个有界、隔离且不访问 provider 的 process smoke。

### 10. 回写与父项验收

1. 在 Migration Governance 写入矩阵、L0～L5 结论、覆盖率、缺口与阻断项。
2. 将测试证据和结论回写 #1055。
3. 将父项级测试审查结论同步到 #762。
4. 发现大范围缺口时创建原生 sub-issue，并设置正确 blocked-by。
5. 关键行为未闭合时，#762 NEVER 完成；Issue 关闭均等待用户确认。

## 拆分条件

出现任一情况时 MUST 拆为原生 sub-issue，而非扩大本 PR：

- 需要修改 Session/Runtime 业务语义；
- 发现 future schema、恢复原子性或 finalized Step 数据丢失缺陷；
- 需要新增跨进程 resume Harness；
- 需要重构跨 adapter 共享 contract suite；
- Composition 场景需要较大测试基础设施；
- 纯测试目录迁移与行为缺口修复无法保持独立。

## 完成定义

- #762 全部直接行为叶子已纳入矩阵，后续新增叶子已同步依赖。
- 适用的 L0～L5 证据可追溯，无未解释空白。
- 测试缺口已补齐，或由有 owner 的原生 Issue 承接。
- 所有最终门禁通过，首次失败与 flaky 处置有记录。
- Migration Governance、#1055 与 #762 已同步结论。
- 用户确认后才关闭 Issue。
