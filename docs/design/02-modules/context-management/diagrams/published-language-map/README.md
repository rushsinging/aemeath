# Context 根面 Published Language 设计图

`index.html` 是自包含交互设计图（浏览器直接打开）。点击六个领域节点之一，
右侧面板列出该组每个根导出符号的：类型、定义签名与位置、字段、方法、
消费者模块，以及消费者的调用源码（▶ 标出 AST 命中的那一行）。

## 分组

| 节点 | 内容 |
|---|---|
| Session | 会话聚合根、快照/恢复读模型、`SessionManagementPort` 与管理实现 |
| Window·Budget | `ContextPort` 四拆端口、请求/窗口载荷、token 预算与估算、上下文工厂 |
| Compact | 压缩单一入口、决策与进度载荷、`CompactLlmBinding`、深路径结果类型 |
| Prompt·Guidance | `Language` 枚举、guidance 加载与安全评估、`GuidanceResolution` 加载事实 |
| 注入 | Reminder 策略与载荷、memory 召回注入 |
| 回写链 | step/append/tool 回执、幂等写实现、清理确认 |

## 数据来源

- 符号清单：`agent/features/context/src/lib.rs` 的 `pub use` 根导出面。
- 消费者与调用位置：`syn` 解析全仓 `*.rs`，只认 `ExprCall`（调用）、`ExprStruct`（构造）、
  `TypePath`（类型引用）节点；`use`/`pub use` 整棵跳过，因此 re-export 不会被误算为消费。

## 重新生成

```bash
# 1. 渲染底图（需要 archify skill）
node <archify>/bin/archify.mjs render architecture \
  docs/design/04-architecture/context-pl-map/spec.architecture.json \
  /tmp/base.html

# 2. 采集符号数据后，把 payload 注入 inject-panel.js 的 __PL_DATA__ 占位
#    （payload = { groups, nodeGroup, symbols }，见 symbol-data.json 结构）

# 3. 把 inject-panel.js 以 <script> 追加到 base.html 的 </body> 前，输出 index.html
```

`symbol-data.json` 是生成 `index.html` 时使用的完整数据快照，可用于对照或复现。
