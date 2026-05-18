# Feature #32: TUI 选中和复制逻辑统一

- **完成日期**：2026-05
- **归档日期**：2026-05-14
- **状态**：已确认完成

## 完成内容

1. clipboard helper 统一入口（copy_to_clipboard + copy_selection_to_clipboard），删除 mouse_handler 重复私有方法
2. OutputArea 新增 is_selecting() getter 统一访问方式
3. 双击 output 时同步清理其他区域 selection
4. 滚轮添加区域判断
5. input area selection 高亮统一使用 safe_text 显示宽度换算，避免 CJK 宽字符导致高亮截断
