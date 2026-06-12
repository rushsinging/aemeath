# Feature #50：CLI TUI 目录整理

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/151

## 1. 设计目标

清理 `apps/cli/src` 目录中的结构问题，按功能聚合为语义清晰的文件夹，消除冗余和废弃代码。

目标：

1. 删除 `tui/app/` 中间层，按功能聚合为 6 个文件夹
2. 解决同名文件 + 目录并存（`input_area.rs` / `input_area/`、`key_hints.rs` / `key_hints/`）
3. 合并 `src/render/` 和 `tui/widgets/` 中的重复渲染模块
4. 收拢 `status_bar` 碎片文件
5. 删除 `application/` 废弃骨架
6. 标记或移除 `repl/` 残留

非目标：

1. 不修改任何业务逻辑、行为或 API
2. 不调整 services/（runtime, core, tools 等）目录结构
3. 不修改 TEA 消息类型、State 结构或渲染逻辑

## 2. 现状问题

### 2.1 `tui/app/` 中间层

TUI 目录下套了一层 `app/`，37 个文件按技术职责（render/update/slash）散落，缺乏功能内聚。

### 2.2 同名文件 + 目录并存

```
run_orchestration.rs          ← 模块壳（perm override + mod 声明）
run_orchestration/            ← 子模块目录（3 文件）
tui/input_area.rs             ← 模块壳
tui/input_area/               ← 子模块目录（6 文件）
tui/key_hints.rs
tui/key_hints/
```

Rust 模块系统中这些是合法的（`.rs` 通过 `mod` 声明引用子目录中的文件），但三层并存造成阅读混淆。

### 2.3 render 三层散落

| 路径 | 内容 | 问题 |
|------|------|------|
| `src/render/` | diff, markdown, progress, theme | 与 tui/widgets/ 功能重叠 |
| `tui/render/` | chat/mod.rs（薄壳） | 功能不明，可能废弃 |
| `tui/widgets/` | diff, markdown, tool_display | render/widgets 边界模糊 |

### 2.4 status_bar 碎片

```
status_bar.rs
status_bar_format.rs
status_bar_selection.rs
status_bar_tests.rs
status_bar_v2_tests.rs
```

5 个文件散落在 `tui/` 根下，缺乏目录收敛。

### 2.5 废弃骨架

- `src/application/` — 仅 `mod.rs` + `chat/mod.rs` 骨架，功能已在 runtime + TUI app 实现
- `src/repl/` — 旧版 rustyline REPL ~10 文件，当前主要使用 TUI

## 3. 目标结构

```
apps/cli/src/
├── main.rs
├── cli.rs
├── model_selection.rs
├── sessions_command.rs
├── run_orchestration.rs           → 迁移为 run_orchestration/mod.rs
├── run_orchestration/
│   ├── prompt.rs
│   ├── runtime.rs
│   ├── setup.rs
│   └── setup/
│       ├── prompt_bundle.rs
│       └── tooling.rs
│
├── repl/                         ← 标记 deprecated（暂不删除）
│
└── tui/
    ├── mod.rs                    ← TuiApp 入口
    ├── key_hints/                ← 键盘快捷键提示（key_hints.rs → mod.rs）
    │
    ├── core/                     ← TEA 核心（Model + Update）
    │   ├── mod.rs                ← App struct
    │   ├── msg.rs
    │   ├── event.rs
    │   ├── update.rs
    │   ├── update/
    │   │   ├── key.rs
    │   │   ├── key_nav.rs
    │   │   ├── key_scroll.rs
    │   │   ├── enter.rs
    │   │   ├── done.rs
    │   │   ├── ask_user_key.rs
    │   │   ├── reminder_recap.rs
    │   │   ├── spawn_context.rs
    │   │   ├── spinner.rs
    │   │   └── ui_event.rs
    │   ├── run_loop.rs
    │   ├── runtime.rs
    │   ├── cmd_exec.rs
    │   ├── util.rs
    │   └── state/                ← TEA Model（ChatState, InputState, ...）
    │       ├── mod.rs
    │       ├── chat.rs
    │       ├── input.rs
    │       ├── session.rs
    │       ├── layout.rs
    │       └── ask_user.rs
    │
    ├── input/                    ← 用户输入
    │   ├── input_handler.rs
    │   ├── mouse_handler.rs
    │   ├── paste_handler.rs
    │   ├── clipboard.rs           ← 从 tui/ 根归入
    │   └── input_area/            ← 原 tui/input_area.rs → input_area/mod.rs
    │
    ├── display/                  ← 渲染显示 + 根级孤儿文件归入
    │   ├── render.rs
    │   ├── stream.rs
    │   ├── task_window.rs
    │   ├── dialog.rs              ← 从 tui/ 根归入
    │   ├── syntax.rs              ← 从 tui/ 根归入
    │   ├── task_list.rs           ← 从 tui/ 根归入
    │   ├── theme.rs               ← 从 tui/ 根归入
    │   ├── safe_text.rs           ← 从 tui/ 根归入
    │   ├── status_bar.rs
    │   ├── status_bar_format.rs
    │   ├── status_bar_selection.rs
    │   ├── status_bar_tests.rs
    │   └── status_bar_v2_tests.rs
    │
    ├── session/                  ← 会话生命周期
    │   ├── session_lifecycle.rs
    │   ├── resume.rs
    │   └── processing.rs
    │
    ├── slash/                    ← 斜杠命令（不变）
    │   ├── slash.rs
    │   ├── help.rs
    │   ├── memory.rs
    │   ├── reflection.rs
    │   └── suggestions.rs
    │
    ├── widgets/                  ← UI 组件
    │   ├── mod.rs
    │   ├── diff.rs               ← 从 src/render/diff.rs 合并
    │   ├── markdown.rs           ← 从 src/render/markdown.rs 合并
    │   ├── progress.rs           ← 从 src/render/progress.rs 合并
    │   ├── theme.rs              ← 从 src/render/theme.rs 合并
    │   ├── tool_display.rs
    │   └── tool_display/
    │       ├── agent.rs
    │       ├── common.rs
    │       ├── results.rs
    │       ├── task_impls.rs
    │       └── tool_impls.rs
    │
    └── completion/               ← 自动补全（不变）
        ├── mod.rs
        ├── commands.rs
        ├── files.rs
        ├── models.rs
        ├── parser.rs
        ├── sessions.rs
        └── types.rs
```

