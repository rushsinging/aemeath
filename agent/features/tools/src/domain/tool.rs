use super::ToolExecutionContext;
use crate::domain::{types::tool_search::ToolInfo, ImageData, ToolResult, ToolSuspension};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use std::borrow::Cow;

/// 工具列表查询能力（contract 层定义，由 `ToolRegistry` 实现）。
///
/// 用于 `ToolSearch` 从注册表动态获取已注册工具的完整信息，
/// 避免 contract 层依赖 core 层的 `ToolRegistry` 具体类型。
pub trait ToolListProvider: Send + Sync {
    /// 所有已注册工具的 name 列表。
    fn tool_names(&self) -> Vec<String>;
    /// 按 name 获取工具 description。
    fn tool_description(&self, name: &str) -> Option<String>;
    /// 按 name 获取工具完整信息（含 description、input_schema、is_read_only）。
    fn tool_info(&self, name: &str) -> Option<ToolInfo>;
}

/// Type-erased tool trait（registry 存储这个）。
///
/// 工具源应实现 [`TypedTool`]，用 [`TypedToolAdapter`] 包装后注册。
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;

    /// 按 lang 返回本地化 description（注入 LLM 的 tool schema 用）。
    /// 默认委托 [`description`](Self::description)（默认语言英文），
    /// 需要双语的工具覆盖此方法。
    fn description_for(&self, lang: &str) -> Cow<'_, str> {
        let _ = lang;
        Cow::Borrowed(self.description())
    }

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
        false
    }

    fn timeout_secs(&self) -> u64 {
        120
    }

    fn cancellation(&self) -> crate::domain::published_language::CancellationDeclaration {
        crate::domain::published_language::CancellationDeclaration::NonCooperative
    }

    fn is_input_safe(&self, _input: &Value) -> bool {
        false
    }

    /// Parse a validated input into a typed suspension without invoking the
    /// legacy asynchronous call path. Most tools complete normally.
    fn suspension(&self, _input: &Value) -> Option<Result<ToolSuspension, String>> {
        None
    }

    /// Execute the tool（type-erased）。工具源应实现 [`TypedTool`]。
    async fn call(&self, input: Value, ctx: &ToolExecutionContext) -> ToolResult;
}

// ── TypedTool ────────────────────────────────────────────────────

/// Typed tool result（工具源返回这个）。
pub struct TypedToolResult<T: Serialize + Send + 'static> {
    /// 给 LLM 的文本（经 `to_llm_view` text-first 投影）。
    pub text: String,
    /// 结构化数据（adapter 自动序列化为 JSON，给 TUI）。
    pub data: Option<T>,
    pub is_error: bool,
    pub error_kind: Option<crate::domain::ToolErrorKind>,
    pub images: Vec<ImageData>,
}

impl<T: Serialize + Send + 'static> TypedToolResult<T> {
    pub fn success(text: impl Into<String>, data: T) -> Self {
        Self {
            text: text.into(),
            data: Some(data),
            is_error: false,
            error_kind: None,
            images: vec![],
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            data: None,
            is_error: true,
            error_kind: Some(crate::domain::ToolErrorKind::InvalidInput),
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

    /// 按 lang 返回本地化 description（注入 LLM 的 tool schema 用）。
    ///
    /// 默认委托 [`description`](TypedTool::description)（默认语言英文），保证未覆盖的工具
    /// ——包括无法按 lang 切换的 MCP 动态工具——自动优雅降级。需要双语的工具覆盖此方法，
    /// 从 `share::i18n::tools::xxx(lang)` 取文案。
    fn description_for(&self, lang: &str) -> Cow<'_, str> {
        let _ = lang;
        Cow::Borrowed(self.description())
    }

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
        false
    }

    fn timeout_secs(&self) -> u64 {
        120
    }

    fn cancellation(&self) -> crate::domain::published_language::CancellationDeclaration {
        crate::domain::published_language::CancellationDeclaration::NonCooperative
    }

    fn is_input_safe(&self, _input: &Value) -> bool {
        false
    }

    /// Parse a validated input into a typed suspension. Returning `Some` makes
    /// the execution adapter skip [`TypedTool::call`].
    fn suspension(&self, _input: &Value) -> Option<Result<ToolSuspension, String>> {
        None
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

    fn description_for(&self, lang: &str) -> Cow<'_, str> {
        self.0.description_for(lang)
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

    fn cancellation(&self) -> crate::domain::published_language::CancellationDeclaration {
        self.0.cancellation()
    }

    fn is_input_safe(&self, input: &Value) -> bool {
        self.0.is_input_safe(input)
    }

    fn suspension(&self, input: &Value) -> Option<Result<ToolSuspension, String>> {
        self.0.suspension(input)
    }

    async fn call(&self, input: Value, ctx: &ToolExecutionContext) -> ToolResult {
        let result = self.0.call(input, ctx).await;
        let data = match &result.data {
            Some(data) => serde_json::to_value(data)
                .expect("TypedToolAdapter: data serialization should not fail"),
            None => Value::Null,
        };
        ToolResult {
            text: result.text,
            data,
            is_error: result.is_error,
            error_kind: result.error_kind,
            images: result.images,
        }
    }
}
