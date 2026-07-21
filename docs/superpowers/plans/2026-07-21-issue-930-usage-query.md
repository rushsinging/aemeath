# Issue 930 Usage 查询实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 Audit 通过 `UsageQueryPort` 提供按关联 ID、provider、model 与时间过滤的 Usage 查询、opaque cursor 分页、损坏行 warning 和纯 token 汇总。

**Architecture:** `application/query.rs` 持有版本化 decode、过滤、分页及汇总策略；`adapters/query.rs` 仅通过 `UsageAppendStorePort` 逐分区读取。查询入口保持在 Audit crate，不引入 Runtime、CLI 或 TUI 依赖，也不泄漏 AppendLog 类型。

**Tech Stack:** Rust、Tokio、serde_json、async_trait、Audit `UsageAppendStorePort`。

---

### Task 1: 建立查询行为的失败契约

**Files:**
- Create: `agent/features/audit/tests/usage_query_contract.rs`

- [x] 使用 `FileUsageAppendStore` 与每测试独立临时目录构造固定 stream/line 数据，验证 `UsageQueryPort` 的读取策略。
- [x] 编写失败测试：按 Session、Run、RunStep、Invocation、provider、model 和半开时间范围的 AND 过滤。
- [x] 编写失败测试：跨分区分页、opaque cursor 续页、非法 cursor、超出上限 limit、空结果。
- [x] 编写失败测试：坏中间行与未终结尾行产生 `CorruptLine` warning，但不阻断其余记录。
- [x] 编写失败测试：summary 复用过滤、汇总六类 token、optional token 按零处理、不出现 Cost。
- [x] 运行 `cargo test -p audit --test usage_query_contract`，确认因 query 实现尚不存在而失败。

### Task 2: 实现 Audit 查询策略与 adapter

**Files:**
- Create: `agent/features/audit/src/application/query.rs`
- Create: `agent/features/audit/src/adapters/query.rs`
- Modify: `agent/features/audit/src/application.rs`
- Modify: `agent/features/audit/src/adapters.rs`
- Modify: `agent/features/audit/src/lib.rs`

- [x] 在 application 定义单一 query policy：时间范围验证、版本化 envelope decoder、坏行映射、统一过滤 predicate、cursor codec 与 token accumulator。
- [x] cursor 编码包含下一条待扫描的 stream 与行偏移；仅接受当前实现生成、且与本次过滤范围一致的 cursor，其他返回 `UsageQueryError::InvalidCursor`。
- [x] 在 adapter 通过 `UsageAppendStorePort` 实现 session 定向读取和有序跨分区逐个扫描；append-store 错误映射为不含内部类型的 `UsageQueryError::Storage`。
- [x] 发布构造函数及 `UsageQueryPort` 实现，保持 crate-root 窄 façade；不扩展 query-facing PL，不增加 Cost、CLI/TUI 或 Runtime 依赖。
- [x] 运行 `cargo test -p audit --test usage_query_contract`，确认全部通过。

### Task 3: 回写目标文档和 Issue 门禁

**Files:**
- Modify: `docs/design/02-modules/audit/README.md`
- Modify: `docs/design/02-modules/audit/01-usage-storage.md`
- Modify: `docs/design/03-engineering/03-migration-governance.md`

- [x] 回写 query 实际目录、cursor 边界、坏行及 I/O 错误语义、测试证据；记录 Cost/Pricing、retention、production wiring 与旧 CostTracker 的后续 owner。
- [ ] 更新 Issue #930 的开发中与完成前 checklist，以及“开发前文档—代码差异”各项最终状态和 PR Test Plan。

### Task 4: 验证和收口

**Files:**
- Verify: `agent/features/audit/**`

- [x] 运行 `cargo fmt --all -- --check`。
- [x] 运行 `cargo check -p audit`。
- [x] 运行 `cargo test -p audit --tests`。
- [x] 运行 `cargo clippy --workspace --all-targets -- -D warnings`。
- [x] 运行 `PATH="/opt/homebrew/bin:$PATH" bash .agents/hooks/check-architecture-guards.sh`。
- [x] 运行 `git diff --check`，逐项对照 #930 验收项和文档门禁；未闭合项必须回填 Issue，不得声称完成。
