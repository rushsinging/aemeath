# Tool Display 优化 — TUI 如何拿到 typed tool result（DDD 边界讨论）

> 状态：草案 / 多模型讨论中（**未形成最终结论**）
> 触发：plan 2026-06-18-tool-display-structured-data 中"R struct 放 packages/global/types"方案被 `check-architecture-guards.sh` 阻断（`cli must not depend on types`）。
> 目的：把 3 个模型（glm planner / gpt reviewer / mimo coder）从 DDD / 清洁架构 / 实现成本三个角度的独立评估记录下来，供决策时回溯。
> 相关 issue：#273 (TUI tool call detail 优化) / #325 (ToolResult 扁平化重构)。

## 1. 背景

`#273` 要求 TUI 拿到 typed `tool result`（如 `ReadResult.path` / `EditResult.diff`），不再硬挖 `serde_json::Value` 字符串键。`#325` 把 `ToolResult` 扁平化：`{ok, message, data: R, images}`。

关键设计点：**29 个 typed result struct（R struct）放哪里**？
- 31 个 tool（read/write/edit/glob/grep/web_fetch/web_search/bash/sleep/agent/ask_user/enter_worktree/exit_worktree/brief/config_tool/lsp/plan_mode/memory/skill/task_create/task_get/task_list/task_stop/task_update/task_list_create/task_list_complete/tool_search/mcp_tool/mcp_manager/list_mcp_resources/read_mcp_resource）每个一个 R struct。
- R 既是 tool 业务的输出契约，也是 TUI Display 的输入数据。

**初版 plan 决策**（被守卫阻断）：
- 新建 `packages/global/types` crate，`cli` + `tools` 都直接 dep types
- 守卫错误：`cli must not depend on types; allowed: ['composition', 'sdk']` / `tools must not depend on types; allowed: ['project', 'share', 'storage']`
- plan 决策依据"TUI 不跨边界依赖 agent 业务层"不成立——架构规则更严格：cli **完全不**直接 dep `share` 或 `types`，必须经 `composition` 间接拿。

**Phase 0a 5 commit 已被撤销**（`git reset --soft HEAD~4` + 删 `packages/global/types/`），需要重新选 R struct 位置。

## 2. 架构铁律（DDD 六边形 / clean）

来源：`docs/design/02-architecture-guards.md` §依赖铁律 + `docs/design/01-outline.md`。

| 层级 | 角色 | 允许 dep | 关键约束 |
|---|---|---|---|
| `cli` (apps/cli) | 入口 / Inbound Adapter | `{composition, sdk}` | **禁** dep 任何 business crate（runtime/project/policy/prompt/provider/tools/storage/hook/audit/share） |
| `composition` | 装配根 | 全部 FEATURE_CRATES + `share` + `sdk` + `logging` | 唯一能 re-export 业务类型给 cli 的层 |
| `runtime` | 编排 | 全部 supporting + `share` + `sdk` + `logging` | 唯一能连接所有 feature 的层 |
| `share` | 共享内核 | `{logging, utils}` | **最小内核**（禁 IO/并发/时钟/业务类型膨胀），deps 白名单：`serde, serde_json, thiserror, tokio, log, logging, unicode-width, utils` |
| `sdk` | 通信契约 | `{utils}` | cli ↔ runtime 数据契约，**禁**业务类型 |
| `tools` | 业务实现 | `{share, project, storage}` | 不能 dep `composition` / `sdk` / 其他 feature |

**核心约束**：
- cli 唯一允许 dep：`composition` + `sdk`
- tools 不能直接 dep `composition` / `sdk`
- share 是"零业务类型"内核
- 数据流：`tool call() → ToolResult → runtime ChatEvent → sdk ChatEvent → cli observe → TUI Display`（wire 始终是 `serde_json::Value`）

## 3. 现有数据流（事实）

来源：plan 2026-06-18-tool-display-structured-data 第 13-78 行 + glm 模型直接源码核查。

```
tools/business/<tool>.rs           ── build ──▶   share::tool::ToolResult { content: serde_json::Value }  ① 生产
runtime/.../chat/looping/tools.rs ── send ──▶   RuntimeStreamEvent::ToolResult                          ②
runtime/.../core/client/event.rs  ── map ───▶   sdk::ChatEvent::ToolResult { content: Value }           ③ SDK 边界
cli/adapter/tool_flow_projector.rs ── sanitize ▶ Value                                                ④
cli/model/conversation/tool_flow.rs ── store ─▶ Value                                                  ⑤
cli/view_assembler/output.rs:495   ── content.get("data").get("diff")… ─▶ 字符串                     ⑥ 痛点（字符串键硬挖）
```

