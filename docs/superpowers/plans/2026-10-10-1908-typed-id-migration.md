# #1908 Typed ID 全面迁移（单 PR）

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/1908

## 拍板

- 写新读旧；`new_v7()` → `new_typed_id`
- ChatRunId / RunId 同前缀 `run`
- `from_legacy_or_new`：合法 typed/uuidv7 保留；非法串**原样保留**（退役确定性 uuidv7，保持映射稳定）
- TUI 本地 ids 的 `new_v7` 改 typed；`parse_uuid7` 调用改 `parse`
- 单 PR

## 前缀

| 类型 | 前缀 |
|---|---|
| ChatId | cht |
| ChatRunId / RunId | run |
| SessionId | ses |
| RunStepId | stp |
| ModelInvocationId | inv |
| AgentId | agt |
| InteractionRequestId | irq |
| ToolCallId | tcl |
| InputId | inp |
| MemoryId | mem |
| ActivityId | act |
| BackgroundProcessId | bgp（已有） |

## 任务

1. share::ids 宏化 String newtype + ids_tests + specs 前缀表
2. ActivityId（sdk）对齐 typed；MemoryId 迁 typed
3. 旁路 `Uuid::now_v7` 清零（runtime request_id 等）
4. TUI：`new_v7`→typed；executor/resumed_history `parse`
5. 修编译/测试；开 PR
