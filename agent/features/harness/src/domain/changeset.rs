//! ChangeSet 编辑契约：omitted-is-inherited 与「校验失败整份作废」。
//!
//! - 提案只携带变更项：未列出的组件自动继承当前版本；
//! - 应用前统一预检（Upsert 逐条 `validate`、Delete 校验存在性），
//!   任一条失败则**整份**作废（含合法条目），NEVER 部分采纳。

use crate::application::ComponentRegistry;
use crate::constants::ERROR_DOMAIN;
use crate::domain::component::{ComponentContent, ComponentId};
use crate::domain::errors::HarnessError;

/// 单条组件编辑。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ComponentEdit {
    /// 新建或更新组件。
    Upsert {
        id: ComponentId,
        content: ComponentContent,
    },
    /// 删除组件（不存在则整份作废）。
    Delete { id: ComponentId },
}

/// 组件变更集（RSI 提案与热载 diff 的载体）。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChangeSet {
    pub edits: Vec<ComponentEdit>,
}

/// 已成功应用的条目（按输入顺序返回）。
#[derive(Debug, Clone, PartialEq)]
pub enum AppliedEdit {
    Upserted { id: ComponentId },
    Deleted { id: ComponentId },
}

/// 预检单条 edit（不落盘）。
fn precheck_edit(
    registry: &dyn ComponentRegistry,
    edit: &ComponentEdit,
) -> Result<(), HarnessError> {
    match edit {
        ComponentEdit::Upsert { id, content } => registry.validate(id, content).map_err(|error| {
            HarnessError::invalid(
                ERROR_DOMAIN,
                format!("条目校验失败 {id}：{}", error.message()),
            )
        }),
        ComponentEdit::Delete { id } => {
            let exists = registry.list().contains(id);
            if exists {
                Ok(())
            } else {
                Err(HarnessError::invalid(
                    ERROR_DOMAIN,
                    format!("条目校验失败 {id}：组件不存在，无法删除"),
                ))
            }
        }
    }
}

/// 应用变更集：统一预检 → 全部通过才统一写入。
///
/// 任一预检失败返回 [`HarnessError`]（Invalid），registry 状态保持不变。
pub fn apply_change_set(
    registry: &mut dyn ComponentRegistry,
    change_set: &ChangeSet,
) -> Result<Vec<AppliedEdit>, HarnessError> {
    for edit in &change_set.edits {
        precheck_edit(registry, edit)?;
    }

    let mut applied = Vec::with_capacity(change_set.edits.len());
    for edit in &change_set.edits {
        match edit {
            ComponentEdit::Upsert { id, content } => registry.write(id, content)?,
            ComponentEdit::Delete { id } => registry.delete(id)?,
        }
        applied.push(match edit {
            ComponentEdit::Upsert { id, .. } => AppliedEdit::Upserted { id: id.clone() },
            ComponentEdit::Delete { id } => AppliedEdit::Deleted { id: id.clone() },
        });
    }
    Ok(applied)
}
