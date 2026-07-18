/// 单个日志上下文字段在 child scope 中的覆盖语义。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum FieldPatch<T> {
    /// 保留父 scope 的已解析值。
    #[default]
    Inherit,
    /// 将字段覆盖为指定值。
    Set(T),
    /// 在 child scope 中显式清空字段。
    Clear,
}

impl<T: Clone> FieldPatch<T> {
    fn resolve(self, parent: &Option<T>) -> Option<T> {
        match self {
            Self::Inherit => parent.clone(),
            Self::Set(value) => Some(value),
            Self::Clear => None,
        }
    }
}

/// 七个执行相关日志字段的不可变已解析快照。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LogContext {
    pub session_id: Option<String>,
    pub chat_id: Option<String>,
    pub turn: Option<usize>,
    pub request_id: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub role: Option<String>,
}

/// 创建 child scope 时对七个执行字段的增量覆盖。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LogContextPatch {
    pub session_id: FieldPatch<String>,
    pub chat_id: FieldPatch<String>,
    pub turn: FieldPatch<usize>,
    pub request_id: FieldPatch<String>,
    pub model: FieldPatch<String>,
    pub provider: FieldPatch<String>,
    pub role: FieldPatch<String>,
}

impl LogContext {
    /// 从当前不可变快照派生 child scope，不修改父 context。
    pub fn patched(&self, patch: LogContextPatch) -> Self {
        Self {
            session_id: patch.session_id.resolve(&self.session_id),
            chat_id: patch.chat_id.resolve(&self.chat_id),
            turn: patch.turn.resolve(&self.turn),
            request_id: patch.request_id.resolve(&self.request_id),
            model: patch.model.resolve(&self.model),
            provider: patch.provider.resolve(&self.provider),
            role: patch.role.resolve(&self.role),
        }
    }
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
