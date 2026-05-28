# Feature #53: TUI Model/View 架构迁移

**状态**：✅ 已完成（2026-05-28 用户确认）

**优先级**：高

## 背景

TUI 存在 model/textarea 双状态不同步问题：
- 键盘输入走 model + widget 桥接
- 光标移动（LEFT/RIGHT）直接操作 widget，不更新 model
- 中文输入法下多字节字符（CJK）的光标位置在 model 和 textarea 之间不一致，字符索引 vs 字节位置不匹配导致输入顺序颠倒

## 解决方案

按 `docs/superpowers/specs/2026-05-27-tui-model-view-architecture.md` 与 M1-M18 plan 分阶段迁移：

- **M1-M11**：建立 `view_model` / `view_state` / `view_assembler`、Conversation/Input/Runtime/Diagnostic model、Effect/EffectResult/Effect boundary，补齐 root reducer/coordinator 与事件 mapper，删除旧 `Cmd` adapter
- **M12-M18**：接入 `core::App` 双轨 `TuiModel`/`AppViewState` 基线、Agent ChatEvent 进入 ConversationModel reducer、键盘输入/提交走 InputModel、runtime/session/diagnostic 状态回写状态栏、集中 effect runtime 边界、增加 ViewModel→ratatui Line 渲染适配层、强化/通过 architecture guards
- **最终清理**：删除双轨 facade/bridge（dual_track/input_bridge/runtime_bridge）与 OutputArea 反向 assembler，AgentEvent 输出改由 ConversationModel→OutputViewModel→OutputArea widget adapter 驱动。OutputArea/InputArea/StatusBar 仅保留为 ratatui widget/适配器，不再作为模型 facade

## 相关提交

- `a8cf2ec` feat: 推进 TUI Model/View 单源迁移
- `10ba87e` feat: 合并 TUI Model/View 单源迁移
- `7ba9b73` feat: 删除 TUI legacy facade

## 附带修复

- **中文输入法输入顺序颠倒**：删除双轨 facade 后不再有 model/textarea 双状态同步，单源模型驱动无需光标转换，CJK 字符索引/字节位置不匹配问题自然消失