## 4. 迁移步骤

### Phase 1：创建目标目录骨架
- 创建 `tui/core/`, `tui/input/`, `tui/display/`, `tui/session/`
- 确保 cargo check 通过

### Phase 2：迁移 TEA 核心 + State → `tui/core/`
- 移动 `tui/app/mod.rs`, `msg.rs`, `event.rs`, `update.rs`, `update/`, `run_loop.rs`, `runtime.rs`, `cmd_exec.rs`, `util.rs`
- 移动 `tui/state/` → `tui/core/state/`
- 更新所有 `use crate::tui::app::` → `use crate::tui::core::`
- 更新 `tui/mod.rs` 引用

### Phase 3：迁移输入处理 → `tui/input/`
- 移动 `tui/app/input_handler.rs`, `mouse_handler.rs`, `paste_handler.rs`
- 移动 `tui/clipboard.rs` → `tui/input/clipboard.rs`
- `tui/input_area.rs` → `tui/input/input_area/mod.rs`，`input_area/` 目录内容移入
- 更新所有引用

### Phase 4：统一同名文件 + 目录
- `run_orchestration.rs` → `run_orchestration/mod.rs`
- `key_hints.rs` → `key_hints/mod.rs`
- 确保无 `.rs` 文件与同名目录并存

### Phase 5：迁移渲染 + 孤儿文件 → `tui/display/`
- 移动 `tui/app/render.rs`, `stream.rs`, `task_window.rs`
- 移动 `tui/status_bar*.rs` （5 个文件）到 `display/`
- 移动 `tui/dialog.rs`, `safe_text.rs`, `syntax.rs`, `task_list.rs`, `theme.rs` 到 `display/`
- 更新所有引用（`crate::tui::dialog` → `crate::tui::display::dialog` 等）

### Phase 6：迁移会话 → `tui/session/`
- 移动 `tui/app/session_lifecycle.rs`, `resume.rs`, `processing.rs`
- 更新所有引用

### Phase 7：合并 render → widgets
- 对比 `src/render/` 和 `tui/widgets/` 中的 diff/markdown/theme 文件
- 去重合并到 `tui/widgets/`
- 删除 `src/render/` 和 `tui/render/`
- 更新所有 `use crate::render::` → `use crate::tui::widgets::`

### Phase 8：删除废弃骨架
- 删除 `src/application/`
- 删除 `tui/app/`（确认所有文件已迁移）
- 删除 `tui/render/`（已合并到 widgets）

### Phase 9：repl/ 标记
- 在 `src/repl/mod.rs` 顶部添加 `#[deprecated]` 注解
- 评估 CLI 是否仍在任何路径调用 repl，若无调用则直接删除

## 5. 涉及引用更新

需要全局更新的 import 路径：

| 旧路径 | 新路径 |
|--------|--------|
| `crate::tui::app::` | `crate::tui::core::` |
| `crate::tui::state::` | `crate::tui::core::state::` |
| `crate::tui::app::input_handler` | `crate::tui::input::input_handler` |
| `crate::tui::app::render` | `crate::tui::display::render` |
| `crate::tui::app::processing` | `crate::tui::session::processing` |
| `crate::tui::dialog` | `crate::tui::display::dialog` |
| `crate::tui::syntax` | `crate::tui::display::syntax` |
| `crate::tui::task_list` | `crate::tui::display::task_list` |
| `crate::tui::theme` | `crate::tui::display::theme` |
| `crate::tui::clipboard` | `crate::tui::input::clipboard` |
| `crate::tui::safe_text` | `crate::tui::display::safe_text` |
| `crate::tui::status_bar*` | `crate::tui::display::status_bar*` |
| `crate::render::` | `crate::tui::widgets::` |
| `crate::application::` | 删除 |
| `tui::input_area::` | `tui::input::input_area::` |

## 6. 验证标准

1. `cargo check` 无错误
2. `cargo build --release -p cli` 通过
3. `cargo clippy` 警告数不增加
4. 838 测试全部通过
5. 架构守卫 7 项全部通过
6. `tui/app/` 目录不存在
7. `application/` 目录不存在
8. `src/render/` 目录不存在
9. 无同名文件 + 目录并存（`run_orchestration.rs`/`run_orchestration/`、`input_area.rs`/`input_area/`、`key_hints.rs`/`key_hints/`）
