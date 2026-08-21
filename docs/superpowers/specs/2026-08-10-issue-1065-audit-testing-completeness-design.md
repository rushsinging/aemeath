# Issue #1065 Audit 测试完整性审查设计

> 状态：已实施，证据见 `docs/design/02-modules/audit/01-usage-storage.md` §10
> 范围：父项 #857「Audit：Usage-only 非阻塞审计 MVP」及其直接执行叶子
> 交付：行为—测试矩阵、测试与必要修正、验证证据、设计回写、Issue 更新及独立 PR

## 1. 目标

依据 `docs/design/03-engineering/04-testing-and-coverage.md`，对 Audit Usage-only MVP 建立可追溯的 L0～L5 测试体系，补齐测试、实现或文档缺口，并给出父项级验收结论。

本次不是新增 Audit 业务能力。若审查发现现有实现违反已批准设计，先建立失败证据，再修复根因；若修复明显超出测试收口范围，则记录阻断并在取得用户同意后交由独立业务叶子承接。

## 2. 审查边界

### 2.1 纳入范围

1. Audit Published Language、统一关联 ID、V1 envelope 和敏感内容禁入。
2. File AppendLog 分区、framing、append/flush、读取、枚举和路径安全。
3. bounded sender、worker、指标、失败隔离和 shutdown drain。
4. Usage 查询过滤、分页、cursor、坏行隔离和 token summary。
5. Runtime logical invocation 对 UsageRecord 的构造与非阻塞发出。
6. Composition bridge、session-scoped worker、Main/Sub 共享 sink 与 canonical Session 分区。
7. CLI/frontend 收敛后的 Audit shutdown，且 drain 结果不覆盖原始运行结果。
8. CostTracker、cost history 写路径以及旧 Cost DTO/event/presentation 的退役证据。
9. Audit 及直接关联 crate 的生产可达性、架构门禁与覆盖率信号。

### 2.2 排除范围

1. 新增 Price、Cost 或 Pricing 能力。
2. 保存 prompt、response、thinking、tool 或 hook 原文。
3. 新增 retention、导入器、跨进程 exactly-once 或断电绝对持久语义。
4. 与 Audit 验收无关的目录迁移和架构重构。
5. 仅为提高 workspace 总覆盖率而添加无行为价值的测试。

## 3. 稳定行为单元

行为—测试矩阵以以下十二个稳定行为单元为主轴：

1. Usage Published Language 与敏感字段边界。
2. Session 分区与 AppendLog 单行 framing。
3. append、flush、reopen 可见性与 no-follow 路径安全。
4. bounded sender 的非阻塞、QueueFull 与 WorkerUnavailable。
5. worker 顺序、单条失败终结、指标一致性与 drain。
6. query 全字段过滤和半开时间范围。
7. 版本化 cursor、跨分区续页和 cursor 失效。
8. JSON 损坏、未知 schema、截断尾行与 storage error 隔离。
9. token summary 及 Cost/Price 禁入。
10. Runtime logical invocation 到 UsageSink 的事实构造和失败不影响 Run。
11. Composition 的 session worker、Main/Sub 共享 sink 和 frontend shutdown。
12. legacy Cost surface 的生产不可达与 Guard 防回流。

矩阵每行必须记录：行为/风险、必要层级、现有测试路径、缺口、处理方式、最终证据及不适用层级理由。

## 4. 分层策略

### 4.1 L0：编译与结构

运行并记录：

- production-only build/check/clippy；
- all-targets clippy；
- architecture guards；
- public surface 与 test-only API guard；
- 与 Audit 依赖链有关的 feature/platform 检查；
- legacy Cost surface 防回流检查。

覆盖率与 production reachability 分开判断，测试引用不得用于掩盖生产 dead code。

### 4.2 L1：局部行为

覆盖值对象和纯策略，包括：

- worker 配置的零值回退与容量下界；
- range 合法性与半开边界；
- cursor 编解码、错误输入和 query fingerprint；
- V1 decoder 的终结行、未知版本和坏 JSON；
- filter 与 token summary 的边界。

L1 测试与源码分离到 owning module 的同级 `*_tests.rs`，不在 crate 根契约测试中间接替代局部边界。

### 4.3 L2：模块协作

使用 owning-layer Fake/Spy 验证：

- worker 与 `UsageAppendStorePort` 的 append→flush 编排；
- append/flush 失败后的单条终结、计数和后续 drain；
- query service 与 store port 的 list/read/error 映射；
- Runtime invocation 与 Recording UsageSink 的字段完整性和 Dropped 不改变调用结果。