**关键事实**：
1. `sdk::ChatEvent::ToolResult.content` 永远是 `serde_json::Value`（wire format）。无论 R 放哪，R 必须 `(de)serialize` 穿过这条 Value 管道。
2. TUI Display 实际只读 3 种字段形状（mimo 模型源码核查 `output.rs:495-541`）：
   - `EnterWorktree/ExitWorktree`：`message` + `branch`
   - `Edit/file_edit`：`message` + `data.diff`
   - 其余全部：`display` → `message` → `text` → fallback
3. 29 个 R struct 中，**26 个的字段 TUI 当前不消费**——这些字段是为未来扩展或非 TUI 消费者（如 server、history 回放）准备。

## 4. 6 候选方案

### 方案 1：新建 `packages/global/types` crate，`cli` + `tools` 都直接 dep

- 机制：types 承载 29 个 XxxResult；tools 写 `use types::tool_result::ReadResult; type Result = ReadResult;`；cli 写 `use types::tool_result::ReadResult; serde_json::from_value(data)`。
- 守卫：❌ cli 禁 dep types（`allowed: ['composition', 'sdk']`）；❌ tools 禁 dep types（`allowed: ['project', 'share', 'storage']`）。
- DDD 视角：违反"cli 薄入口"原则；创造"无人负责的类型垃圾桶"。
- **结论**：**否决**（守卫机械失败）。

### 方案 2：R 放 `agent/shared/src/tool/result/`，composition re-export

- 机制：share 承载 29 个 XxxResult；tools 已有 share dep（不增）；composition `pub use share::tool_result::*;`；cli 通过 composition `use composition::ReadResult;`。
- 守卫：✅ 全路径通过。
- DDD 视角：⚠️ 表面合规（share deps 白名单含 serde/serde_json），但违反 share **最小内核** 原则。29 个 tool 专属业务 struct（ReadResult/GrepResult/McpToolResult）显然是业务类型，不是"数据契约/纯函数"。
- **结论**：**不推荐**（机械合规但污染内核；破坏聚合根内聚）。

### 方案 3：R 放 `tools/src/business/<tool>.rs`（与 tool 同源），composition re-export

- 机制：tool 内部 `pub struct ReadResult` + `type Result = ReadResult`；composition `pub use tools::business::read::ReadResult;`；cli 通过 composition `use composition::ReadResult;`。
- 守卫：✅ tools 已有 share/project/storage dep，零新增；composition re-export 是 `ROOT_REEXPORT_ALLOW` 已登记模式（与现有 `project::ProjectContext` 同机制）。
- DDD 视角：✅ 聚合根内聚（tool 即其输出的聚合根）；composition 是"公共 API" facade（防腐层）；cli 只经 composition 接触 R，不直接 dep tools。
- 缺点：cli 间接 dep tools 业务类型——但这正是"composition 是唯一装配根、cli 通过它访问所有能力"的设计本意，**不是缺点**。
- **结论**：**glm 首选**。

### 方案 4：R 放 `composition` 内部

- 机制：composition 承载 29 个 XxxResult；tools `use composition::ReadResult`。
- 守卫：❌ tools 禁 dep composition（composition 在 tools 之上）。
- **结论**：**否决**（循环依赖）。

### 方案 5：R 放 `sdk`

- 机制 1（直接）：sdk 承载 29 个 XxxResult；tools `use sdk::ReadResult`。
  - 守卫：❌ tools 禁 dep sdk。
- 机制 2（分层）：sdk 定义 29 个 `ReadResultView`（类似现有 `WorkspaceContextView`）；runtime 做 `tools::ReadResult → sdk::ReadResultView` 转换。
  - 守卫：✅ runtime 可以 dep sdk；cli 可以 dep sdk。
  - 代价：runtime 必须为 29 个 struct 写 29 个转换函数 + 重复 DTO，DRY 被打破。
- **结论**：**gpt 首选方案 5（机制 2）**——但实施成本高。

### 方案 6：放弃 typed R，回退 `serde_json::Value`

- 机制：撤销 `#273` typed 重构；保持 `display_text_for_tool_result` 字符串键硬挖。
- 守卫：✅ 零新增 dep。
- 代价：`#273` 目标失效；TUI Display 字段名改动需全局 grep 重构。
- **结论**：**否决**（不满足需求）。

## 5. 3 模型独立结论

### 5.1 glm planner — 推荐方案 3（落地变体）

