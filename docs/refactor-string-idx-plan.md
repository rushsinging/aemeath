# String 索引强类型化重构计划

> 目标：用 newtype 在编译期阻止 byte/char/col 三种"字符串索引"互相误用，
> 一劳永逸消除 `byte index N is not a char boundary` 这类 panic。

## 背景与动机

历史上反复出现的一类 panic：

| 时间 | 文件 | 现象 |
|------|------|------|
| 早期 | `aemeath-llm/src/client.rs` | log truncation 切到中文 char 内部（memory #1280） |
| 2026-04-21 | 同上 | UTF-8 boundary panic 导致 Rust LLM 客户端崩溃挂 UI |
| 2026-04-25 | `tui/output_area/streaming.rs:67` | `<think>` 改成 `"思路"` 后 `+7` 偏移落在中文 char 中 |
| 2026-04-26 | `tui/output_area/selection.rs:17` | `screen_line_map` 字符索引被 `&line[..]` 当字节切片 |

根因：Rust `&str` 的索引 API 单位是**字节**且要求 char boundary，但代码里
同时存在三种语义的 `usize`：

- 字节索引（`s.find()`、`s.len()`、字面量长度）
- 字符索引（`chars().count()`、`s.chars().nth(n)`）
- 显示列（`unicode_width::UnicodeWidthStr`）

它们都是 `usize`，类型系统帮不上忙，全靠人肉维护。一旦混用就 panic。

## 设计

### 三个 newtype

放在新建模块 `aemeath-core/src/string_idx.rs`：

```rust
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct ByteIdx(usize);

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct CharIdx(usize);

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct ColIdx(usize);
```

**关键约束**：
- 不实现 `From<usize>` / `Deref<Target = usize>`：不能把裸 `usize` 隐式塞进来
- 互相之间不能 `+`、不能比较：编译期阻止"字符索引拿去切字节"
- 想拿 `usize` 必须显式 `.as_usize()`，可读性强

### 操作 API

```rust
impl ByteIdx {
    pub const ZERO: Self;
    pub fn end_of(s: &str) -> Self;                   // s.len()
    pub fn after_str(self, lit: &str) -> Self;        // self + lit.len()
    pub fn new_at_boundary(s: &str, n: usize) -> Option<Self>;
    pub fn as_usize(self) -> usize;
}

impl CharIdx {
    pub const ZERO: Self;
    pub fn count_in(s: &str) -> Self;                 // s.chars().count()
    pub fn add(self, n: usize) -> Self;
    pub fn as_usize(self) -> usize;
}

impl ColIdx {
    pub const ZERO: Self;
    pub fn width_of(s: &str) -> Self;                 // unicode_width
    pub fn add(self, n: usize) -> Self;
    pub fn as_usize(self) -> usize;
}

// 跨类型转换：必须带 &str 上下文，强制显式
pub fn char_to_byte(s: &str, c: CharIdx) -> ByteIdx;
pub fn byte_to_char(s: &str, b: ByteIdx) -> CharIdx;
pub fn col_to_char(s: &str, c: ColIdx) -> CharIdx;
pub fn char_to_col(s: &str, c: CharIdx) -> ColIdx;
```

### 安全切片

通过扩展 trait 而不是 `Index<Range>`（避免 `SliceIndex` unstable 问题）：

```rust
pub trait StrSlice {
    fn bslice(&self, range: ops::Range<ByteIdx>) -> &str;
    fn bslice_to(&self, end: ByteIdx) -> &str;
    fn bslice_from(&self, start: ByteIdx) -> &str;
    fn cslice(&self, range: ops::Range<CharIdx>) -> &str;  // 内部转字节
}
impl StrSlice for str { ... }
```

调用方写：`s.bslice_from(byte_start)`，类型不对编译就过不了。

### 用法示例

**streaming.rs（修复历史 panic 1）**

```rust
// before — 字面量长度 hardcoded 7
let content_start = abs_start + 7;
&buf[content_start..]

// after — 类型禁止 ByteIdx + literal_int
let content_start = abs_start.after_str(THINK_OPEN);
buf.bslice_from(content_start)
```

**selection.rs（修复历史 panic 2）**

```rust
// before
let (_, char_start, _) = self.screen_line_map[i];   // char_start: usize
&line[char_start..]                                  // panic on Chinese

// after
let (_, char_start, _) = self.screen_line_map[i];   // char_start: CharIdx
let byte_start = char_to_byte(line, char_start);    // 显式转换
line.bslice_from(byte_start)
```

**`screen_line_map` 字段也跟着类型化：**

```rust
// before
pub screen_line_map: Vec<(usize, usize, usize)>,  // 三个 usize 容易换错位置

// after
pub screen_line_map: Vec<(LineIdx, CharIdx, CharIdx)>,
```

可顺手再加 `LineIdx`、`MsgIdx` 防止换错"哪个 vec 的索引"。

## 边界处理

| 边界 | 策略 |
|------|------|
| `serde_json` / `reqwest` body | 仍传 `String` / `&str`，新类型只在内部计算用 |
| `ratatui` 渲染 | 接口要 `String`，最后 `to_string()` 输出 |
| `clipboard` 写入 | 同上 |
| `tui_textarea` 索引 | 该库自身提供 char-aware API，把它的返回值转 `CharIdx` |
| `serde` 序列化 | 给 newtype 加 `#[serde(transparent)]` 或不序列化（newtype 只活在内存中）|
| 现有 `Vec<char>` / `chars()` 操作 | 输入是 `CharIdx::add`，输出仍是 char 序列 |

