# #3 CLI 子命令

**归档日期**：2026-04-27

**实现**：
- `aemeath models` — 表格列出所有可用模型 / `--json`
- `aemeath sessions` — 列出会话 / `--json` / `--delete`
- `aemeath run [OPTIONS]` — 显式启动聊天（所有原有 flag）
- `aemeath [OPTIONS]`（无子命令）— 默认走 `run` 逻辑，接受全部 `run` 子命令参数（如 `--provider`、`--model`、`--resume` 等），保持旧扁平 CLI 的兼容行为

**涉及文件**：`aemeath-cli/src/cli.rs`、`aemeath-cli/src/main.rs`