- **核心论点**："R 的真相唯一归属在 tools（聚合根内聚），composition 作为已登记的 re-export facade 暴露给 cli，sdk/share 零改动。"
- **架构证据**：`tools→{share,project,storage}`、`composition→tools`、`cli→{composition,sdk}` 三条全过；composition `ROOT_REEXPORT_ALLOW` 已有 `project::ProjectContext` 先例。
- **风险点**（glm 自承）："需在 `architecture-guards.md` §6 追加 `tools: {ReadResult, EditResult, …}` 登记条目"。
- **优势**：聚合根内聚 + composition facade + sdk/share 零改动。
- **劣势**：composition 公开 API 膨胀（29 个 re-export）；cli 走 composition 间接拿到工具专属类型。

### 5.2 gpt reviewer — 推荐方案 5（分层 DTO）

- **核心论点**："typed tool result 是 `runtime → cli/TUI` 的通信数据，本质是通信契约；TUI Display contract 应该是 sdk 通信契约；让 CLI 使用 feature crate 的业务类型是把业务模型当成 wire contract。"
- **架构证据**：typed result 是"通信契约"——sdk 名"communication contract"应该承接；runtime 适配层做 `internal result → sdk::ToolResultData` 转换是"严格防腐层"。
- **关键洞察（gpt 独到）**："TUI 不只是 tool 内部消费者，还是跨 `runtime → cli` 通信边界；让 CLI 走 composition re-export 拿到 feature DTO 是边界泄漏。"
- **风险点**（gpt 自承）："SDK 会变大，且需要 adapter/conversion 层把 tool 执行结果映射成 SDK DTO；主要代价在 runtime 适配层。"
- **优势**：严格防腐层、协议稳定、Display exhaustive。
- **劣势**：runtime 增加 29 个转换函数（DRY 风险）；SDK 膨胀。

### 5.3 mimo coder — 推荐方案 7（consumer-side typed deserialization）

- **核心论点**："TUI 实际只用 3 种字段形状（worktree/branch、edit/diff、通用 message/text），不需要 29 个独立 struct；通用 `ToolDisplayData` + `Option` 字段 + `from_value` 工厂方法覆盖所有 case；零生产侧改动。"
- **实证证据**（mimo 源码核查）：`output.rs:495-541` 实际 `display_text_for_tool_result` 的 3 种分支。
- **颠覆性洞察**："29 个 R struct 中 26 个字段 TUI 不消费——这些字段是为未来扩展（server、history 回放）准备，不是 TUI 急需。"
- **建议结构**：
  ```rust
  // 90% 的工具共享这一个结构
  pub struct ToolDisplayData {
      pub status: Option<String>,
      pub display: Option<String>,
      pub message: Option<String>,
      pub text: Option<String>,
      // 特定工具扩展
      pub branch: Option<String>,   // worktree
      pub diff: Option<String>,     // edit
      pub file_path: Option<String>,// read/write
  }
  impl ToolDisplayData {
      pub fn from_value(value: &Value) -> Self { /* 兼容 derive 或手动提取 */ }
      pub fn best_display_text(&self, fallback: &str) -> String { /* display > message > text > fallback */ }
  }
  ```
- **物理位置建议**：`share::tool::display_data.rs`（纯数据，无 IO，符合 share 最小内核）。
- **优势**：0 production 改动（151 个 call site 不动）；回归风险最低；TUI 仍获得类型安全。
- **劣势**：失去 per-tool exhaustive 强类型（但 TUI 实际不需要）；29 个 tool 字段结构信息丢失。

## 6. 关键洞察汇总

| 维度 | 事实 | 影响 |
|---|---|---|
| wire 格式 | `sdk::ChatEvent::ToolResult.content` 永远是 `Value` | 任何方案都需 `(de)serialize` 穿越这条管道 |
| TUI 实际使用面 | 3 种字段形状（worktree/branch、edit/diff、通用） | 26/29 struct 字段 TUI 不消费 |
| 守卫规则 | cli→{composition, sdk}；tools→{share, project, storage} | 排除方案 1/4/5（直接型），可行者：方案 2/3/5（分层） |
| composition re-export | 已是 sanctioned 模式（`project::ProjectContext`） | 方案 3 落地无新守卫动作（需在 `architecture-guards.md` §6 补登记） |
| share 最小内核 | 禁业务类型膨胀 | 方案 2 名不副实 |
| 29 vs 31 | plan 写"29"，实际数是 31（mcp_tool/mcp_manager 拆 2 个 + list_mcp_resources + read_mcp_resource） | 实施时需以实际 31 为准 |

## 7. 候选 4 选 1（**待用户决策**）

