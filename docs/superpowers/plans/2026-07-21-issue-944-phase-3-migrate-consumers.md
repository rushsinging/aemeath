# Issue #944 阶段三实施计划：按消费者批次迁移 Reducer Façade

> 对应 Issue：[ #944](https://github.com/rushsinging/aemeath/issues/944)，父 Issue：#860。
> 前置：阶段一已建立 `AgentIntent/reduce_intent`；阶段二已退役 spinner、queued、compact 旁路 mutation。

## 目标

迁移**生产**消费者的 `model.{conversation,input,diagnostic,session}.apply(...)` 到 App reducer façade，移除各调用点手工 dirty 标记。完成后，除 `update/root_reducer.rs` 外，生产 TUI 代码不再直接调用四个 Context 的 `apply()`。

## 批次与退役

| 批次 | 生产路径 | 当前直接 apply | 新入口 | 当批退役条件 |
|---|---|---|---|---|
| A | `app/update/{key,notice,ask_user_key,ui_event}.rs` | Input、Conversation | `App::apply_agent_intent` | 这四文件零 `.model.*.apply(`；删除紧随其后的重复 `mark_output_dirty` |
| B | `app/{runtime,util}.rs`、`app/slash/suggestions.rs` | Conversation、Input | `App::apply_agent_intent` | 三文件零直接 apply |
| C | `effect/session/resume.rs` | Session、Conversation、Input | `App::apply_agent_intent` | resume 生产路径零直接 apply；保留 App 顶层 session metadata，留给后续 SessionProjection 阶段 |
| D | `adapter/status_widget.rs` | Conversation、Diagnostic、Session | 仅测试 helper 改为 reducer 调用 | 生产实现零直接 apply；测试不扩大生产 API |

**不在本阶段迁移：** `update/root_reducer.rs`（唯一允许的 Context apply）；测试中的 setup apply；`App::new` 初始化；`update_ui` 删除、Config/Workspace 拆分、字段私有化。

## 实施顺序

1. **Red：** 为 `App::apply_agent_intent` 增加或调整测试，验证 Conversation 变更自动标记 output、Input 自动标记 input，并只生成一个 render request。
2. **共同 façade：** 确认 `apply_agent_intent` 合并 reducer dirty；它不得执行 Effect，仅保留 reducer 结果的 render dirty。
3. **批次 A：** 把 key/notice/ask-user/ui-event 的写入改为 `AgentIntent`；删除由 reducer Change 覆盖的手工 output/status dirty，保留仅与本地 UI 状态相关的刷新。
4. **批次 B：** 迁移 runtime/util/suggestions；验证 slash completion、workspace 状态和 transient notice。
5. **批次 C：** 迁移 session resume 三 Context 写入；保留 effect 内的 SDK 数据解析与 App 顶层 session metadata。
6. **批次 D：** adapter 测试改走 reducer，不让 status widget 保持第二 mutation API。
7. **L0：** 在 `architecture_tests.rs` 扫描生产 TUI 源；仅允许 `update/root_reducer.rs` 包含 `model.{conversation,input,diagnostic,session}.apply(`。

## 每批验证

```text
cargo test -p cli tui::app::update
cargo test -p cli tui::effect::session
cargo test -p cli tui::adapter::status_widget
cargo test -p cli tui::architecture_tests
cargo check -p cli
git diff --check
```

批次完成后运行对应定向测试，再继续下批；任一批出现不能由当前 `AgentIntent` 表达的 mutation，停止并将新 Intent 作为同一阶段最小补充，不引入裸 Context mut accessor。

## 阶段退出

- `rg 'model\.(conversation|input|diagnostic|session)\.apply\(' apps/cli/src/tui` 的生产结果仅为 `update/root_reducer.rs`；
- 不残留因直接 apply 而补的重复 dirty/render 调用；
- 阶段四开始前，不新增新的直接 Context apply 调用。
