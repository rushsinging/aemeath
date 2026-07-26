# Workflow（未实现）

Workflow 模块当前未实现，原 Reasoning Graph、ReasoningPort、阶段信号与 UI 阶段事件已退役。

现阶段仅保留模型调用的静态 reasoning/thinking 能力：每个 Run 从配置或用户命令取得冻结的 reasoning level，并直接传给 Context 与 Provider；Runtime 不推断 Explore、Plan、Execute、Verify 阶段。

后续若重新引入 Workflow，必须基于真实的流程编排需求重新设计领域模型、状态所有权、持久化、恢复和 Runtime 集成边界，不复用已退役的工具名称/命令关键词启发式状态机。