### 选 A：glm 方案 3（聚合根内聚）
- **物理位置**：`agent/features/tools/src/business/<tool>.rs`（与 tool 同源）
- **桥梁**：`agent/composition/src/lib.rs` 加 `pub use tools::business::*;`
- **生产侧改动**：151 个 `ToolResult::success_json(json!({...}))` 改为 `serde_json::to_value(&ReadResult{...})`
- **消费侧改动**：TUI `display_text_for_tool_result` 1 函数 + per-tool 29 个 Display 分支
- **守卫动作**：`architecture-guards.md` §6 `ROOT_REEXPORT_ALLOW` 追加 `tools: {ReadResult, ...}` 31 条
- **代价**：composition 公开 API 膨胀（31 re-export）；TUI Display 改 29 个分支（per-tool 强类型）
- **优势**：DDD 纯正、聚合根内聚、新增 tool 改 1 个文件

### 选 B：gpt 方案 5（SDK 通信契约）
- **物理位置**：`packages/sdk/src/tool_result/<tool>.rs`
- **桥梁**：runtime 加 adapter 层 `tools::ReadResult → sdk::ReadResultView`（29 个转换函数）
- **生产侧改动**：151 call site 改为 `serde_json::to_value(&ReadResult{...})`（与 A 同）
- **消费侧改动**：TUI 直接 dep sdk 拿 typed
- **守卫动作**：`architecture-guards.md` §6 追加 `sdk: {ReadResult, ...}` 31 条
- **代价**：runtime 适配层 29 个转换函数（DRY 风险）；SDK 膨胀
- **优势**：严格防腐层、协议稳定、TUI exhaustive

### 选 C：mimo 方案 7（consumer-side 通用 struct）
- **物理位置**：`agent/shared/src/tool/display_data.rs`（1 个 struct + 6 Option 字段 + `from_value`）
- **桥梁**：无
- **生产侧改动**：**0**（151 call site 不动）
- **消费侧改动**：TUI `display_text_for_tool_result` 1 函数改为 `ToolDisplayData::from_value(content).best_display_text(fallback)`
- **守卫动作**：无
- **代价**：失去 per-tool exhaustive 强类型（但 TUI 实际不需要）；server/history 消费者仍需自己 derive
- **优势**：0 改动面、回归风险最低、TUI 立即改善、方案最小
- **风险**：generic struct 不能覆盖所有未来 case（如果 tool 增加新字段，consumer 需手动扩展 `ToolDisplayData`）

### 选 D：mimo 起步 + glm 演进（hybrid）
- **Phase 0**：选 C 通用 struct，立即改善 #273
- **Phase 1**：逐步把高频 tool（read/edit/grep/bash）的 R struct 升级为方案 3 形式
- **代价**：两阶段分摊风险
- **优势**：**最小风险**（Phase 0 几乎零改动）+ 完整演进路径（Phase 1 强化）
- **劣势**：plan 文件需拆为两阶段

## 8. 决策点（请用户拍板）

1. **A / B / C / D** 中选哪个？
2. **29 vs 31**：plan 描述"29 个 tool"是错的吗？实际是 31 个（mcp_tool/mcp_manager 拆 2 + list_mcp_resources + read_mcp_resource）？以哪个为准？
3. **R struct 字段范围**：每 tool 的 R struct 包含哪些字段（仅 TUI 消费的 3 种形状 + 通用？还是 tool 业务输出的全部字段）？
4. **wire 兼容**：`sdk::ChatEvent::ToolResult.content` 保持 `Value`（Phase 0a 决策）还是改 typed enum（需要 runtime 适配）？
5. **`architecture-guards.md` §6 登记**：方案 A/B 需要登记 31 条；方案 C 不需要。

## 9. 状态

- [ ] 等待用户决策（4 选 1）
- [ ] 决策后更新 `2026-06-18-tool-display-structured-data.md` plan 文件
- [ ] 决策后更新 issue #273 / #325 body
- [ ] 决策后重做 Phase 0a（路径调整）

## 10. 参考资料

- plan：`docs/superpowers/plans/2026-06-18-tool-display-structured-data.md`（Phase 0a 已撤销）
- 守卫规则：`docs/design/02-architecture-guards.md` §2/3/4/6
- 依赖铁律：`docs/design/01-outline.md` §依赖铁律
- issue：#273 (TUI tool call detail 优化) / #325 (ToolResult 扁平化重构)
- 相关设计：`docs/superpowers/specs/2026-05-27-tui-model-view-architecture.md`
- 已撤销 commit：`bed64636`（plan）保留；`f2686be8` / `19cadee3` / `363aaa83` / `c097186c` 已 `git reset --soft HEAD~4` 撤销
- HTML 评审（间接相关）：`docs/superpowers/ddd-cli-architecture-review.html`（feature #47 apps/cli 架构审视）
