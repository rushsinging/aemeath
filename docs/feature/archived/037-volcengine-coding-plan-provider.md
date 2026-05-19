# Feature #37 火山引擎（Volcengine）Coding Plan Provider

- **状态**：已完成，用户确认
- **完成日期**：2026-05-19
- **实现提交**：27708d3 feat(llm): add volcengine coding plan provider refs #37
- **合并提交**：876c638 Merge branch 'feature/37'

## 目标

新增 LLM provider，支持火山引擎 Doubao/火山 Coding Plan API（兼容 OpenAI 协议），接入 ApiDriverKind，并配置默认 base URL、model 与 API key 环境变量。

## 实现内容

1. 新增 `ApiDriverKind::Volcengine`，字符串为 `volcengine`。
2. Volcengine 复用 OpenAI-compatible provider。
3. 默认 base URL：`https://ark.cn-beijing.volces.com/api/coding/v3`。
4. API key 环境变量：`VOLCENGINE_CODING_PLAN_API_KEY`。
5. 默认模型：`doubao-seed-2-0-code-preview-260215`。
6. 内置 Volcengine Coding Plan 模型源，并保留可用的 doubao 2.0、GLM、DeepSeek、Kimi 模型。
7. 更新本机 `~/.aemeath/config.json`，加入用户指定的 Volcengine 模型别名：
   - `doubao-seed-2.0-code`
   - `doubao-seed-2.0-pro`
   - `doubao-seed-2.0-lite`
   - `doubao-seed-code`
   - `minimax-latest`（对应 `minimax-m2.7`）
   - `glm-5.1`
   - `deepseek-v3.2`
   - `deepseek-v4-flash`
   - `deepseek-v4-pro`
   - `kimi-k2.6`

## 验证

- worktree 验证：`cargo fmt --check`、`cargo check`、`cargo test` 通过。
- main 验证：`cargo check`、`cargo test` 通过。
- main 上 `cargo fmt --check` 存在既有无关格式问题：`aemeath-cli/src/tui/output_area/tool_display.rs`。
- 已通过 Volcengine Coding Plan `/models` 接口确认可访问模型列表。

## 确认

用户已确认需求完成，归档。
