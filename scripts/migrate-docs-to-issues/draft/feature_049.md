<!-- Migrated from: docs/feature/active.md#49 -->
### #49 AskUserQuestion 增强——title+description 选项、智能 All/None、Type something 输入框

**状态**：已完成（c98c26c），待确认

**实现**：
1. **OptionItem 类型**：sdk 层新增 `OptionItem { title, description }`，向后兼容纯字符串
2. **智能内建选项**：≥2 LLM 选项时追加 All/None/Type something；1 个追加 None/Type something；0 个纯自由输入
3. **Type something 输入框**：选中后进入行内编辑态，Up 返回选项列表，Enter 提交，Esc 取消
4. **选项渲染**：title 加粗 + description 灰色缩进双行布局
5. **工具 schema 更新**：options 支持 `oneOf(string, object { title, description })`

**变更范围**：sdk, runtime, tools, TUI 全链路（18 files, +404 -96）

**涉及路径**：AskUserQuestion options 渲染、输入态切换、回答构造、文案常量

**验收标准**：选项末尾稳定出现 All/Chat；All 回传结构化选项集合；Chat 进入自由输入态；内建选项不与 LLM option 重名冲突

---
