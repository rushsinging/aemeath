# Feature #27: 日志分化：input.log / output.log / tool.log

- **完成日期**：2026-05
- **归档日期**：2026-05-14
- **状态**：已确认完成

## 目标

agent 交互日志从 `aemeath.log` 分离为三个 JSON 文件，日志目录移至 `logs/`，`aemeath.log` 收窄为应用诊断日志。

## 完成内容

- input.log（LLM 输入快照）
- output.log（LLM 完整输出）
- tool.log（工具调用请求+结果）
- 日志目录移至 `logs/`
- `aemeath.log` 收窄为应用诊断日志
