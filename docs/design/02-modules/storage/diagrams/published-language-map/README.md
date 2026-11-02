# Storage 根面 Published Language 设计图

`index.html` 是自包含交互设计图（浏览器直接打开）。点击七个领域节点之一，
右侧面板先展示该分类的完整链路（groupChains），再列出该组每个根导出
符号的：类型、职责、定义签名与位置、字段、方法、消费者模块，以及消费
者的调用源码（▶ 标出 AST 命中的那一行）。

本图同时作为 #1865（storage PL 收敛）的设计对照底图：链路中已标注
A–E 工作项涉及的病症位置。

## 分组

| 节点 | 内容（#1865 A–E 收口后 41 符号，五分） |
|---|---|
| 工厂 | `wire_file_system_blob` / `wire_file_system_dataset`（根级 pub fn，返回 `Arc<dyn 端口>`） |
| 端口 | `AtomicBlobPort`（read/write_atomic/promote_previous/quarantine/delete_all_generations/list_primary）、`AtomicDatasetPort`（read_manifest/read_consistent/read_previous/commit_atomic/commit_incremental） |
| 安全路径 | `SafePathSegmentData` 校验 + `SafeStorageRoot`/`SafeStorageDir`/`SafeOpenOptions`/`SafeStorageFileType`/`SafeStorageEntryData` 受限句柄 |
| 共享词汇 | `StorageNamespaceData`（previous_policy/minimum_durability/effective_durability 策略）、`StorageKeyData`、`DurabilityData`、`TransactionScopeData`、`GenerationData` |
| 崩溃恢复与隔离 | 恢复决策载荷（`CorruptionReason`/`QuarantineDisposition`/`CorruptTransactionError`）+ 隔离三件；#1865 A 决策单点 / C 错误语义折叠记录 |
| blob 载荷与回执 | 写入/删除选项、读/写/删/提升回执、隔离三件（Reason/Outcome/Receipt）、`CommitWarningData`、`StorageEntryData` |
| dataset 载荷与回执 | 成员三型、变更集、清单、修订指纹（`revision_member_digest` 单源）、提交回执与可见性 |

## 数据来源

- 符号清单：`agent/features/storage/src/lib.rs` 根导出面（根级 `pub fn`
  wire 工厂 + `pub use`，共 43）。
- 消费者与调用位置：`xtask pl-collect`（syn 解析全仓 `*.rs`），
  `use` 树整棵跳过，排除 storage crate 自身。
- 职责（duty）、分组、groupChains 为人工编辑内容；#1865 病症标注来自
  issue 对账表（2026-10-28 实测核验）。

## 实现权与测试替身口径（2026-11-02）

不 sealed、不逐条登记：定义方只提供 trait + 生产 adapter；跨 crate 测试替身统一在
`agent/testing/testkit` 维护（消费方经 `[dev-dependencies] testkit` 引用），各 crate
`src/test_support.rs` 通道由守卫 `pattern.pl.fake-impl-location` 禁止；生产实现方向由
`dependency.workspace-matrix` 管。详见 `06-ports-and-adapters.md §2.4`。

## 重新生成

```bash
# 1. 采集符号数据
cargo run -p xtask -- pl-collect agent/features/storage . > /tmp/storage-pl-payload.json
#    人工补 duty / groups / nodeGroup / groupChains 后落为 symbol-data.json

# 2. 构建自包含 index.html
python3 tools/diagram/build_diagram.py docs/design/02-modules/storage/diagrams/published-language-map
```

`symbol-data.json` 是生成 `index.html` 时使用的完整数据快照，可用于对照或复现。
