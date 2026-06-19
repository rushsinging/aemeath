// TUI 渲染层 view_model 视角的 tool result 数据结构。
//
// view_model 不得依赖 model（架构规则 `check-tui-model-view-boundaries.sh`），
// 因此 model/conversation 下的同名字段类型在 view_model/conversation 重新声明。
// view_assembler 负责在组装 view 时把 model 端字段拷贝到本类型。
pub mod tool_result_payload;
