<!-- Migrated from: docs/feature/archived/023-tui-safe-text-indexing.md -->
# #23 TUI 字符串/切片安全索引收口

**状态**：✅ 已完成，用户已确认
**归档日期**：2026-05-06
**优先级**：高

## 目标

把 TUI 路径中“按字符索引、按字节切片、按宽度截断、按显示列号定位”等容易越界的操作收口到统一工具模块，业务路径全部走安全 API，避免直接 `chars[from..to]`、`s[i..j]`、`chars().nth(n)`、把 `text.len()` 当字符/显示宽度等高风险写法。

## 完成内容

1. 新增 `aemeath-cli/src/tui/safe_text.rs`，统一提供 panic-free 字符范围、字符串切片、显示宽度截断、列号转换、split index clamp，并补充 `str_display_width`。
2. `selection.rs` 的复制选中文本路径迁移到 `safe_char_slice` / `safe_str_slice_by_char`。
3. `output_area/mod.rs` 的 `screen_line_map.split_off` 迁移到 `clamp_split_index`。
4. `output_area/display.rs` 的宽度截断和列号转换委托给 `safe_text`。
5. `input_area.rs` 自动换行后缀提取改为 `safe_char_slice`。
6. 新增 `scripts/check-unsafe-text-ops.sh` 门禁，阻止 TUI 业务路径重新出现高风险切片/索引写法。
7. 补充 safe_text/display 相关边界测试，以及 markdown CJK link 渲染测试，覆盖 CJK 宽字符与安全索引场景。

## 实际 API

`aemeath-cli/src/tui/safe_text.rs` 提供：

- `safe_char_slice(chars, from, to)`
- `safe_char_at(s, idx)`
- `clamp_char_range(from, to, chars_len)`
- `safe_str_slice_by_char(s, from, to)`
- `truncate_unicode_width(s, max_cols)`
- `str_display_width(s)`
- `col_to_char_idx(s, col)`
- `clamp_split_index(offset, len)`

这些 API 均按 panic-free 设计：越界时返回空切片、空字符串、`None` 或 clamp 后的位置。

## 业务迁移范围

- `aemeath-cli/src/tui/output_area/selection.rs`
- `aemeath-cli/src/tui/output_area/mod.rs`
- `aemeath-cli/src/tui/output_area/display.rs`
- `aemeath-cli/src/tui/input_area.rs`
- `scripts/check-unsafe-text-ops.sh`

## 边界说明

- `safe_text` 是 TUI 字符索引 / 显示宽度安全层。
- `aemeath-core::string_idx` 是字节 / 字符强类型索引层，两者当前并存。
- `markdown.rs` 中经 `.get(byte_range)` 验证的字节范围可通过白名单保留。
- `streaming.rs` 的 thinking block 解析属于字节级协议/标签扫描，继续使用 core 的 `ByteIdx` / `StrSlice`。

## 关联问题

| Bug | 路径 | 越界类型 |
|-----|------|----------|
| #4（archived）| Output area 渲染 | `screen_line_map` 索引越界 / CharIdx 运算溢出 / wrap 计算与 screen_line_map 不一致 |
| #5（archived）| 鼠标选中位置 | `screen_line_map` 滚动裁剪未同步 |
| #8（archived）| 字符串索引 | 字节/字符长度混淆 |
| #16（archived）| `/resume` 列表 CJK | `chars().nth(x_usize)` 用屏幕列号当字符索引 + `text.len()` 当显示宽度 |
| #28（archived）| 复制选中文本 | `chars[from..to]` 中 `from` 未做 `chars.len()` 裁剪 |

## 验证

用户已确认完成。
