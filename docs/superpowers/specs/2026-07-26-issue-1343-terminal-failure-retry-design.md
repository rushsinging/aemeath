# 模型空终态与流中断统一重试设计

> 对应 Issue：[#1343](https://github.com/rushsinging/aemeath/issues/1343)
> Milestone：`v0.1.0 — Context Engineering + 架构重构`

## 1. 问题

模型调用存在两条会意外终止当前 Run 的路径：

1. OpenAI-compatible 流已经向 TUI 输出部分内容，随后 HTTP chunk body 被上游或代理
   截断，例如 `unexpected EOF during chunk size line`。Provider 已将该传输错误发布为
   `retryable StreamTruncated`，但 Runtime 的重试策略只要看到已提交的可见 delta 就直接
   失败，因此不会发起下一次模型调用。
2. Provider 正常发布 `InvocationEvent::Completed`，但 completion 既没有非空助手文本，
   也没有 tool call。Runtime 当前仍把它组装为 `ModelStep::Complete { text: "" }`，只记录
   `empty_terminal_text` 警告，随后把 Run 当作成功结束。

两者的共同点是：当前 attempt 没有产生可安全接受的模型终态，但现有调用级重试机制没有
统一接管。

## 2. 设计目标

1. `StreamTruncated`、网络错误等既有可重试失败即使已经输出可见 delta，也继续走模型
   调用重试。
2. 空终态在成为成功的 `ModelStep` 之前被转成调用级可重试失败。
3. Main 与 Sub 共用同一套判断、退避、取消和重试事件语义。
4. 沿用当前一次初始调用加最多十次重试、10 秒起步的指数退避和 120 秒上限。
5. 重试耗尽后发布明确失败，绝不把空终态静默标记为完成。
6. 本次不撤回、不替换已经投影到 TUI 的部分输出；重试后可能出现重复内容，这是已接受的
   临时权衡。

## 3. 方案选择

采用 Runtime 调用级统一失败方案：Provider 负责发布结构化传输错误，Runtime 在共享的
invocation reducer/coordinator 边界验证 completion，并由现有 `RetryPolicy` 决定是否
重试。

未采用以下方案：

- 不在 shared loop engine 收到 `ModelStep::Complete` 后另起重试循环。该方案会形成两套
  attempt、退避、取消和重试事件状态。
- 不由各 Provider 自行重试。Provider 能识别传输中断，却无法一致判断 Main/Sub 所需的
  语义空终态，也会绕过 Runtime 已有的 attempt 管理。

## 4. 架构与数据流

### 4.1 传输中断

Provider decoder 继续把 HTTP body/连接读取中断映射为：

```text
InvocationEvent::Failed(ProviderError {
    kind: StreamTruncated,
    retryable: true,
    ...
})
```

协议内容本身非法仍保持不可重试的 `Protocol`，不通过错误字符串做模糊匹配。现有 chunked
body 半途 EOF 契约测试必须覆盖用户报告的 `unexpected EOF during chunk size line`
路径；若该结构化契约已经成立，则不额外改动 Provider 生产映射。

### 4.2 空终态

共享 `InvocationEventReducer` 在接受 `InvocationEvent::Completed` 时先验证 completion：

- 至少一个非空白 `Text` block：有效；
- 至少一个 `ToolCall` block：有效，即使没有文本；
- 只有空白文本、没有 content，或只有 `Thinking` block：空终态。

空终态转换成 `retryable Protocol` 错误，安全错误信息明确说明 Provider 在没有助手文本或
tool call 的情况下完成。它通过 `ModelInvocationCoordinator` 返回，不能继续组装成功的
`InvocationResponse`，因此 Main 与 Sub 不会生成空的 `ModelStep::Complete`。

这里复用 `ProviderError` 作为 Runtime 与 coordinator 之间已有的调用失败语言，不新增第二套
Runtime retry error，也不为了单一语义新增公开错误枚举。`retryable=true` 是重试资格，
`Protocol` 则保留该终态违反调用协议的诊断含义。

### 4.3 Runtime 重试策略

`RetryPolicy` 的资格判断不再把 `visible_delta` 作为否决条件。是否重试仅由结构化错误、
`retryable` 标志、当前 attempt 和既有特殊分支决定：

- `ContextTooLong`：仍进入 compact；
- `RateLimited`：保持当前立即失败行为，本 Issue 不改变 429 策略；
- `retryable=false`：立即失败；
- 达到第 11 次 attempt：返回最后一次真实错误；
- 其他 `retryable=true`：按现有退避计划重试。

可见 delta 状态可以继续作为诊断信息保留，但不得参与重试资格判断。Main 在重试前关闭当前
active streaming block，并发布既有 `ModelInvocationRetrying` 事件。Sub 使用同一 coordinator；
其无显示 sink 的现有行为不变。

每次 retry 都重新创建 request。同一 Run Step、消息上下文、工具 schema 和取消 token 保持
不变；失败 attempt 的空 completion 不写入消息历史或 usage tracker。现有调用链尚未把
`ModelInvocationUsageContext` 接入 Main/Sub attempt，本 Issue 不扩展 usage identity 架构。

### 4.4 TUI 可见行为

已发出的 Text/Thinking/Tool delta 保留在现有会话投影中。Runtime 发布重试状态后重新请求，
新 attempt 的 delta 继续追加。当前版本不识别或去重前后两个 attempt 的重复片段，也不修改
SDK/TUI 事件协议。

这意味着用户可能短暂看到：

```text
部分回答
[模型调用重试状态]
部分回答（再次输出）……完整回答
```

撤回、替换和 attempt-aware 渲染属于后续独立设计，不纳入 #1343。

## 5. 错误与取消语义

- 重试退避期间收到取消：立即返回 `Cancelled`，不得等待退避结束。
- 流缺少 terminal event：保持现有 retryable `StreamTruncated`。
- 空终态连续出现：每次均占用一个 attempt，并发出与其他模型重试一致的状态。
- 第 11 次仍为空或截断：Run 以最后一次错误失败，SDK/TUI 收到现有失败终态。
- completion 有 tool call 但没有正文：正常进入 Tools step，不视为空。
- completion 只有 thinking：视为空；thinking 不能替代对用户可消费的终态或工具动作。
- 不可重试认证、权限、配置及协议解析错误：保持立即失败。

## 6. 测试策略

遵循 TDD，先修改或新增失败测试，再改生产逻辑。跨层链路逐层覆盖：

### 6.1 Provider 层

- 保留并强化 OpenAI-compatible chunked body 半途 EOF 契约：terminal 必须是
  `Failed(StreamTruncated, retryable=true)`。
- 确认非法 SSE/JSON 内容仍是不可重试 `Protocol`，防止扩大重试范围。

### 6.2 Runtime coordinator/policy 层

- `visible_delta=true` 加 retryable `StreamTruncated/Network` 返回 `RetryAfter`。
- `visible_delta=true` 加 fatal error 仍返回 `Fail`。
- partial delta 后 stream 截断，coordinator 执行第二次 attempt 并成功。
- 达到 attempt 上限后不启动第 12 次调用，并返回最后错误。

### 6.3 Runtime reducer 与 Main/Sub adapter 层

- reducer 将空 content、空白文本和 thinking-only completion 转成 retryable failure。
- reducer 接受非空文本及 tool-call-only completion。
- Main：第一次输出 partial delta 后截断，保留已投影内容、发出 retry event，第二次成功。
- Main：第一次空 completion，第二次成功；连续空 completion 耗尽后失败。
- Sub：覆盖同样的空 completion 成功重试与耗尽失败，确保没有 Main 专属旁路。
- 失败 attempt 不提交成功响应或 usage 状态。

### 6.4 SDK/TUI 层

既有 `ModelInvocationRetrying` 与 Run failure 投影协议不变。补场景测试确认部分输出后出现重试
状态且后续文本继续追加；明确断言不发生撤回或替换。

## 7. 验证门禁

- Runtime 与 Provider 相关定向测试；
- `cargo test -p provider`；
- `cargo test -p runtime`；
- 若触及 Composition 映射，运行 `cargo test -p composition`；
- `cargo check -p cli` 及相关 TUI 场景测试；
- `cargo fmt --check`；
- 受影响 crate 的 `cargo clippy -- -D warnings`；
- workspace 测试及架构守卫。

## 8. 非目标

- 不撤回、覆盖、替换或去重已显示的流式内容；
- 不修改重试次数、退避时长、jitter 或 RateLimit 策略；
- 不新增 Provider-specific retry loop；
- 不修改 Session 落盘格式、消息 schema 或 tool call 协议；
- 不处理与 #1343 无关的 Runtime 重构。
