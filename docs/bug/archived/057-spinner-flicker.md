# Bug #57: Spinner 有时闪烁过快

- **发现日期**：2026-05
- **归档日期**：2026-05-22
- **状态**：已确认修复
- **优先级**：中

## 症状

TUI 中 spinner 动画帧推进速度不一致，LLM 流式响应/工具更新频繁时闪烁过快。

## 根因

`OutputArea::render` 每次重绘都会递增 `spinner.frame`，LLM stream chunk、tool/task 状态更新、终端事件和强制重绘越频繁，spinner 推进越快。AskUser 等待态还会在 stop 后 set phase 间接重启 spinner。

## 修复

新增固定 90ms `spinner_ticker`，并设置 `MissedTickBehavior::Skip`；`spinner.frame` 只在 `Msg::SpinnerTick` 中推进，render 只读取当前帧；AskUser 等待用户时明确 stop spinner，避免 phase 更新重启。
