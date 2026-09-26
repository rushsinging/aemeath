---
name: build-cli
description: Use when building and installing the aemeath CLI binary from the current checkout (release build, version stamping, macOS re-signing) or when the installed ~/.local/bin/aemeath must be refreshed after merging to main.
---

# Build CLI

## Overview

把当前 checkout 构建为 release 二进制并安装到 `~/.local/bin/aemeath`，唯一入口是仓库根的 `build_cli.sh`。未显式指定版本号时，脚本使用 `0.0.0-<short commit>` 作为版本，因此安装后的 `aemeath --version` 会带出对应源码 revision。

## 何时使用

- 合并到 main 后需要刷新本机已安装的二进制；
- 需要按某个具体 commit 复现 CLI 行为；
- 需要验证 release 构建产物（日常带调试日志的开发运行请改用 `run_debug.sh`）。

## 步骤

1. 确认 checkout 与目标 commit：`git log --oneline -1`；在 worktree 中操作时先确认分支已包含目标代码。
2. 需要固定版本号时显式传入：`AEMEATH_VERSION=0.1.0 ./build_cli.sh`；不设置则由脚本注入 `0.0.0-<short commit>`。
3. 执行构建：`./build_cli.sh`
   - 内部执行 `cargo build --release --package cli`；
   - 随后复制到 `$INSTALL_DIR/aemeath`（默认 `INSTALL_DIR=$HOME/.local/bin`）；
   - macOS 上清理 `com.apple.provenance` xattr 并做 ad-hoc 重签名，避免 Gatekeeper 杀掉新拷贝的二进制。
4. 验证版本：`~/.local/bin/aemeath --version`，输出应与 `git rev-parse --short=8 HEAD`（或显式设置的 `AEMEATH_VERSION`）一致。
5. 验证可执行路径：`which aemeath`，确认解析到 `$INSTALL_DIR/aemeath`。

## Rules

- **MUST** 使用 `./build_cli.sh` 安装二进制；**NEVER** 手工 `cp target/release/aemeath ~/.local/bin/`——脚本负责 xattr 清理与重签名，手工复制会让二进制在 macOS 上无法启动。
- **MUST** 在目标 commit 上构建：主仓落后时先 `git pull --ff-only`，worktree 中先确认分支内容。
- **SHOULD** 构建前检查磁盘余量；不足时按 `clean-worktree` 技能清理构建缓存（`scripts/clean-worktree-targets.sh --dry-run --max-size-gb 20`，确认后再加 `--yes`）。
- **NEVER** 在 `build_cli.sh` 之外复制或替换 `$INSTALL_DIR` 下的二进制来“热修”。

## 常见错误

| 错误 | 正确做法 |
|---|---|
| 手工 `cp target/release/aemeath ~/.local/bin/` | 运行 `./build_cli.sh`（含重签名与 xattr 处理） |
| `--version` 仍显示旧 commit | 确认构建发生在目标 commit；检查 `INSTALL_DIR` 是否被环境变量覆盖 |
| 在落后多版本的 checkout 上构建 | 先 `git pull --ff-only`，再构建 |
| 用 release 二进制排查交互问题 | 改用 `./run_debug.sh`（默认 `AEMEATH_LOG_LEVEL=debug`，同样注入源码 revision） |
| 磁盘写满导致链接失败（`ld: write() failed, errno=28`） | 清理构建缓存后重试 |
