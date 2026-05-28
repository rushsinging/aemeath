//! 自动补全相关类型定义

use std::path::PathBuf;

/// 单个补全建议项
#[derive(Debug, Clone)]
pub struct Suggestion {
    /// 唯一标识
    pub _id: String,
    /// 显示文本
    pub display_text: String,
    /// 可选描述
    pub _description: Option<String>,
    /// 建议类型
    pub suggestion_type: SuggestionType,
}

/// 建议类型枚举
#[derive(Debug, Clone, PartialEq)]
pub enum SuggestionType {
    Command,
    File,
    Directory,
    Model,
    Session,
}

/// 生成建议所需的上下文
#[derive(Debug)]
pub struct SuggestionContext {
    /// 完整输入文本
    pub input: String,
    /// 输入中的光标位置
    pub cursor_offset: usize,
    /// 当前工作目录
    pub cwd: PathBuf,
    /// 可用模型列表（provider_name, model_id）
    pub models: Vec<(String, String)>,
    /// 可用技能列表（name, description, aliases）
    pub skills: Vec<(String, String, Vec<String>)>,
    /// 可用命令列表（name, description, aliases）— 从 CommandRegistry 动态获取
    pub commands: Vec<(String, String, Vec<String>)>,
    /// 最近 session 列表（id, summary）— 用于 /resume 补全
    pub sessions: Vec<(String, String)>,
}

/// 补全触发类型
#[derive(Debug, Clone, PartialEq)]
pub enum TriggerType {
    SlashCommand,
    AtSymbol,
    ModelArg,
    ModelSubCommand,
    ResumeArg,
}
