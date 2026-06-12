# #48 TUI 窗口 resize 渲染刷新设计

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/152

## 状态

- 状态：设计已确认，待实现
- 方案：方案 2，集中式 `ResizeState / LayoutSnapshot`
- 范围：仅处理 TUI resize 后的布局、缓存、滚动与选择状态一致性，不重构整体渲染架构

## 背景

TUI 收到终端 resize 事件后，当前代码只把 `crossterm::event::Event::Resize(_, _)` 转换为 `Msg::Resize`，而 `update()` 对 `Msg::Resize` 基本不做处理。部分渲染逻辑会在下一帧隐式使用新的 `Frame` 尺寸，例如 status line 会按传入宽度截断，input area 会在 render 时按区域宽度绘制，output area 在宽度变化时会失效 `rendered_cache`。

这种隐式刷新不够完整：高度变化后的 scroll clamp、layout 快照、selection 坐标、input viewport、以及 Markdown/table/code/diff 等重渲染时机缺少统一入口，容易在拖动终端窗口时出现旧尺寸缓存、错位、截断或选区偏移。

## 目标

1. resize 事件进入 update 后有唯一处理入口。
2. 记录最近一次终端尺寸，忽略重复 resize。
3. resize 后显式失效 output render cache，确保 wrap、Markdown/table/code/diff 等基于新宽度重算。
4. resize 后根据最新可见高度 clamp output scroll，避免空白区域或越界。
5. resize 后刷新 input area 的宽度相关状态，避免 cursor / viewport / selection 使用旧宽度。
6. status line 继续依赖 render 宽度即时计算，不引入额外缓存。
7. 避免每帧无条件重建重缓存，仅在尺寸变化、内容变化或主题变化时失效。

## 非目标

1. 不引入完整 dirty flag 渲染系统。
2. 不重写 ratatui layout 结构。
3. 不改变 output/input/status 的视觉设计。
4. 不改变消息、Markdown、代码块、diff 的具体渲染样式。
5. 不新增复杂 resize debounce；若后续发现性能问题再单独处理。

## 现状调研

### 事件入口

- `cli/src/tui/app/run_loop.rs` 读取 crossterm event。
- `Event::Resize(_, _)` 当前转换为 `Msg::Resize`。
- `cli/src/tui/app/update.rs` 中 `Msg::Resize` 未承载尺寸，也未集中处理状态刷新。

### 布局

- `cli/src/tui/app/render.rs` 每帧根据 `Frame::area()` 重新计算 layout。
- status line 已是两行布局，位于顶部。
- output/input/queue/task 等区域高度由 render 阶段即时分配。

### OutputArea

- `cli/src/tui/output_area/render.rs` 调用 `ensure_rendered_cache(area.width)`。
- `cli/src/tui/output_area/rendered_cache.rs` 已有按宽度失效缓存的逻辑。
- `scroll_offset` 与可见高度相关，但 resize 事件本身没有集中 clamp。
- selection 依赖渲染后的行坐标，窗口尺寸变化可能让旧坐标失效。

### InputArea

- `cli/src/tui/input_area/render.rs` render 时根据 area 计算宽度并绘制。
- 宽度变化没有明确的 update 阶段入口。
- cursor/viewport/selection 主要依赖 textarea 自身行为，需要在 resize 后显式更新可用宽度或清理越界状态。

### StatusBar

- `cli/src/tui/status_bar.rs` 和 `status_bar_format.rs` 按 render area width 生成上下两行。
- 当前无需缓存失效，只需确保 resize 后会 redraw。

## 推荐设计：集中式 ResizeState / LayoutSnapshot

### 数据结构

