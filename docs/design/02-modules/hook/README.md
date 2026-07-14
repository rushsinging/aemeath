# Hook（通用域）

> 层级：02-modules / hook（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#790（S2）
> Hook 拥有用户配置驱动的脚本匹配、执行与输出协议；Runtime 拥有触发时机和控制流解释。

## 1. 模块定位

Hook 是可插拔生命周期扩展机制，不是 Policy、Workflow 或第二个 Run 状态机。

```text
业务 BC / Runtime 到达 HookPoint
  → HookPort.dispatch(HookInvocation)
  → HookOutcome
  → 调用方解释 directive 并推进自己的聚合
```

本期采用一个类型化 HookPort。Boundary / Tool / Notification 是 HookPoint 元数据，不拆成多个端口；未来只有出现不同沙箱、一致性、并发或安全策略时才重新评估。

## 2. 单一端口与类型化输入

```rust
trait HookPort: Send + Sync {
    async fn dispatch(
        &self,
        invocation: HookInvocation,
        cancellation: &dyn CancellationSignal,
    ) -> HookOutcome;
}

enum HookInvocation {
    PreToolUse(PreToolUseInput),
    PostToolUse(PostToolUseInput),
    PostToolUseFailure(PostToolUseFailureInput),
    UserPromptSubmit(UserPromptInput),
    Stop(StopInput),
    StopFailure(StopFailureInput),
    SessionStart(SessionInput),
    SessionEnd(SessionInput),
    PreCompact(PreCompactInput),
    PostCompact(PostCompactInput),
    PostToolBatch(PostToolBatchInput),
    SubRunStart(SubRunInput),
    SubRunStop(SubRunInput),
    TaskCreated(TaskInput),
    TaskCompleted(TaskInput),
    PermissionRequest(PermissionInput),
    PermissionDenied(PermissionInput),
    Notification(NotificationInput),
    InstructionsLoaded(InstructionsInput),
    ConfigChange(ConfigChangeInput),
    Elicitation(ElicitationInput),
    ElicitationResult(ElicitationResultInput),
    UserPromptExpansion(UserPromptExpansionInput),
    CwdChanged(CwdChangedInput),
    FileChanged(FileChangedInput),
    TeammateIdle(TeammateIdleInput),
}
```

使用 enum 绑定 HookPoint 与 payload，禁止 `point + 无约束 JSON` 形成非法组合。对外统一语言使用 `SubRun`；adapter 可兼容 Claude Code 的 `SubagentStart/Stop` 名称。

## 3. HookPoint 元数据

```rust
struct HookPointMetadata {
    class: HookClass,
    can_block: bool,
    can_modify_input: bool,
    can_add_context: bool,
    failure_policy_configurable: bool,
}

enum HookClass {
    Boundary,
    Tool,
    Notification,
}
```

元数据由系统拥有，用户配置不直接声明 class。

| 类别 | 典型 HookPoint | 能力 |
|---|---|---|
| 前置闸口 | PreToolUse、UserPromptSubmit、PreCompact、PermissionRequest、Elicitation、UserPromptExpansion | 可 Block；按 point 元数据决定是否可改 input |
| Stop 闸口 | Stop | 只允许 Continue / Block；阻止 Finishing → Completed |
| 后置增强 | PostToolUse、PostToolUseFailure、PostCompact、PostToolBatch、ElicitationResult | 追加 context / 通知，不撤销已发生行为 |
| 生命周期 | SessionStart/End、SubRunStart/Stop、TaskCreated/Completed | 追加 context、清理或通知；生命周期归调用方 |
| 观察 | StopFailure、PermissionDenied、ConfigChange、CwdChanged、FileChanged、TeammateIdle | 只记录或通知 |

### HookPoint 能力矩阵

