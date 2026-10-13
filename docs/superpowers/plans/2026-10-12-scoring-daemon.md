# Scoring Daemon Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 System One 生产路径从「每会话进程内 embedded」改为「本机单一 scoring daemon + UDS 客户端」，多会话共享一份 GGUF；彻底退役进程内 embedded 装配。

**Architecture:** `aemeath systemone serve`（feature `systemone-daemon`）持有 llama.cpp worker；会话经 `DaemonScoringClient` 实现 `ScoringPort`；装饰链仍为 calibrated → audited → client。事件流继续由客户端 audited 写（#1909）。设计真相源：`docs/design/02-modules/systemone/04-scoring-daemon.md`。

**Tech Stack:** Rust 2021、Tokio、Unix domain socket、既有 llama-cpp-2 / PointerHead、composition 装配、CLI 子命令。

**Issue:** https://github.com/rushsinging/aemeath/issues/1935

---

## PR0 — 设计文档（本批次）

- [x] 新增 `04-scoring-daemon.md`（Q1–Q17）
- [x] 修订 `01-systemone-scoring.md` / README / `02-kev-deployment.md` 交叉引用
- [ ] 开文档 PR，Refs #1935

---

## PR1 — daemon + 协议 + 单实例

### Task 1: 协议 DTO 与帧编解码（TDD）

**Files:**
- Create: `agent/features/systemone/src/domain/daemon_protocol.rs`（或同等路径）
- Test: 同模块 `*_tests.rs`

- [ ] **Step 1:** 红 — `protocol_version` round-trip；长度前缀边界；版本不匹配错误 kind
- [ ] **Step 2:** 绿 — 实现 length-prefixed JSON 编解码
- [ ] **Step 3:** 提交 `feat(systemone): #1935 daemon 协议帧与 DTO`

### Task 2: 单实例锁 + sock 路径解析

**Files:**
- Create/Modify: serve 侧 lock/path helpers
- Test: 锁互斥、陈旧 sock unlink 条件

- [ ] 红 → 绿 → 提交 `feat(systemone): #1935 scoring.lock 与 sock 路径`

### Task 3: `aemeath systemone serve` 最小可运行

**Files:**
- CLI 子命令接线；`DaemonServe` listener + 串行 worker 接入既有 llama worker
- feature `systemone-daemon` 门控

- [ ] 红：无模型时 serve 失败并提示 download
- [ ] 绿：有模型时 ping/answer 一次成功（可用 fixture 或 `--ignored` smoke）
- [ ] 提交 `feat(cli): #1935 aemeath systemone serve`

### Task 4: PR1 验收

- [ ] `cargo test -p systemone` 相关单测绿
- [ ] clippy / 架构守卫
- [ ] 开 PR1，Refs #1935

---

## PR2 — 客户端 + 退役 embedded + 惰性拉起

### Task 5: `DaemonScoringClient` 实现 `ScoringPort`（TDD）

- [ ] 红：超时 / 协议错误 → `ScoringUnavailable`
- [ ] 绿：正常 answer 映射概率分布
- [ ] 提交

### Task 6: composition 装配 + 惰性 spawn + 首次提示

- [ ] `scoring.enabled` 且场景开、sock 不通 → spawn serve
- [ ] 会话退出不杀 daemon
- [ ] 首次 Unavailable 提示（可关）；之后静默
- [ ] 提交

### Task 7: 退役进程内 embedded 生产装配

- [ ] 删除 / 停用 composition 中 embedded wiring
- [ ] 测试改为 mock 或真 daemon；更新 79-case 真机门禁前提
- [ ] `build_cli` / feature 文档与默认：会话不链引擎
- [ ] 提交

### Task 8: 配置项登记

- [ ] `socket_path` / `connect_timeout_secs` / `infer_timeout_secs` 贯通 + `specs/3.9-config-compat.md`
- [ ] 提交

### Task 9: PR2 终验 + 关单条件

- [ ] 多进程内存只一份 GGUF（手工或脚本观测）
- [ ] `enabled=false` 零成本
- [ ] 事件流仍由客户端写入
- [ ] 开 PR2；若完整覆盖 #1935 checklist 则 `Closes #1935`，否则 `Refs`

---

## 执行提示

- 每个 Task 必须单一可验证；禁止「实施并验证」笼统步骤。
- 修改代码前再向用户确认 PR1 开工；PR0 仅文档可直接合入流程。
- Stop Hook / 架构守卫：feature 门控与 façade 白名单变更须同步 registry。
