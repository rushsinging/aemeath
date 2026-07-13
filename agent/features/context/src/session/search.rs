//! 会话搜索与过滤

use crate::session::storage::list_sessions;
use crate::session::types::{Session, SessionFilter};

/// Search sessions with filter criteria
pub async fn search_sessions(filter: &SessionFilter) -> Vec<Session> {
    let sessions = list_sessions().await;

    sessions
        .into_iter()
        .filter(|s| {
            // Title filter (partial match)
            if let Some(title) = &filter.title {
                let matches = s
                    .metadata
                    .title
                    .as_ref()
                    .map(|t| t.to_lowercase().contains(&title.to_lowercase()))
                    .unwrap_or(false);
                if !matches {
                    return false;
                }
            }

            // Tag filter (exact match)
            if let Some(tag) = &filter.tag {
                let tag_lower = tag.to_lowercase();
                if !s
                    .metadata
                    .tags
                    .iter()
                    .any(|t: &String| t.to_lowercase() == tag_lower)
                {
                    return false;
                }
            }

            // Project filter (partial match)
            if let Some(project) = &filter.project {
                let project_lower = project.to_lowercase();
                let matches = s
                    .metadata
                    .project
                    .as_ref()
                    .map(|p: &String| p.to_lowercase().contains(&project_lower))
                    .unwrap_or(false);
                if !matches {
                    return false;
                }
            }

            // Favorite filter
            if let Some(is_favorite) = filter.is_favorite {
                if s.metadata.is_favorite != is_favorite {
                    return false;
                }
            }

            // Model filter (exact match)
            if let Some(model) = &filter.model {
                if s.metadata
                    .model
                    .as_ref()
                    .map(|m| m != model)
                    .unwrap_or(true)
                {
                    return false;
                }
            }

            true
        })
        .collect()
}
