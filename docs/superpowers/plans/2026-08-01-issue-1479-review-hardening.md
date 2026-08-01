# #1479 审查补强实施计划

## 目标

在既有 Dataset Session reader/writer 基础上，落实 Issue 审查指出的根因约束：普通 mutation 不再通过完整 `CanonicalSession` clone/encode；Session state 按职责拆分；迁移、compact、Tool receipt、GC 与性能验收分别具备独立契约；完整 Session 生产旁路退役。

## 执行顺序

1. 为普通 mutation 建立不物化完整历史的 Context 变更入口失败测试。
2. 将 Session state 拆为按职责独立的 Dataset member，并补充未变化 member 复用测试。
3. 为 legacy migration、compact 与 Tool receipt 生命周期补充独立内存和恢复契约测试。
4. 为 Dataset stage/journal/previous 共享 member 增加锁内 orphan GC 与崩溃恢复契约。
5. 建立 10 MiB、50 MiB、162 MiB Session 的写入量与内存性能基线记录入口。
6. 更新架构守卫禁止完整 Session writer 与完整编码生产旁路。
7. 运行 Context、Storage、Composition、架构守卫和 schema 检查。

## TDD 与验证

- 每个领域行为先建立失败测试，再修改生产代码。
- Context、Storage、Composition 分别验证，持久化链路补充落盘/Resume 场景。
- 最终运行 `cargo test`、`cargo check`、`cargo run -p xtask -- sdk-wire-schema check`、fast/full architecture guards，并检查 `git diff --check`。
- 若固定尺寸性能数据无法在当前环境稳定采集，将只提交确定性基线入口与采集说明，不虚构实测结果。

## 退役清单

- 普通 mutation 对完整 `CanonicalSession` 的生产 clone。
- 普通 mutation 对完整 `SessionCodec::encode` 的生产调用。
- Dataset writer 之外的完整 Session Blob 生产写入旁路。
- 未登记的 stage/journal/previous 清理路径。
