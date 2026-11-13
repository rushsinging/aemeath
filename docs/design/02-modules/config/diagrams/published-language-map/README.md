# Config 根面 Published Language 设计图

`index.html` 是自包含交互设计图（浏览器直接打开）。点击四个领域节点之一，
右侧面板先展示该分类的完整链路（groupChains），再列出该组每个根导出
符号的：类型、职责、定义签名与位置、字段、方法、消费者模块，以及消费
者的调用源码（▶ 标出 AST 命中的那一行）。

`#1866`（Connect 域收编）收口后的终态视图：根面 26 符号、四分功能子域。

## 分组

| 节点 | 内容 |
|---|---|
| 配置变更通知 | `ConfigUpdateData`（命令）、`ConfigChangeData`/`ConfigChangeCauseData`/`ConfigSubscriptionData`/`ConfigRefreshOutcomeData`（事件）、`ConfigFieldData`/`ProjectConfigLocationData`/`PreparedConfigUpdateData`/`PreparedProjectConfigData`（值对象） |
| 全局配置与 Connect | `GlobalConfigDocument`/`GlobalConfigRevision`/`GlobalConfigConnectStore`/`BootstrapConfigReceipt`/`CliConfigInputData`/`is_persist_conflict` + `wire_connect`/`wire_global_connect_store` |
| Provider 运行时解析 | `resolve_provider_runtime` / `_for_selection`（endpoint 优先级 + UA 回退） |
| 装配入口 | `ConfigWiring`/`ConfigAppService`/`NativeConfigStore` + 3 个 `wire_project_config*` + `wire_config_override_store` |

## 数据来源

- 符号清单：`agent/features/config/src/lib.rs` 根导出面（26，与
  `facade.config.root-exports` 白名单 exact-match）。
- 消费者与调用位置：`xtask pl-collect`（syn 解析全仓 `*.rs`），
  `use` 树整棵跳过，排除 config crate 自身；`is_test` 标记区分生产/测试。
- 职责（duty）、分组、groupChains 为人工编辑内容（#1866 对账核实）。

## 实现权与测试替身口径（2026-11-02）

不 sealed、不逐条登记：定义方只提供 trait + 生产 adapter；跨 crate 测试替身统一在
`agent/testing/testkit` 维护（消费方经 `[dev-dependencies] testkit` 引用），各 crate
`src/test_support.rs` 通道由守卫 `pattern.pl.fake-impl-location` 禁止；生产实现方向由
`dependency.workspace-matrix` 管。详见 `06-ports-and-adapters.md §2.4`。

## 重新生成

```bash
cargo run -p xtask -- pl-collect agent/features/config . > /tmp/config-payload.json
# 人工补 duty / groups / nodeGroup / groupChains 后落为 symbol-data.json
python3 tools/diagram/build_diagram.py docs/design/02-modules/config/diagrams/published-language-map
```

`symbol-data.json` 是生成 `index.html` 时使用的完整数据快照，可用于对照或复现。
