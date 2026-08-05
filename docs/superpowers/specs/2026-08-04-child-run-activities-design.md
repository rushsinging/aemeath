# Child Run 活动链路统一设计

> 本设计将 Main/Sub Run 实时事件与现有 Agent ToolCall activity 展示作为一个垂直交付实施，覆盖原 #612 与 #946 的交叉范围。

## 目标

Main Run 通过 Agent ToolCall 启动一个 Sub-agent Run；Sub-agent 不得继续调用 Agent。Runtime、SDK、TUI ACL 与 ConversationModel 完整传递 Child Run 的身份和结构化活动事件，并继续投影到当前已经符合预期的 Agent ToolCall activity 展示路径。用户可见行为只新增 Sub-agent Text / Thinking / ToolOutput 的可见内容，不改变现有 ToolCall、ToolResult、折叠、排序、颜色和布局。

## 根因

当前 Sub-agent 活动通过 `AgentProgressEvent` 传递，但 TUI 第二层 ACL 很快把事件格式化为字符串并写入父 Agent ToolCall 的 `activities`。这导致事件类型、Child Run 身份和父子关联在展示前被压缩；`ToolOutput` 还会返回空 mapping，造成静默丢失。Model 只有摘要文本，没有可按 Child Run identity 归属的结构化事实。

## 约束

- Main Run 可以调用 Agent；Sub-agent 的 `sub-agent/sub-agent-restricted` scope 不包含 Agent。
- 本次不实现 Sub-agent → Sub-agent 递归派发。
- Child Run 由父 Run 的 Agent ToolCall 启动，必须贯穿：`agent_id`、`run_id`、`parent_run_id`、`spawned_by_tool_call_id`。
- 父 Agent ToolResult 与 Child Run terminal 是两个独立事实，不互相覆盖或替代。
- 不新增 Child Run 根级视觉块，不把 Sub-agent Text / Thinking 提升为主对话根块。
- 结构化 Child Run 事件只能有一个事实来源；旧 `AgentProgress` 只能作为兼容入口或单向展示投影，不能形成第二状态源。

## 统一数据链

```text
Main Run
└── Agent ToolCall
    └── Child Run
        ├── Text
        ├── Thinking
        ├── 普通 ToolCall
        ├── ToolResult
        └── Terminal

Runtime Child Run Event
  → SDK Published Language
  → TUI-owned ACL DTO
  → ConversationModel Child Run facts
  → 当前 Agent ToolCall activities
  → 现有 Render
```

结构化事件至少保留事件 kind、Child Run identity、单调序号/版本和对应 payload。Text、Thinking、普通 ToolCall、ToolResult、terminal 不在 Runtime 或第一层 ACL 被压成不可区分的字符串。

## 展示行为

现有展示继续保持：

```text
● Agent reviewer
  ⟳ analysing codebase
  ⟳ → Read src/main.rs
  ⟳ → Grep provider
  ⎿ Found relevant files
```

结构化事实按 `spawned_by_tool_call_id` 找到父 Agent ToolCall 后，派生到现有 activity 行：

- Sub-agent Text 进入普通 activity 内容；
- Sub-agent Thinking 进入同一 activity 路径并保留 thinking 语义；
- Sub-agent ToolCall 继续使用现有 `→ ToolName ...` 格式；
- Sub-agent ToolOutput 不再静默丢弃，按现有摘要规则进入对应 Agent activity；
- 父 Agent ToolResult 继续走现有 ToolResult 展示；
- Child terminal 保持独立事实，在当前兼容展示边界内不覆盖父 ToolResult。

## 分层职责

### Runtime / Tools

Runtime 在 Agent ToolCall 执行边界拥有父 Run、父 ToolCall 和派生 Sub Run 的关系；发布结构化 Child Run 活动。Tools 的 Sub-agent scope 继续排除 Agent，普通工具事件必须归属于 Child Run，而不能伪装为 Main Run 活动。

### SDK

SDK 发布可序列化、类型明确的 Child Run identity 与事件 Published Language，并为历史字段提供明确的兼容反序列化策略。SDK 不承载 TUI 展示字符串。

### TUI ACL

第一层 ACL 只做 SDK DTO → TUI-owned DTO 的无损转换；第二层 ACL 只做 DTO → Intent 的语义映射。两层都不得静默丢弃 Text、Thinking、ToolCall、ToolResult、ToolOutput 或 terminal。

### ConversationModel

Model 按 Child Run identity 归属事件，使用父 Agent ToolCall 作为当前展示宿主。并发 Child Run 必须互不串流；未知或无法归属的事件进入带完整 identity 的诊断路径。Model 不依赖 SDK 类型、Runtime 句柄或 sender。

### View / Render

ViewAssembler、ViewModel 和 Render 继续消费现有 Agent ToolCall activity 数据，不建设第二套 Child Run 视觉管线。只有当现有结构无法表达 Text/Thinking 语义时，才扩展现有 activity view 的字段，不改变布局契约。

## 错误与乱序

- 事件必须使用 Child Run identity 路由，不能根据当前 active turn、工具名称或到达顺序猜测归属。
- 父 ToolCall 尚未进入 Model 时到达的 Child Run 事件不得静默丢弃；应保留为带 identity 的 orphan/待绑定事实，后续可在父关系建立后归位。
- 父 ToolResult 到达不能清除 Child Run 的历史事件；Child terminal 到达不能伪造或覆盖父 ToolResult。
- 重复事件应按 identity + sequence/revision 幂等处理；旧事件不得回滚已接收的新事实。
- 不允许把“Sub-agent 不调用 Agent”的工具权限约束仅放在 TUI；Tools scope 与 Runtime 装配测试必须共同证明。

## 测试策略

按相邻层补齐证据：

1. Tools：Main scope 包含 Agent，Sub-agent scope 不包含 Agent。
2. Runtime：Child Run identity 完整；Text、Thinking、普通 ToolCall、ToolResult、terminal 事件类型和父 ToolCall 关联完整；父 ToolResult 与 Child terminal 独立。
3. SDK：结构化事件序列化/反序列化保留 identity、kind 和 payload。
4. TUI 第一层 ACL：SDK → TUI-owned DTO 字段无损。
5. TUI 第二层 ACL：每种 Child Run 事件产生显式 Intent，ToolOutput 不再产生空 mapping。
6. ConversationModel：两个并发 Child Run 分别进入对应父 Agent ToolCall；乱序、重复、orphan 与 terminal 独立性有断言。
7. 场景：现有 ToolCall / ToolResult 展示保持不变；Sub-agent Text / Thinking / ToolOutput 可见且不串流；Sub-agent 不产生 Agent ToolCall。

测试使用固定 identity、固定序号和确定性事件输入，不使用短 sleep 证明并发或排序行为。

## 最小补丁与根因方案

最小补丁是给现有 `AgentProgress` 添加 identity 并修复 `ToolOutput` 空 mapping，成本低但仍保留字符串过早压缩和摘要作为事实源，后续仍需重做 Model。

本设计采用根因方案：在同一个垂直交付中建立唯一结构化 Child Run 事实，并从该事实派生现有 activity 展示。成本是跨 Runtime、SDK、Tools 和 TUI 多层修改与契约测试，但能消除双轨、字段丢失、并发串流和重复转换，且不改变已经认可的 UI。
