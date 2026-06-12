<!-- Migrated from: docs/feature/archived/048-tui-resize-render-refresh.md -->
# Feature #48：TUI 窗口 resize 时重新计算渲染层并刷新显示层

| 字段 | 值 |
|------|-----|
| 优先级 | 高 |
| 完成日期 | 2026-05 |
| 归档日期 | 2026-05-24 |
| 状态 | 已确认完成 |

## 需求

用户拖动终端窗口大小或收到 resize 事件时，TUI 应重新计算 layout、wrap、scroll、selection、Markdown/table/code/diff 等渲染缓存，并刷新显示层，避免窗口尺寸变化后显示内容仍使用旧宽高导致错位、截断、样式丢失或缓存不一致。

## 设计

采用集中式 resize 处理方案：

1. resize 事件进入 update 后有统一处理入口。
2. resize 消息携带终端宽高。
3. App 记录最近一次终端尺寸，忽略重复 resize。
4. resize 后显式刷新 output cache、scroll、selection 与 input width/selection。
5. status line 继续按 render 宽度即时重算，不引入额外缓存。
6. 避免每帧无条件重建缓存，仅在尺寸变化、内容变化或主题变化时失效。

详细设计保留在 `docs/feature/specs/048-tui-resize-render-refresh.md`。

## 实现结果

TUI resize 已接入集中式处理：Resize 消息携带终端宽高，App 记录最近尺寸并统一刷新 output cache/scroll/selection 与 input width/selection；status line 继续按 render 宽度即时重算。

## 验证

2026-05-24 用户确认 feature #48 已完成。活动列表中移除 #48，并保留此归档记录。
