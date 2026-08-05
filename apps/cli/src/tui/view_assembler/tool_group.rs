#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolGroupKind {
    Explore,
    Run,
    Write,
    Tasks,
}

impl ToolGroupKind {
    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::Explore => "Explore",
            Self::Run => "Run",
            Self::Write => "Write",
            Self::Tasks => "Tasks",
        }
    }
}

pub(crate) fn classify_tool_name(tool_name: &str) -> Option<ToolGroupKind> {
    match tool_name {
        "Read" | "Glob" | "Grep" => Some(ToolGroupKind::Explore),
        "Bash" => Some(ToolGroupKind::Run),
        "Write" | "Edit" => Some(ToolGroupKind::Write),
        "TaskCreate" | "TaskUpdate" | "TaskBlockBy" | "TaskListGet" | "TaskLists"
        | "TaskListCreate" | "TaskListComplete" | "TaskGet" | "TaskStop" => {
            Some(ToolGroupKind::Tasks)
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "tool_group_tests.rs"]
mod tests;
