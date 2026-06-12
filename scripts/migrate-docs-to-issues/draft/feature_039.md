<!-- Migrated from: docs/feature/archived/039-ctrlc-two-stage-exit.md -->
# Feature #39：Ctrl+C 两段式退出

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 完成日期 | 2026-05 |

## 需求

第一次 Ctrl+C 清空 input area（如有内容），input 为空时第一次 Ctrl+C 显示提示「再按一次退出」；3 秒内再次 Ctrl+C 才退出 TUI；超时后计数器重置，status line 自动复原。

## 实现

### 改动文件

- `aemeath-cli/src/tui/app/update/key.rs`：提取 `ctrlc_action` 纯函数 + `CtrlCAction` 枚举，三段式 Ctrl+C 行为
- `aemeath-cli/src/tui/app/update.rs`：re-export `CTRL_C_TIMEOUT_SECS` 常量
- `aemeath-cli/src/tui/app/mod.rs`：新增 `check_ctrlc_timeout()` 方法
- `aemeath-cli/src/tui/app/run_loop.rs`：每帧 tick 调用 `check_ctrlc_timeout()`

### 行为

1. `is_processing` → 中断处理（不变）
2. `showing suggestions` → 清除建议（不变）
3. `input 非空` → 清空 input area，记录时间戳，提示 "Input cleared (Ctrl+C again to exit)"
4. `input 为空` → 3 秒超时窗口内再次按下退出，否则提示 "Press Ctrl+C again to exit"
5. 超时后 status line 自动恢复为 "Ready"（每 50ms tick 检查）

## 关键提交

| commit | 说明 |
|--------|------|
| `6fb9ce5` | feat(#39): Ctrl+C 两段式退出 |
| `757f057` | fix(#39): Ctrl+C 超时改为 3 秒，超时后 status line 复原 |
| `471e9c9` | fix(#39): Ctrl+C 超时后 status line 自动复原 |

## 测试

5 个测试用例（`key.rs`）覆盖 `ctrlc_action` 纯函数：
- input 非空 → ClearInput
- input 空、首次 → WarnExit
- input 空、超时窗口内 → Quit
- input 空、超时过期 → WarnExit
- 边界值（2.9s / 3.1s）
