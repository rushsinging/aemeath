//! 组件版本快照与回滚（#2074）：采纳前备份旧内容，回滚恢复。
//!
//! 快照按 ComponentId 泛化（kind/name 分层），每 cycle 一文件：
//! `~/.agents/harness/versions/<kind>/<name>/<cycle_id>.json`。
//! 三形态：`Existing`（旧内容，upsert/delete 前）、`Tombstone`（新建前，
//! 回滚=删除该组件）。回滚统一走 registry 校验 + 原子写。

use crate::application::ComponentRegistry;
use crate::constants::ERROR_DOMAIN;
use crate::domain::component::ComponentId;
use crate::domain::errors::HarnessError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 快照内容：组件旧内容或 tombstone。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SnapshotContent {
    /// 组件旧**原始落盘文本**（逐字节回滚，NEVER 经 frontmatter 对象
    /// 往返重排 key）。
    Existing {
        raw: String,
    },
    Tombstone,
}

/// 单条组件快照。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComponentSnapshot {
    /// 组件稳定标识（kind/name 形态字符串）。
    pub component: String,
    pub cycle_id: String,
    pub taken_at_secs: u64,
    /// 触发该快照的编辑动作（"upsert" / "delete"）。
    pub action: String,
    pub payload: SnapshotContent,
}

/// 快照库：`versions/<kind>/<name>/<cycle_id>.json`。
pub struct ComponentVersionStore {
    root: PathBuf,
}

impl ComponentVersionStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn component_dir(&self, id: &ComponentId) -> PathBuf {
        self.root.join(id.kind.as_str()).join(id.name.as_str())
    }

    /// 采纳前保存快照（组件存在 → Existing；不存在 → Tombstone）。
    pub fn capture(
        &self,
        registry: &dyn ComponentRegistry,
        id: &ComponentId,
        cycle_id: &str,
        action: &str,
    ) -> Result<ComponentSnapshot, HarnessError> {
        let payload = match registry.read_raw(id) {
            Ok(raw) => SnapshotContent::Existing { raw },
            Err(_) => SnapshotContent::Tombstone,
        };
        let snapshot = ComponentSnapshot {
            component: format!("{id}"),
            cycle_id: cycle_id.to_string(),
            taken_at_secs: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or(0),
            action: action.to_string(),
            payload,
        };
        let dir = self.component_dir(id);
        std::fs::create_dir_all(&dir).map_err(|error| {
            HarnessError::storage(ERROR_DOMAIN, format!("创建版本目录失败：{error}"))
        })?;
        let path = dir.join(format!("{cycle_id}.json"));
        let text = serde_json::to_string(&snapshot).map_err(|error| {
            HarnessError::invalid(ERROR_DOMAIN, format!("快照序列化失败：{error}"))
        })?;
        std::fs::write(&path, text)
            .map_err(|error| HarnessError::storage(ERROR_DOMAIN, format!("写快照失败：{error}")))?;
        Ok(snapshot)
    }

    /// 列出某组件的历史快照（cycle_id 降序——最近在前）。
    pub fn history(&self, id: &ComponentId) -> Result<Vec<ComponentSnapshot>, HarnessError> {
        let dir = self.component_dir(id);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Ok(Vec::new());
        };
        let mut snapshots: Vec<ComponentSnapshot> = entries
            .flatten()
            .filter_map(|entry| {
                let text = std::fs::read_to_string(entry.path()).ok()?;
                serde_json::from_str::<ComponentSnapshot>(&text).ok()
            })
            .collect();
        snapshots.sort_by(|a, b| b.cycle_id.cmp(&a.cycle_id));
        Ok(snapshots)
    }

    /// 加载指定 cycle 的快照（None → 最近一个）。
    pub fn load(
        &self,
        id: &ComponentId,
        cycle_id: Option<&str>,
    ) -> Result<ComponentSnapshot, HarnessError> {
        match cycle_id {
            Some(cycle) => {
                let path = self.component_dir(id).join(format!("{cycle}.json"));
                let text = std::fs::read_to_string(&path).map_err(|error| {
                    HarnessError::storage(
                        ERROR_DOMAIN,
                        format!("读取快照失败 {}: {error}", path.display()),
                    )
                })?;
                serde_json::from_str(&text).map_err(|error| {
                    HarnessError::invalid(ERROR_DOMAIN, format!("快照解析失败：{error}"))
                })
            }
            None => {
                let history = self.history(id)?;
                history.into_iter().next().ok_or_else(|| {
                    HarnessError::invalid(ERROR_DOMAIN, format!("组件无历史快照：{id}"))
                })
            }
        }
    }
}

