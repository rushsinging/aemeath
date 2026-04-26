# 新增的 /think 命令无法自动补全

## 症状
在 TUI 中输入 `/t` 时，自动补全列表不显示 `/think`。输入 `/think` 后命令可以正常执行（切换 thinking 模式），但无法通过 Tab 补全发现或快速输入。

## 根因
`/think` 命令在 `aemeath-cli/src/tui/app/slash.rs` 第 129-143 行作为硬编码 match arm 实现，但**未注册到 `CommandRegistry`**。

自动补全在 `slash.rs` 第 407-412 行通过 `CommandRegistry::with_defaults().list()` 获取命令列表，`register_defaults()` 中不包含 `think`，因此 `/think` 永远不会出现在补全建议中。

这是一个**重复出现的 bug 模式**：新增 slash 命令时只加硬编码 handler，忘记同步注册到 `CommandRegistry` 的 `register_defaults()`。受影响的命令还包括 `/context`、`/paste`、`/images`、`/clear-images`、`/save`（它们也都只有硬编码 handler，没有注册）。

## 修复
1. 在 `aemeath-core/src/command/commands/misc.rs`（或其他合适位置）添加 `think_command()` 构造函数
2. 在 `aemeath-core/src/command/commands/mod.rs` 的 `builtin` 模块中导出 `think_command`
3. 在 `aemeath-core/src/command/mod.rs` 的 `cmd` 模块中添加 `pub const THINK: &str = "think";`
4. 在 `aemeath-core/src/command/registry.rs` 的 `register_defaults()` 中注册 `think_command()`
5. （可选）将 `slash.rs` 中的 `/think` 硬编码 handler 改为走 `CommandRegistry` 执行路径，或将 `/think` 的特殊逻辑（切换 reasoning）通过 `CommandAction` 暴露

## 回归测试
- 输入 `/t` 应出现 `/think` 补全建议
- `/think` 命令本身功能正常（切换 thinking mode on/off）
- `/think on` / `/think off` 参数正常工作
- 状态栏 thinking 指示灯同步更新

## 关联
- 属于重复出现的模式：之前也出现过类似情况（命令仅硬编码不注册）
- 涉及路径/模块：slash.rs handler ← CommandRegistry ← completion/commands.rs