| HookPoint | Block | UpdatedInput | AdditionalContext |
|---|---:|---:|---:|
| PreToolUse | ✅ | ✅ Tool input | ✅ |
| UserPromptSubmit | ✅ | ✅ Prompt | ✅ |
| PreCompact | ✅ | ❌ | ✅ |
| PermissionRequest | ✅ | ✅ approval request | ✅ |
| Elicitation | ✅ | ✅ elicitation request | ✅ |
| UserPromptExpansion | ✅ | ✅ expanded prompt | ✅ |
| Stop | ✅ | ❌ | ❌ |
| SessionStart / SubRunStart | ❌ | ❌ | ✅ |
| PostToolUse / PostToolUseFailure / PostCompact / PostToolBatch / ElicitationResult | ❌ | ❌ | ✅ |
| SessionEnd / SubRunStop / TaskCreated / TaskCompleted / Notification / InstructionsLoaded | ❌ | ❌ | 由 point metadata 固定声明 |
| StopFailure / PermissionDenied / ConfigChange / CwdChanged / FileChanged / TeammateIdle | ❌ | ❌ | ❌ |

Hook adapter 必须依据该矩阵校验 HookDirective：

- can_block=false 收到 Block → 协议错误，进入 ExecutionFailed；
- can_modify_input=false 收到 UpdatedInput → 协议错误；
- can_add_context=false 收到 Context → 协议错误；
- Stop 收到任何 ContinueWith* → 协议错误；
- updated input 返回调用方后必须重新执行 schema/Policy 校验。

## 4. 用户配置

```rust
struct HookSubscription {
    point: HookPoint,
    matcher: HookMatcher,
    command: HookCommand,
    timeout: Duration,
    failure_policy: Option<HookFailurePolicy>,
    order: i32,
    enabled: bool,
}

enum HookFailurePolicy {
    Continue,
    Block,
}
```

- 配置按 `order` 与声明顺序稳定执行；
- 只有 metadata 允许时才能配置 failure_policy=Block；
- 普通 Hook 未配置时默认 Continue；
- Stop Hook 执行失败的系统语义固定为 Block；用户不能改成 Continue；
- 后置、观察及已结束生命周期事件不允许配置 Block；
- 非法组合在 Config 校验阶段拒绝，而非运行时静默忽略。

## 5. 执行协议与 Outcome

```rust
struct HookOutcome {
    executions: Vec<HookExecution>,
    directive: HookDirective,
}

enum HookDirective {
    Continue,
    Block { reason: HookReason },
    ContinueWithContext { context: String },
    ContinueWithUpdatedInput { input: JsonValue },
    ContinueWithContextAndInput { context: String, input: JsonValue },
}

struct HookExecution {
    status: HookExecutionStatus,
    attempts: u8,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    duration: Duration,
}
```

协议：

| 结果 | 语义 |
|---|---|
| exit 0 + 合法 JSON/空输出 | 成功，解析 directive |
| JSON `decision: block` / `continue: false` | 主动 Block（exit 0 时仍可通过 JSON 声明） |
| 任意非零退出码（exit 1/2/...） | 主动 Block |
| spawn / wait / IO / timeout | ExecutionFailed |
| exit 0 + 非法 JSON | ExecutionFailed |

> **设计决策**：任意非零退出码 = Block，而非要求用户用 exit 2 表示 block。原因：
> 1. **Unix 惯例**：非零退出码 = 失败/阻止，用户写 hook 脚本时自然用 `exit 1` 表示拒绝
> 2. **不增加认知负担**：要求用户记住"exit 2 才是 block"不合理——大多数 hook 脚本用 `exit 1`
> 3. **exit 0 + JSON `decision: block`** 是结构化声明 block 的方式（需要传递 reason 时使用）
>
> 主动 Block 是业务结果，不执行重试；ExecutionFailed 才进入重试。

## 6. 单 Hook 执行重试

```text
hook.max_attempts = 3
```

- 最大尝试次数为 3（含第一次）；
- 重试执行故障，不重试业务 Block；
- timeout 重试前必须终止并回收旧子进程，禁止孤儿命令继续产生副作用；
- 每次尝试的明细进入 HookExecution 或结构化诊断；
- max_attempts 的静态默认值由 ConfigSnapshot 提供，Hook BC 应用。

重试耗尽：

| HookPoint | 最终 directive |
|---|---|
| 普通 Hook | Continue，并保留 ExecutionFailed 明细 |
| Stop | Block(StopHookExecutionFailed) |