/// 恢复单个组件到指定快照（tombstone → 删除；Existing → 校验+写回）。
pub fn rollback_component(
    registry: &mut dyn ComponentRegistry,
    id: &ComponentId,
    snapshot: &ComponentSnapshot,
) -> Result<String, HarnessError> {
    match &snapshot.payload {
        SnapshotContent::Existing { raw } => {
            registry.write_raw(id, raw)?;
            Ok(format!("已恢复 {id} 至 cycle {}", snapshot.cycle_id))
        }
        SnapshotContent::Tombstone => {
            registry.delete(id)?;
            Ok(format!(
                "已删除 {id}（该组件在 cycle {} 前不存在）",
                snapshot.cycle_id
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::filesystem::FilesystemComponentRegistry;
    use crate::domain::component::{ComponentContent, ComponentKind, ComponentName};
    use std::collections::HashMap;
    use std::path::Path;

    fn guardrails_id() -> ComponentId {
        ComponentId {
            kind: ComponentKind::Guardrails,
            name: ComponentName::new("default"),
        }
    }

    fn store_with_component(
        dir: &Path,
    ) -> (ComponentVersionStore, FilesystemComponentRegistry, PathBuf) {
        let path = dir.join("guardrails.md");
        std::fs::write(&path, "旧内容\n").unwrap();
        let mut map: HashMap<ComponentId, PathBuf> = HashMap::new();
        map.insert(guardrails_id(), path.clone());
        let registry = FilesystemComponentRegistry::new(map);
        let store = ComponentVersionStore::new(dir.join("versions"));
        (store, registry, path)
    }

    #[test]
    fn capture_existing_and_rollback_restores_content() {
        let dir = tempfile::tempdir().unwrap();
        let (store, mut registry, path) = store_with_component(dir.path());
        let snapshot = store
            .capture(&registry, &guardrails_id(), "cycle-1", "upsert")
            .unwrap();
        assert!(matches!(snapshot.payload, SnapshotContent::Existing { .. }));
        // 模拟 RSI 改写
        let new_content = ComponentContent {
            frontmatter: None,
            body: "新内容 17 行\n第二行\n".to_string(),
        };
        registry.write(&guardrails_id(), &new_content).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("新内容"));
        // 回滚
        let loaded = store.load(&guardrails_id(), None).unwrap();
        rollback_component(&mut registry, &guardrails_id(), &loaded).unwrap();
        let restored = std::fs::read_to_string(&path).unwrap();
        assert!(restored.contains("旧内容"), "内容恢复：{restored}");
        assert!(!restored.contains("新内容"));
    }

    #[test]
    fn tombstone_rollback_deletes_new_component() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fresh.md");
        let map: HashMap<ComponentId, PathBuf> = HashMap::new();
        let mut registry = FilesystemComponentRegistry::new(map);
        let store = ComponentVersionStore::new(dir.path().join("versions"));
        let id = ComponentId {
            kind: ComponentKind::ToolGuidance,
            name: ComponentName::new("fresh"),
        };
        // 组件不存在 → tombstone
        let snapshot = store.capture(&registry, &id, "cycle-2", "upsert").unwrap();
        assert!(matches!(snapshot.payload, SnapshotContent::Tombstone));
        // 新建后回滚 → 删除
        let mut registry2 =
            FilesystemComponentRegistry::new(HashMap::from([(id.clone(), path.clone())]));
        registry2
            .write(
                &id,
                &ComponentContent {
                    frontmatter: None,
                    body: "new".to_string(),
                },
            )
            .unwrap();
        assert!(path.exists());
        rollback_component(&mut registry2, &id, &snapshot).unwrap();
        assert!(!path.exists(), "tombstone 回滚删除组件");
        // 原 registry 未被使用（保持类型一致）
        let _ = &mut registry;
    }

    /// 快照存**原始落盘文本**：不经 frontmatter 对象往返重排 key。
    #[test]
    fn snapshot_preserves_raw_layout() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("g.md");
        // name 在前（非 alphabet 序）
        let original = "---\nname: default\ndescription: 规则\n---\n正文\n";
        std::fs::write(&path, original).unwrap();
        let map: HashMap<ComponentId, PathBuf> = HashMap::from([(guardrails_id(), path.clone())]);
        let registry = FilesystemComponentRegistry::new(map);
        let store = ComponentVersionStore::new(dir.path().join("versions"));
        let snapshot = store
            .capture(&registry, &guardrails_id(), "cycle-raw", "upsert")
            .unwrap();
        match &snapshot.payload {
            SnapshotContent::Existing { raw } => {
                assert_eq!(raw, original, "原始字节布局 MUST 保留")
            }
            SnapshotContent::Tombstone => panic!("应为 Existing"),
        }
    }

    #[test]
    fn history_lists_descending_and_selects_by_cycle() {
        let dir = tempfile::tempdir().unwrap();
        let (store, registry, _path) = store_with_component(dir.path());
        store
            .capture(&registry, &guardrails_id(), "cycle-1", "upsert")
            .unwrap();
        store
            .capture(&registry, &guardrails_id(), "cycle-9", "upsert")
            .unwrap();
        let history = store.history(&guardrails_id()).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].cycle_id, "cycle-9", "降序：最近在前");
        let picked = store.load(&guardrails_id(), Some("cycle-1")).unwrap();
        assert_eq!(picked.cycle_id, "cycle-1");
    }
}
