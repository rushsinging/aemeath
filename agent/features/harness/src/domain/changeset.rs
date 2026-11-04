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
    /// 对现有组件正文做锚点补丁；格式无关，只要求 old/anchor 唯一。
    Patch {
        id: ComponentId,
        operations: Vec<PatchOperation>,
    },
    /// 删除组件（不存在则整份作废）。
    Delete { id: ComponentId },
}

/// 单条补丁操作。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum PatchOperation {
    /// 替换唯一旧片段。
    ReplaceExact {
        old_string: String,
        new_string: String,
    },
    /// 在唯一定位锚点后插入文本。
    InsertAfterAnchor { anchor: String, new_text: String },
    /// 删除唯一旧片段。
    DeleteExact { old_string: String },
}

/// 组件变更集（RSI 提案与热载 diff 的载体）。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChangeSet {
    pub edits: Vec<ComponentEdit>,
}

impl ComponentEdit {
    pub fn id(&self) -> &ComponentId {
        match self {
            Self::Upsert { id, .. } | Self::Patch { id, .. } | Self::Delete { id } => id,
        }
    }

    pub fn action(&self) -> &'static str {
        match self {
            Self::Upsert { .. } => "upsert",
            Self::Patch { .. } => "patch",
            Self::Delete { .. } => "delete",
        }
    }
}

/// 已成功应用的条目（按输入顺序返回）。
#[derive(Debug, Clone, PartialEq)]
pub enum AppliedEdit {
    Upserted { id: ComponentId },
    Patched { id: ComponentId },
    Deleted { id: ComponentId },
}

pub(crate) fn apply_patch_operations(
    body: &str,
    operations: &[PatchOperation],
) -> Result<String, HarnessError> {
    let mut output = body.to_string();
    for operation in operations {
        match operation {
            PatchOperation::ReplaceExact {
                old_string,
                new_string,
            } => {
                let count = output.match_indices(old_string).count();
                if old_string.is_empty() || count != 1 {
                    return Err(HarnessError::invalid(
                        ERROR_DOMAIN,
                        format!("patch old_string MUST 唯一且非空，实际匹配 {count} 次"),
                    ));
                }
                output = output.replacen(old_string, new_string, 1);
            }
            PatchOperation::InsertAfterAnchor { anchor, new_text } => {
                let count = output.match_indices(anchor).count();
                if anchor.is_empty() || count != 1 {
                    return Err(HarnessError::invalid(
                        ERROR_DOMAIN,
                        format!("patch anchor MUST 唯一且非空，实际匹配 {count} 次"),
                    ));
                }
                let index = output.find(anchor).expect("count 已确认") + anchor.len();
                output.insert_str(index, new_text);
            }
            PatchOperation::DeleteExact { old_string } => {
                let count = output.match_indices(old_string).count();
                if old_string.is_empty() || count != 1 {
                    return Err(HarnessError::invalid(
                        ERROR_DOMAIN,
                        format!("patch old_string MUST 唯一且非空，实际匹配 {count} 次"),
                    ));
                }
                output = output.replacen(old_string, "", 1);
            }
        }
    }
    Ok(output)
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
        ComponentEdit::Patch { id, operations } => {
            let content = registry.read(id)?;
            let patched = apply_patch_operations(&content.body, operations)?;
            let patched_content = ComponentContent {
                frontmatter: content.frontmatter,
                body: patched,
            };
            registry.validate(id, &patched_content).map_err(|error| {
                HarnessError::invalid(
                    ERROR_DOMAIN,
                    format!("patch 校验失败 {id}：{}", error.message()),
                )
            })
        }
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

    // 写前快照：任一后续写入失败时按逆序恢复，避免多条目半采纳。
    let backups: Vec<(ComponentId, Option<String>)> = change_set
        .edits
        .iter()
        .map(|edit| {
            let id = edit.id().clone();
            let raw = registry.read_raw(&id).ok();
            (id, raw)
        })
        .collect();

    let mut applied = Vec::with_capacity(change_set.edits.len());
    for edit in &change_set.edits {
        let result = match edit {
            ComponentEdit::Upsert { id, content } => registry.write(id, content),
            ComponentEdit::Patch { id, operations } => {
                let content = registry.read(id)?;
                let patched = apply_patch_operations(&content.body, operations)?;
                let patched_content = ComponentContent {
                    frontmatter: content.frontmatter,
                    body: patched,
                };
                registry.validate(id, &patched_content)?;
                registry.write(id, &patched_content)
            }
            ComponentEdit::Delete { id } => registry.delete(id),
        };
        if let Err(error) = result {
            for (id, raw) in backups.iter().rev() {
                match raw {
                    Some(raw) => {
                        if let Err(restore_error) = registry.write_raw(id, raw) {
                            log::error!(
                                target: crate::LOG_TARGET,
                                "组件事务回滚失败 {id}：{}（原始错误：{}）",
                                restore_error.message(),
                                error.message()
                            );
                        }
                    }
                    None => {
                        let _ = registry.delete(id);
                    }
                }
            }
            return Err(error);
        }
        applied.push(match edit {
            ComponentEdit::Upsert { id, .. } => AppliedEdit::Upserted { id: id.clone() },
            ComponentEdit::Patch { id, .. } => AppliedEdit::Patched { id: id.clone() },
            ComponentEdit::Delete { id } => AppliedEdit::Deleted { id: id.clone() },
        });
    }
    Ok(applied)
}