`StopFailure` 是独立观察 HookPoint：Stop subscription 在 3 次执行重试耗尽后，Hook BC 先合成 `Block(StopHookExecutionFailed)` 返回 Runtime，再尽力派发一次 StopFailure 通知。Runtime **MUST** 在收到 Block 后推进自己的状态迁移（Finishing → PreparingContext）；StopFailure 是 best-effort 观察，Runtime **NEVER** 因 StopFailure 结果改变已决定的 Block 语义。StopFailure 的结果不递归触发新的 StopFailure。

## 7. 与 Run Loop Engine 的边界

Stop Hook 是 Hook 与 Run 状态机的关键协作点，完整语义见 [01-run-loop-integration.md](01-run-loop-integration.md)。

简化规则：

- Hook BC 执行 Stop subscription、内部重试并返回 directive；
- Runtime 在 Finishing 触发 Stop；
- Continue → Completed；
- Block → feedback 注入同一 Run，回到 PreparingContext；
- Runtime 维护每个 Run 的 stop_block_count；
- 超过 15 次 → RunFailed(StopHookRetryExhausted)。

## 8. 安全与资源

- Hook command 是用户配置 shell，必须明确 workspace_root 和环境变量白名单；
- stdin 使用结构化 JSON；
- stdout/stderr 应有大小上限，超限内容交 Storage 机制处理，不直接塞入模型窗口；
- timeout 必须 kill + wait 回收子进程；
- Hook 输出的 updated input 必须由调用方重新执行 schema/Policy 校验；
- Hook BC 不决定 updated input 是否进入 Tool 或 Context Window。

## 9. 不变量

- **MUST** 一个 HookPort、一套匹配/执行/协议语义。
- **MUST** 区分主动 Block 与 ExecutionFailed。
- **MUST** Stop 执行失败重试耗尽后返回 Block。
- **MUST** 普通 Hook 失败默认 Continue。
- **MUST** Runtime 拥有触发时机和状态迁移。
- **MUST NOT** Hook 创建第二个 Run 状态机。

## 10. 多 Subscription 聚合规则

同一 HookPoint 可能有多个 subscription。聚合规则：

| 维度 | 规则 |
|---|---|
| **执行顺序** | 按 `order` + 声明顺序串行执行；前一个的 `UpdatedInput` 作为下一个的输入 |
| **Block 短路** | 任一 subscription 返回 Block → 立即停止后续 subscription，整体 directive = Block |
| **Context 合并** | 多个 `ContinueWithContext` 的 context 字符串按顺序拼接 |
| **UpdatedInput 串联** | subscription B 的 input = subscription A 的 UpdatedInput；无 UpdatedInput 时保持原值 |
| **失败组合** | 普通 subscription ExecutionFailed 不阻塞后续（默认 Continue）；Stop subscription ExecutionFailed → Block |

## 11. 环境变量与安全

- **env_clear**：Hook 子进程 **MUST** 默认只继承白名单变量，不继承全部父进程 env；
- **白名单**：`PATH` / `HOME` / `SHELL` / `LANG` / `LC_ALL` / `TERM` / `AEMEATH_PROJECT_DIR` / `CLAUDE_PROJECT_DIR`；
- **Config 可扩展**：ConfigSnapshot.hooks 可添加额外 env 变量到白名单；
- **NEVER 泄漏密钥**：API key、token 等 **NEVER** 进入 Hook 子进程 env；
- **stdin**：结构化 JSON（含 HookPoint、input payload、session metadata），**NEVER** 包含 ConfigSnapshot 原文。
- **MUST NOT** 让用户配置非法 HookPoint 能力组合。
- **MUST NOT** timeout 后遗留未回收子进程。

## 10. 相关文档

- Run Loop 集成：[01-run-loop-integration.md](01-run-loop-integration.md)
- Runtime 状态机：[../runtime/03-loop-and-state-machine.md](../runtime/03-loop-and-state-machine.md)
- Policy：[../policy/README.md](../policy/README.md)
- BC 责任章程：[../../01-system/01-product-and-domain.md](../../01-system/01-product-and-domain.md)
- Migration：[../../03-engineering/migration-governance.md](../../03-engineering/migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：单 HookPort、类型化协议、失败策略与 3 次执行重试 | #790 |
