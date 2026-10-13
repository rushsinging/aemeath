---
name: e2e-accept
description: Use when verifying an aemeath behavior end-to-end with the live TUI driven by an LLM (cmux paste), especially after runtime/tool/TUI changes that unit tests alone cannot prove.
---

# aemeath 端到端验收（LLM 驱动）

## Overview

用 **当前 worktree 源码** 起真实 TUI，经 cmux 把验收提示贴进会话，让会话内 LLM 自行调工具并回报 PASS/FAIL。适合 Watch 流式、后台任务、卡片显示等「单测绿但真机才露馅」的链路。

**NEVER** 用本 skill 替代 L1–L5 自动化门禁；它是补强，不是放行条件。

## 硬规则

- **MUST** 用仓库根 `./run_debug.sh` 启动（`cargo run`，始终吃当前工作区未提交改动）。
- **NEVER** 为验收跑 `./build_cli.sh`——会覆盖 `~/.local/bin/aemeath`，污染全局安装版。
- **MUST** 在改动所在 worktree / checkout 目录启动，先 `git rev-parse --short=8 HEAD` 与 `git status -sb` 确认分支与脏区。
- **MUST** 验收结论以屏幕可见证据 + 任务日志文件为准，**NEVER** 只信模型口头「通过」。
- 用词：**端到端验收 / LLM 驱动验收**；**NEVER** 使用「狗粮 / dogfood」等俚语对用户说话。

## 前置

- 本机 cmux 可用：`cmux identify --json`
- 需要：`cmux new-workspace`、`cmux paste --submit`、`cmux read-screen`、`cmux shot`
- 不依赖 cmux-cua（无障碍树点击也能完成）；若有 CUA 可作补充，不是必需。

## 步骤

### 1. 起验收会话

在目标 worktree：

```bash
cmux new-workspace \
  --name "aemeath-e2e-<issue>" \
  --cwd "$PWD" \
  --command 'AEMEATH_TOOL_BACKGROUND_THRESHOLD_SECS=1 AEMEATH_LOG_LEVEL=aemeath:agent:runtime=debug ./run_debug.sh' \
  --focus true --json
```

记下返回的 `workspace_ref` / 之后用：

```bash
cmux list-pane-surfaces --workspace workspace:<id> --json
```

取 terminal 的 `surface_ref`。用 `cmux read-screen --surface surface:<id> --lines 40` 等到 TUI Ready（冷编译可能数分钟）。

### 2. 注入验收提示

把提示写入文件再 paste（避免 shell 转义炸掉中文/括号）：

```bash
cmux paste --surface surface:<id> --workspace workspace:<id> --submit -- "$(cat /tmp/aemeath-e2e-prompt.txt)"
```

提示模板 **MUST** 要求模型：

1. 按场景调用真实工具（不要只空谈）。
2. 列出可核对的验收点（日志路径、关键字、卡片文案）。
3. 最后明确输出 `## PASS` 或 `## FAIL` + 证据摘录。
4. **NEVER** 编造未在屏幕/文件中出现的行。

后台 / Watch 类场景示例要点：

- 阈值已由 env 压到 1s 时写明，避免模型再改配置空等。
- Agent 转后台后立刻 `BackgroundProcessWatch`（可传统一入参 `timeout`，默认/上限 300）。
- 同时读 `~/.agents/sessions/<session_id>.background-process/<bgp_id>.log`。

从 TUI 状态栏抄 `session …` id；日志目录即该 id。

### 3. 观察与取证

```bash
cmux read-screen --surface surface:<id> --lines 80
ls -la ~/.agents/sessions/<session_id>.background-process/
cat ~/.agents/sessions/<session_id>.background-process/bgp_*.log
cmux shot --label e2e-<issue> --out /tmp/aemeath-e2e-<issue>.png
```

轮询间隔建议 8–15s；Watch/长任务可到数分钟。

### 4. 判定

| 结果 | 条件 |
|---|---|
| **PASS** | 屏幕有 `## PASS`，且日志/卡片关键字与提示中的验收点一致 |
| **FAIL** | `## FAIL`，或模型称通过但文件/屏幕无对应证据 |
| **无效** | 只用了全局 `aemeath` 二进制、或 worktree 无目标改动 |

## 与自动化测试的分工

| 层 | 谁做 |
|---|---|
| L1–L4 / 契约 | `cargo test`（默认门禁） |
| L5 PTY | 仓库内 PTY 场景（若该能力已登记） |
| 本 skill | 真 TUI + 真模型工具链的补强验收 |

## 常见错误

| 错误 | 正确做法 |
|---|---|
| `./build_cli.sh` 再跑安装版验收 | `./run_debug.sh` |
| 在主仓跑、改动在 worktree | `--cwd` 指到含改动的 worktree |
| 只看模型说 PASS | 核对 `.log` / `read-screen` / `shot` |
| shell 里直接嵌多行中文 prompt | 先写文件再 `paste -- "$(cat …)"` |
| 用本会话 Agent 工具代替目标 TUI | 验收对象是 cmux 里那份 `run_debug` 进程 |
