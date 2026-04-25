# String 索引混淆（byte vs char）

## 症状
多处代码使用 byte index 操作 Unicode 字符串（`chars().nth()`、byte slice `[..]`），当遇到多字节字符时 panic 或返回错误结果。

## 根因
Rust 的 `str` 操作默认为 byte offset，但 UI 代码中多处语义是 char index（屏幕光标位置、字符选择范围）。两者未被区分，混合使用导致越界或错误切片。

## 修复
引入 `string_idx` 模块，定义 `CharIdx`、`ByteIdx`、`LineIdx` 等新类型，禁止 `usize`->索引的隐式转换，要求通过 `StrSlice` trait 显式操作。

移植了 `char_to_col` / `col_to_char` 从 byte 索引改为 char 索引，修复了一个 bug。

## 回归测试
- 测试覆盖 50 例：构造、转换、数学运算、字符偏移
- 所有涉及 `<think>` 标记解析的 streaming 代码改为 `ByteIdx` 保护
- `selection.rs`：选中/反选时使用 `CharIdx` 计算，消除越界风险

## 关联路径
- selection（最高风险，多处 `.len()` `.chars()` 混用）
- streaming（`<think>` 标记偏移）
- display（`col_to_char` 偏移计算）
