mod common;
mod format;
mod policy;
mod registry;
mod task_impls;
mod tool_impls;
mod traits;

#[cfg(test)]
mod tests;

pub use format::{format_subagent_tool_header, format_tool_call, result_policy};
pub use policy::{DetailsPolicy, HeaderPolicy, ResultPolicy, ResultRender, ToolRenderPolicy};
pub use registry::ToolDisplayEntry;
pub use traits::ToolDisplay;

#[cfg(test)]
pub(crate) use format::result_render_kind;
#[cfg(test)]
pub(crate) use registry::lookup_display;
