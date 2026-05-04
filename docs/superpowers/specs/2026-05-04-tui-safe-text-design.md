# TUI 字符串/切片安全索引收口设计

## 背景

TUI 多次出现同类 panic：字符串字节边界、字符索引、显示列宽、`screen_line_map` 裁剪之间混用。Bug #28 最新暴露了两种同源问题：

1. `selection.rs::get_selected_text()` 中 selection 保存的字符列在内容变短后仍用于 `chars[from..to]`，导致 range 越界。
2. `output_area/mod.rs::render()` 中临时渲染行（spinner/task/status）没有对应 `screen_line_map` 项，使用 `lines.len() - area.height` 裁剪 map 时可能 `split_off(offset)` 越界。

单点加 `min()` 可以止血，但未来新增 TUI 文本逻辑仍容易复发。Feature #23 的目标是把高风险文本操作收口到统一 API，并加检查门禁。

## 目标

- 新增 TUI 专用 `safe_text` 模块，统一处理字符索引、字节切片、显示列宽转换。
- 让业务路径不再直接写高风险切片表达式，优先使用 panic-free API。
- 将 #28 两个止血修复迁移为 `safe_text` 的示范使用点。
- 加本地检查脚本，防止 `chars[from..to]`、`.chars().nth()`、未收口的 `split_off()` 等写法重新进入 TUI 业务路径。
- 增加边界测试和 panic 回归测试，覆盖空输入、CJK、emoji、越界、反向 range、窄窗口临时行。

## 非目标

- 不把模块放到 `aemeath-core`。当前问题集中在 TUI，涉及 ratatui 显示列宽、鼠标列号、可见行映射等 UI 语义。
- 不做全量 grapheme cluster 改造。先使用 Rust `char` + `unicode-width` 解决现有 panic 和 CJK 宽字符问题。
- 不重构整个 output area 架构，只在高风险点做安全 API 替换和门禁。

## 架构

### 新模块

新增：`aemeath-cli/src/tui/safe_text.rs`

职责：

- 提供 panic-free 的字符范围 clamp。
- 提供基于字符索引的安全 `&str` 切片。
- 提供安全字符访问。
- 提供基于 Unicode 显示宽度的截断和列号到字符索引转换。
- 提供安全 `split_off` 辅助，用于动态 offset 可能超过 Vec/VecDeque 长度的场景。

`aemeath-cli/src/tui/mod.rs` 添加：

```rust
pub mod safe_text;
```

### API

```rust
use std::ops::Range;

pub fn clamp_char_range(from: usize, to: usize, chars_len: usize) -> Option<Range<usize>>;

pub fn safe_char_slice(chars: &[char], from: usize, to: usize) -> &[char];

pub fn safe_str_slice_by_char(s: &str, from: usize, to: usize) -> &str;

pub fn safe_char_at(s: &str, idx: usize) -> Option<char>;

pub fn truncate_unicode_width(s: &str, max_cols: usize) -> (&str, usize);

pub fn col_to_char_idx(s: &str, col: usize) -> usize;

pub fn clamp_split_index(offset: usize, len: usize) -> usize;
```

语义约定：

- `clamp_char_range()`：`from` / `to` 均 clamp 到 `chars_len`，如果 clamp 后 `from >= to` 返回 `None`。这样空选择和反向范围都不触发切片。
- `safe_char_slice()`：内部调用 `clamp_char_range()`，无有效范围时返回空切片。
- `safe_str_slice_by_char()`：按字符 index 转换到 UTF-8 字节边界，返回 `&str`；无有效范围时返回 `""`。
- `safe_char_at()`：越界返回 `None`。
- `truncate_unicode_width()`：按显示列宽截断，返回合法 UTF-8 边界的 `&str` 和实际宽度。
- `col_to_char_idx()`：屏幕列号落在宽字符内部时返回该宽字符的 char index。
- `clamp_split_index()`：用于 `split_off` / `skip` 前统一裁剪动态 offset。

## 迁移范围

### 第一批：Bug #28 路径

- `aemeath-cli/src/tui/output_area/selection.rs`
  - `get_selected_text()` 使用 `safe_char_slice()`。
  - 日志里预览字符串使用 `safe_str_slice_by_char()`，避免手写 byte boundary 循环。