## 迁移范围

预扫描后涉及文件（按改动量排）：

| 模块 | 改动量 | 主要点 |
|------|--------|--------|
| **新增** `aemeath-core/src/string_idx.rs` | ~150 行 | 类型 + 转换 + StrSlice trait + 单元测试 |
| `aemeath-core/src/lib.rs` | +1 行 | `pub mod string_idx;` |
| `aemeath-core/src/compact/truncate.rs` | ~30 行 | `safe_slice` 类型化 |
| `aemeath-cli/src/tui/output_area/types.rs` | ~10 行 | `screen_line_map` 字段类型 |
| `aemeath-cli/src/tui/output_area/streaming.rs` | ~30 行 | `<think>` 标记切片 |
| `aemeath-cli/src/tui/output_area/selection.rs` | ~30 行 | char→byte 显式转换（3 处） |
| `aemeath-cli/src/tui/output_area/display.rs` | ~50 行 | `wrap_line`、`screen_col_to_char_idx` 类型化 |
| `aemeath-cli/src/tui/output_area/mod.rs` | ~30 行 | `compute_char_offsets`、render 路径 |
| `aemeath-cli/src/render/mod.rs` | ~20 行 | wrap rendering |
| `aemeath-llm/src/client.rs` | ~20 行 | log truncation helper |
| `aemeath-cli/src/tui/input_area.rs` | ~20 行 | textarea 索引（如有自定义计算） |
| 散点 byte slicing | ~30 行 | grep `&[a-z_]+\[[a-z0-9_]+\.\.` 收尾 |

总计约**新增 150 + 改动 270 行**。

## 五个待确认的设计选择

1. **CharIdx 算术**：CharIdx + usize ✅、CharIdx - CharIdx → usize ✅、CharIdx + CharIdx ❌
2. **ByteIdx 跨字符串**：派生自字符串 A 的 ByteIdx 不可用于切串 B（运行时不强制，靠纪律 + assert）
3. **不走 `SliceIndex<str>`**：用扩展 trait `s.bslice(...)`，避开 nightly 特性
4. **不实现 `Display` 直接打 usize**：Debug 走 `ByteIdx(7)` 这种，避免和 usize 印出来一样不易区分
5. **不在 `String::push_str` / `format!` 设防**：这些 API 不带索引语义，无须改造

如果以上五点有不同看法，提前说。

## 执行步骤（按 commit 划分）

### Commit 1：新增 `string_idx` 模块 + 单元测试
- 类型定义、`as_usize`、`after_str`、转换函数
- `StrSlice` trait 实现于 `str`
- 测试用例覆盖：ASCII、CJK、emoji、char boundary 校验
- **不动任何现有代码**

### Commit 2：`aemeath-core` 内部迁移
- `compact/truncate.rs::safe_slice` 改类型签名
- `aemeath-core` 内所有调用点跟改
- 编译过 + cargo test 过

### Commit 3：`aemeath-llm` 迁移
- `client.rs` 的 log truncation helper 改类型化

### Commit 4：TUI output_area 迁移（重头戏）
- `OutputLine` / `screen_line_map` 字段类型化
- `streaming.rs` / `selection.rs` / `display.rs` 三处 panic 高发点
- `compute_char_offsets`、`wrap_line` 重新签名

### Commit 5：TUI 其他模块 + 散点
- `render/mod.rs`、`input_area.rs`
- `grep -rE '&[a-z_]+\[[a-z0-9_]+\.\.' --include='*.rs'` 扫尾

### Commit 6（可选）：CI 守门
- `scripts/check-no-raw-byte-slice.sh`：阻止新 PR 引入裸字节切片
- 加进 `cargo xtask` 或 pre-commit hook

## 验收标准

- [ ] `cargo build --all` 干净，无 warning
- [ ] `cargo test --all` 通过
- [ ] 手动构造下列场景不再 panic：
  - 中文 / emoji 内容流式 streaming
  - 鼠标拖拽选中含中文的输出
  - 极长中文行触发 wrap
  - log debug truncation 落在 CJK 边界
- [ ] 仓库 grep `&[a-z_]+\[[a-z0-9_]+\.\.` 无新增违规
- [ ] `~/.aemeath/aemeath.log` 不再出现 `byte index ... is not a char boundary`

## 风险与回滚

- 涉及 11 个文件，**不能边修边发**——必须一组 commit 一起 merge
- 中途打断会有"半新半旧"的状态，类型不兼容；回滚直接 `git reset --hard`
- 工作期间 `aemeath` 主分支不可用 → 建议在独立 branch 改完再合
- ratatui 升级 / `tui_textarea` 升级时如果其内部接口变化，可能需要再调整 newtype 边界——成本可控

## 工时估算

按一人持续工作：
- Commit 1: 1 小时（类型 + 测试）
- Commit 2-3: 1 小时（小范围 +llm 客户端）
- Commit 4: 1.5-2 小时（TUI 重头）
- Commit 5: 0.5 小时（扫尾）
- Commit 6: 0.5 小时（CI）

**总计约 3.5-4.5 小时**，含编译验证和最少手动冒烟。

---

确认后开始 Commit 1。
