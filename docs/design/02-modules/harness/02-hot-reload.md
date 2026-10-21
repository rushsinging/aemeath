# Harness · 热加载契约

> 层级：02-modules / harness｜状态：Target｜Milestone：v0.2.0
> 本文定义组件修改如何在不重启会话的情况下生效，以及失败时的回退语义。

## 1. 机制选型

**复用 turn 边界轮询骨架**（`config_reload` + `SourceSnapshotRegistry`：每 run 开始时对监控文件做快照对比，产出 `ConfigDiff`）。**NEVER 引入文件系统事件监听**（notify 类 crate 跨平台行为差异大，且与既有骨架重复）。

watch 面在现有指令/guidance 文件基础上扩展：

| 新增监控 | 说明 |
|---|---|
| `~/.agents/guardrails.md`、项目 `.agents/guardrails.md` | c2 |
| `~/.agents/tool_guidance.md` | c4 覆盖层 |
| `~/.agents/skills/`、项目 skills 目录（目录级指纹） | c5 目录增删 |
| `~/.agents/scripts/`、项目 scripts 目录 | c7 |
| `~/.agents/aemeath.json` | c6 config 热载 |

guidance 目录从静态两文件改为动态发现。

## 2. 一致性模型（核心契约）

1. **run 开始时快照组件树**：记 `ComponentTreeVersion`（内容指纹集合）。run 执行期间组件版本**固定不变**——包括 tool loop 内行为（c8 plugin 里程碑也不中途换版本）。
2. **下一 run 边界三段式**：`detect`（指纹对比）→ `validate`（registry 防御性复检，无论写入方是 daemon 还是外部编辑器）→ `生效 | 回退`。
3. **校验失败回退**：保留旧版本继续运行 + TUI 用户可见通知（复用 ConfigDiff 通知通道，新增 `ComponentReloadFailed` 事件，含组件 id 与校验错误）。**NEVER 带病热载**。
4. **原子写约定**：所有组件文件变更经 registry `write`（tmp+rename），外部编辑器建议同样约定（文档提示，不强制）。

## 3. 与 RSI 变更的衔接

RSI daemon 定时周期内**自行完成** validate → 原子写盘（daemon 负责落盘，见 03 分片）；主进程无需接收或执行 ChangeSet。落盘后的生效路径与任何外部编辑一致：下一 run 边界三段式检测。

**Remind 通知 LLM**：变更生效（含 RSI 产生）时，复用 GuidanceReloadPolicy 通道向 LLM 发送 system reminder（组件已更新及要点摘要），失败回退发 `ComponentReloadFailed` 通知——成功与失败都让模型侧可感知，NEVER 静默换脑。

## 4. 验收

- **L2 单测**（每组件）：文件变更 → run 边界 diff 产出 → 新内容进入下一 run prompt/registry；校验失败 → 旧版保留 + 事件携带错误。
- **L4 TUI 场景**：会话内修改 guardrails/skill → 下一轮生效；校验失败提示可见。
- **L5 PTY**：复用既有 PTY 范式（含斜杠补全两次回车）——会话中修改 AGENTS.md/guardrails，下一条消息行为变化断言。
- 竞态：写入方用原子写；读取方在 run 边界一次性快照，NEVER run 中途重读。

## 5. 开放决策

- run 内长任务（background process）跨多个 run 边界时组件版本归属：按发起 run 的快照执行（倾向，实施时确认）。