- `aemeath-cli/src/tui/output_area/mod.rs`
  - `screen_line_map.split_off(offset)` 前使用 `clamp_split_index()`。
  - 保留最终 `truncate`，确保 map 长度不超过可见行数量。

### 第二批：高风险 TUI 文本路径

- `aemeath-cli/src/tui/output_area/display.rs`
  - `truncate_unicode_width()` 和 `screen_col_to_char_idx()` 迁移到 `safe_text` 或委托给 `safe_text`。
  - 旧函数保留为兼容包装，减少调用点 churn。

- `aemeath-cli/src/tui/input_area.rs`
  - `auto_wrap_current_line()` 中 `chars[best_break..]` 改用 `safe_char_slice()`。
  - suggestion / CJK 宽字符逻辑后续统一走 `safe_text::truncate_unicode_width()`。

- `aemeath-cli/src/tui/output_area/markdown.rs`
  - `rest[..end]`、`trimmed[1..len-1]` 等字符串切片改为 `safe_str_slice_by_char()` 或明确注释 ASCII-only 场景。

- `aemeath-cli/src/tui/output_area/streaming.rs`
  - 保留 `ByteIdx` + `StrSlice`，但新增测试覆盖多字节文本与 think marker 混合场景。

## 门禁

新增脚本：`scripts/check-unsafe-text-ops.sh`

默认扫描 `aemeath-cli/src/tui/**/*.rs`，排除：

- `aemeath-cli/src/tui/safe_text.rs`
- 测试代码中的明确回归样例，如果需要可用注释 `allow unsafe_text_op` 跳过单行。

第一版检查以下模式：

- `.chars().nth(`
- `chars[` 且同一行包含 `..`
- `.split_off(`
- `&s[` 或 `&text[` 这类裸字符串切片的常见形式

脚本打印违规文件和行号，返回非 0。

## 测试策略

### `safe_text.rs` 单元测试

覆盖：

- `clamp_char_range`：正常、空 range、反向、from 越界、to 越界。
- `safe_char_slice`：ASCII、CJK、emoji、越界、反向。
- `safe_str_slice_by_char`：UTF-8 边界、CJK、emoji、越界。
- `truncate_unicode_width`：ASCII、CJK、emoji、max=0、max 落在宽字符内部。
- `col_to_char_idx`：ASCII、CJK、emoji、超出末尾。
- `clamp_split_index`：0、正常、越界。

### 回归测试

保留并扩展当前 #28 测试：

- selection 行变短后复制不 panic。
- selection clamp 后 `from >= to` 返回空。
- spinner/task 临时行超过区域高度时 render 不 panic。

新增迁移测试：

- input auto-wrap 在 CJK / emoji / 超长无空格输入时不 panic。
- markdown inline link/table 处理多字节文本时不 panic。
- streaming think marker 混合多字节文本时不 panic。

## 风险与处理

- grep 门禁可能误报 ASCII-only 切片。处理方式：优先改成安全 API；如果确实是 ASCII 常量或协议 marker，添加局部注释并在脚本中允许 `allow unsafe_text_op`。
- `safe_str_slice_by_char()` 是 O(n)，但 TUI 文本规模可接受。性能敏感路径可先保留 `ByteIdx` + `StrSlice`，但必须有测试证明边界安全。
- `char` 不是用户感知 grapheme。当前目标是消除 panic 和 CJK 宽度问题；复杂 emoji 组合字符作为后续增强。

## 完成标准

- `safe_text.rs` 存在且有覆盖边界的单元测试。
- #28 两个修复点使用 `safe_text` API。
- 至少迁移 `selection.rs`、`output_area/mod.rs`、`display.rs`、`input_area.rs` 的已知高风险点。
- `scripts/check-unsafe-text-ops.sh` 可运行并通过。
- `cargo test -p aemeath-cli safe_text -- --nocapture` 通过。
- `cargo test -p aemeath-cli test_get_selected_text -- --nocapture` 通过。
- `cargo test -p aemeath-cli test_render_clamps_screen_line_map_when_reserved_lines_overflow_height -- --nocapture` 通过。
- `cargo check -p aemeath-cli` 通过。
