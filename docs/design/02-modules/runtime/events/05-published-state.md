# 05 · Published State 事件

## 1. 定位

| 属性 | 契约 |
|---|---|
| Owner | Runtime `PublishedStateRegistry` 负责交付；各业务 owner 提供 committed fact |
| Authority | full-state delivery authority；NEVER 驱动 Lifecycle terminal 或业务决策 |
| Identity | `session_id` + family-local revision |
| Ordering | revision 单调；heartbeat sequence 只表示活性 |
| Delivery | revisioned full-state replacement + heartbeat resend |

Published State 解决“消费者如何获得当前完整状态”，不是另一套领域状态机。

## 2. State families

| Family | SDK event | DTO | Producer fact | TUI consumer | 状态 |
|---|---|---|---|---|---|
| Runtime Status | `RuntimeStatusChanged` | `RuntimeStatusView` | Context decision 与 Runtime presentation facts | `ReplaceRuntimeStatus` | Current |
| Task State | `TaskStateChanged` | `TaskStateView` | typed committed Task change | `ReplaceTaskState` | Current |
| Legacy Task display | `TasksSnapshot` | text lines | 旧展示投影 | `UpdateTaskLines` | Target Removal |

各 family 的 revision epoch 独立，禁止 Runtime Status revision 与 Task revision 相互比较。

## 3. Registry contract

1. Registry 维护 session-scoped、跨 Run 复用的 family full-state；
2. accepted business fact 更新状态并增加该 family business revision；
3. unchanged fact SHOULD dirty-deduplicate，不伪造新业务版本；
4. heartbeat 从 Registry 读取已有完整状态并重发；
5. heartbeat 只增加 `heartbeat_sequence`，NEVER 增加 business revision；
6. reset/session switch 建立独立 revision epoch；
7. Registry 不调用 Provider、Config reader 或其他 BC 临时拼装状态；
8. sink 失败不改变 Lifecycle terminal。

## 4. Runtime Status

Runtime Status 至少明确区分：

- Context budget：context size、effective window、decision token count、threshold、utilization、decision source；
- Runtime presentation：active identity、processing/waiting/cancelling 等；
- current/pending config；
- workspace/worktree；
- operation summary（若纳入 full-state）。

Context budget 事实来自 Context owner；Runtime 只观察、注册并发布。TUI context percentage MUST 读取 Runtime 提供的 utilization，NEVER 使用 `last_input_tokens / context_size` 重建 compact policy。

## 5. Task committed publication

```text
Tool execution returns typed committed fact
  → CommittedSideEffectDispatcher
  → Task capability handler
  → query committed Task full state at matching revision
  → Published State update/publish
  → TaskStateChanged
```

禁止的 authority：

- Tool 名称；
- success 文本；
- Tool Result materialization；
- round 完成；
- Activity；
- 对 JSON result 的推断。

Task mutation lane 必须保证 commit N → query N → publish N 在释放串行边界前完成；revision mismatch 是错误证据，不是正常丢弃策略。

## 6. TUI replacement

TUI 对同一 session/family：

1. 更高 revision 原子替换完整状态；
2. 更低 revision 丢弃；
3. 相同 revision + 更高 heartbeat sequence 更新活性；
4. duplicate 幂等；
5. 不 merge 字段、不跨 session 复用 revision；
6. assembler 只读 snapshot，不重新访问配置或业务 service。

## 7. 与 Activity/Lifecycle 的边界

- Published State 不驱动 Run terminal；
- Activity 可展示 Compact operation 进度，但不替代 Runtime Status；
- Task State 是 committed aggregate view，不是 Tool Activity；
- Context decision 可更新 budget，但 status consumer 不反向触发 compact；
- heartbeat 是 delivery/liveness，不是业务 Activity。

## 8. 不变量

1. 每个 family 有独立 identity/revision；
2. full-state payload 字段必须无损贯通 SDK/TUI；
3. consumer 不能靠缺省字段保留旧值；
4. heartbeat 不增加业务 revision；
5. committed metadata 不进入 SDK/Session/LLM wire；
6. Published State 不是 Lifecycle、Activity 或 Domain State Machine 的复制品。

## 9. 变更门禁

修改 Published State 必须覆盖 Registry update/read、business revision、heartbeat、reset/session isolation、SDK mapping/schema、TUI stale/duplicate replacement、assembler/render 相邻测试，并更新 [09-event-index.md](09-event-index.md)。