异步推进使用通知、barrier、受控 port 或暂停时间，不以短 `sleep` 和重跑证明行为。

### 4.4 L3：契约

冻结并复用：

- Usage PL serde 与敏感字段禁入契约；
- File AppendLog adapter 契约；
- Runtime-owned UsageSink 与 Composition bridge 契约；
- ConfigSnapshot 到 `UsageWorkerConfig` 的冻结值契约。

相同契约断言只定义一次，通过 factory 或 fixture 复用。

### 4.5 L4：场景

至少验证两条组合旅程：

1. `UsageSender → worker → File AppendLog → UsageQueryPort`：事实成功落盘、可过滤查询并汇总。
2. `Runtime logical invocation → Composition bridge → session-scoped worker → canonical Session partition → shutdown drain`：Main/Sub 共享 sink，字段不丢失，Audit 失败或 drain outcome 不覆盖 Run 结果。

L4 只证明组合正确，不替代每个相邻边界的 L1～L3 证据。

### 4.6 L5：系统 Smoke

Audit MVP 不依赖真实网络、PTY、安装资产或独有平台进程语义；真实文件系统行为已由 File AppendLog adapter 契约和 L4 临时目录场景覆盖。因此预计不新增 L5，并在矩阵中明确“不适用”的理由。若审查发现只有真实 CLI 进程才能覆盖的 shutdown 缺口，再以最小稳定 smoke 补充。

## 5. 缺口处理

### 5.1 测试缺口

按 TDD 先建立失败证据，再补最小充分测试。测试必须断言业务结果和冲突结果不存在，失败消息包含关键上下文。

### 5.2 实现缺口

若生产行为违反已批准 Audit 设计，在 #1065 范围内进行根因级修正，不新增业务能力。修正前保留失败测试，修正后运行相邻层和组合层验证。

### 5.3 文档错误

将 Current/Target、实际生产可达性、测试矩阵、覆盖率信号和验收结论回写 Audit 设计或 Migration Governance。文档不得以目标描述冒充已接线能力。

### 5.4 超范围业务问题

先记录阻断、影响和建议 owner；只有取得用户同意后才创建或拆分原生 sub-issue，并设置正确依赖。不得用适配错误现状的测试固化业务错误。

## 6. 确定性与组织

1. 时间、ID、路径和外部端口使用固定值或注入值。
2. 文件测试使用每测试唯一临时目录，不修改进程全局 cwd。
3. worker 并发测试使用事件驱动同步，不依赖毫秒级墙钟差。
4. fixture/Fake 归其真实 owning layer，不建立跨层万能 `test_utils`。
5. 不新增 `mod.rs`、内联测试或 `include!` 拼接。
6. 测试名称表达“行为 + 条件 + 结果”。

## 7. 验证门禁

按由窄到宽的顺序执行，并保留首次结果：

1. Audit、Runtime、Composition、CLI 相关定向测试。
2. Audit crate production build/check 与 all-targets clippy。
3. 直接关联 crate 的 production reachability 和 all-targets clippy。
4. `cargo fmt --check`。
5. architecture guards、public surface、test-only API 和 legacy Cost guard。
6. workspace tests 与 workspace clippy。
7. 适用的 P0/L4/L5 验证。
8. `cargo llvm-cov`/仓库 coverage gate，记录 Audit 和直接关联 crate 的 line、region、function 及 changed-lines 信号。

首次失败不得被重跑成功覆盖；flaky 必须修复或登记为阻断。

## 8. Issue 与 PR 交付

1. 独立分支和 worktree 承载全部修改。
2. PR 正文包含矩阵摘要、测试缺口及处理、验证证据、coverage 信号和剩余风险。
3. PR 使用 `Closes #1065` 与 `Refs #857`，不自动关闭父项。
4. #1065 回写完整矩阵、命令结果、首次失败和最终结论。
5. #857 正文同步父项级测试审查结论；只有关键行为全部闭合后才可完成父项。
6. 所有验证通过且 PR 合并后，再按父项治理流程关闭 #857。

## 9. 完成判定

以下条件同时满足才可宣告 #1065 完成：

- 十二个行为单元均有可追溯证据或明确的不适用理由；
- L0～L5 无未解释空白；
- 测试、实现、文档和过期测试问题均已分类处理；
- 所有关键行为缺口已补齐，或存在有 owner 且依赖正确的阻断项；
- format、production reachability、all-targets clippy、architecture guards、workspace/定向测试、适用场景和 coverage gate 全部通过；
- 设计文档、#1065、#857 与 PR 中的结论一致。
