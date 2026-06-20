use super::ToolExecutionContext;
use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use share::tool::{ImageData, PathAccess, ToolResult};

/// 工具列表查询能力（contract 层定义，由 `ToolRegistry` 实现）。
///
/// 用于 `ToolSearch` 从注册表动态获取已注册工具的 name + description，
/// 避免 contract 层依赖 core 层的 `ToolRegistry` 具体类型。
pub trait ToolListProvider: Send + Sync {
    /// 所有已注册工具的 name 列表。
    fn tool_names(&self) -> Vec<String>;
    /// 按 name 获取工具 description。
    fn tool_description(&self, name: &str) -> Option<String>;
}

/// Type-erased tool trait（registry 存储这个）。
///
/// 工具源应实现 [`TypedTool`]，用 [`TypedToolAdapter`] 包装后注册。
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> Value;

    /// 输出数据的 JSON Schema（用于 TUI 结构化显示）。
    /// 默认返回 `Value::Null`。
    fn data_schema(&self) -> Value {
        Value::Null
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn timeout_secs(&self) -> u64 {
        120
    }

    fn path_accesses(&self) -> &'static [PathAccess] {
        &[]
    }

    fn is_input_safe(&self, _input: &Value) -> bool {
        false
    }

    fn requires_read_before_write(&self) -> bool {
        false
    }

    /// Execute the tool（type-erased）。工具源应实现 [`TypedTool`]。
    async fn call(&self, input: Value, ctx: &ToolExecutionContext) -> ToolResult;
}

// ── TypedTool ────────────────────────────────────────────────────

/// Typed tool result（工具源返回这个）。
pub struct TypedToolResult<T: Serialize + Send + 'static> {
    /// 文本输出（TUI 显示 + 发给 LLM）
    pub output: String,
    /// 结构化数据（adapter 自动序列化为 JSON）
    pub data: Option<T>,
    pub is_error: bool,
    pub images: Vec<ImageData>,
}

impl<T: Serialize + Send + 'static> TypedToolResult<T> {
    pub fn success(output: impl Into<String>, data: T) -> Self {
        Self {
            output: output.into(),
            data: Some(data),
            is_error: false,
            images: vec![],
        }
    }

    pub fn error(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            data: None,
            is_error: true,
            images: vec![],
        }
    }

    /// 添加图片。
    pub fn with_image(mut self, base64: String, media_type: String) -> Self {
        self.images.push(ImageData { base64, media_type });
        self
    }
}

/// Typed tool trait（工具源实现这个）。
#[async_trait]
pub trait TypedTool: Send + Sync {
    type Output: Serialize + Send + 'static;

    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> Value;

    /// 输出数据的 JSON Schema（用于 TUI 结构化显示）。
    /// 默认返回 `Value::Null`，工具可以覆盖以提供 schema。
    fn data_schema(&self) -> Value {
        Value::Null
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn timeout_secs(&self) -> u64 {
        120
    }

    fn path_accesses(&self) -> &'static [PathAccess] {
        &[]
    }

    fn is_input_safe(&self, _input: &Value) -> bool {
        false
    }

    fn requires_read_before_write(&self) -> bool {
        false
    }

    async fn call(&self, input: Value, ctx: &ToolExecutionContext)
        -> TypedToolResult<Self::Output>;
}

// ── TypedToolAdapter ─────────────────────────────────────────────

/// Adapter：[`TypedTool`] → [`Tool`]。
pub struct TypedToolAdapter<T: TypedTool>(pub T);

impl<T: TypedTool> TypedToolAdapter<T> {
    pub fn new(tool: T) -> Self {
        Self(tool)
    }
}

#[async_trait]
impl<T: TypedTool> Tool for TypedToolAdapter<T> {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn description(&self) -> &str {
        self.0.description()
    }

    fn input_schema(&self) -> Value {
        self.0.input_schema()
    }

    fn data_schema(&self) -> Value {
        self.0.data_schema()
    }

    fn is_read_only(&self) -> bool {
        self.0.is_read_only()
    }

    fn is_concurrency_safe(&self) -> bool {
        self.0.is_concurrency_safe()
    }

    fn timeout_secs(&self) -> u64 {
        self.0.timeout_secs()
    }

    fn path_accesses(&self) -> &'static [PathAccess] {
        self.0.path_accesses()
    }

    fn is_input_safe(&self, input: &Value) -> bool {
        self.0.is_input_safe(input)
    }

    fn requires_read_before_write(&self) -> bool {
        self.0.requires_read_before_write()
    }

    async fn call(&self, input: Value, ctx: &ToolExecutionContext) -> ToolResult {
        let result = self.0.call(input, ctx).await;
        let content = match &result.data {
            Some(data) => serde_json::to_value(data)
                .expect("TypedToolAdapter: data serialization should not fail"),
            None => Value::Null,
        };
        ToolResult {
            output: result.output,
            content,
            is_error: result.is_error,
            images: result.images,
            data: None,
        }
    }
}