新增小型值对象：

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TerminalSize {
    pub width: u16,
    pub height: u16,
}
```

`App` 新增字段：

```rust
pub last_terminal_size: Option<TerminalSize>,
```

后续如需要记录分区尺寸，可在不破坏接口的前提下扩展为：

```rust
pub(crate) struct LayoutSnapshot {
    pub terminal: TerminalSize,
    pub output_width: u16,
    pub output_height: u16,
    pub input_width: u16,
    pub input_height: u16,
    pub status_height: u16,
}
```

首版不强制保存完整 `LayoutSnapshot`，避免 update 与 render 的 layout 计算重复过多。实现时可先保存 terminal size，并让 render 继续作为 layout 真相源。

### 消息模型

将 `Msg::Resize` 改为携带尺寸：

```rust
Resize { width: u16, height: u16 }
```

事件循环中：

```rust
Event::Resize(width, height) => self.update(Msg::Resize { width, height }).await?
```

### App 处理入口

新增唯一入口：

```rust
impl App {
    pub(crate) fn handle_resize(&mut self, width: u16, height: u16) {
        let new_size = TerminalSize { width, height };
        if self.last_terminal_size == Some(new_size) {
            return;
        }
        self.last_terminal_size = Some(new_size);
        self.output_area.handle_resize(width, estimated_output_height);
        self.input_area.handle_resize(width);
    }
}
```

其中 `estimated_output_height` 可使用当前 render 布局规则的轻量估算；若估算过早引入重复 layout，可先只传 terminal height，并在 `OutputArea` 内做保守 clamp。最终实现应保证 resize 后下一帧 render 不会出现 scroll 越界。

### OutputArea 行为

新增：

```rust
impl OutputArea {
    pub(crate) fn handle_resize(&mut self, width: u16, visible_height_hint: u16) {
        self.invalidate_rendered_cache_if_width_changed(width);
        self.clamp_scroll_for_visible_height(visible_height_hint);
        self.cancel_active_selection_on_resize();
    }
}
```

规则：

1. 宽度变化必须失效 `rendered_cache`，让 wrapped lines、Markdown/table/code/diff 重新生成。
2. 高度变化必须 clamp `scroll_offset`。
3. 若正在 mouse drag selection，resize 后取消 selection，避免拖选坐标跨尺寸继续使用。
4. 已完成 selection 当前在 mouse up 后会复制并清理，因此无需保留跨 resize 选区。
5. 若后续支持持久 selection，再设计基于 message/block anchor 的重映射，不在本 feature 内实现。

### InputArea 行为

新增：

```rust
impl InputArea {
    pub(crate) fn handle_resize(&mut self, width: u16) {
        self.set_content_width(width.saturating_sub(horizontal_padding));
        self.clamp_selection_or_clear_if_invalid();
    }
}
```

规则：

1. 宽度变化后，下一次 render 必须按新宽度 wrap。
2. cursor 位置保留文本 index，不按旧屏幕列保留。
3. selection 若依赖屏幕列且越界，则清理；若 textarea 已能自处理，则只更新宽度。

### StatusBar 行为

StatusBar 不新增 resize 状态。它继续在 render 时根据 `area.width` 调用格式化函数。验收重点是窄屏和宽屏下第二行仍保留关键信息：真实路径前缀、权限模式、session。

## 测试计划

1. `Msg::Resize { width, height }` 会更新 `App.last_terminal_size`。
2. 重复 resize 不重复失效缓存。
3. output width 变化会失效 `rendered_cache`。
4. resize 后 `scroll_offset` 不超过当前内容与可见高度允许的最大值。
5. active selection 在 resize 后被取消，避免旧坐标继续参与复制或高亮。
6. input resize 后 content width 更新，cursor 文本位置保持。
7. status context row 在不同宽度下仍保留路径/权限/session 关键字段。

## 验收标准

1. 拖动终端窗口后，output 区域不会继续显示旧宽度 wrap 结果。
2. 窗口高度变小时，scroll 不越界，不出现大面积空白。
3. 窗口高度变大时，output 能合理展示更多内容。
4. resize 过程中 selection 不产生错位复制或越界高亮。
5. input 区域在窄屏/宽屏切换后 cursor 和 viewport 正常。
6. status line 两行不遮挡 output/input，并按新宽度截断。
7. `cargo check -p aemeath-cli`、`cargo test -p aemeath-cli`、架构守卫通过。

## 实施顺序建议

1. 修改 `Msg::Resize` 和 run loop，使 resize 携带宽高。
2. 增加 `TerminalSize` 与 `App.last_terminal_size`。
3. 实现 `App::handle_resize` 并接入 update。
4. 为 `OutputArea` 增加 resize API，集中处理 cache invalidation、scroll clamp、selection reset。
5. 为 `InputArea` 增加 resize API，处理宽度状态和 selection/cursor 边界。
6. 补单元测试。
7. 跑 `cargo check -p aemeath-cli`、`cargo test -p aemeath-cli`、`.agents/hooks/check-architecture-guards.sh`。
