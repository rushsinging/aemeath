use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub struct Report {
    pub changed: usize,
    pub covered: usize,
    pub missing: Vec<String>,
}

#[derive(Deserialize)]
struct Export {
    data: Vec<Data>,
}
#[derive(Deserialize)]
struct Data {
    files: Vec<File>,
}
#[derive(Deserialize)]
struct File {
    filename: String,
    segments: Vec<(u64, u64, u64, bool, bool, bool)>,
}

pub fn parse_diff(diff: &str) -> Result<BTreeMap<String, Vec<u64>>> {
    let mut result = BTreeMap::new();
    let mut file = None;
    let mut next = 0;
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            file = Some(path.to_owned());
        } else if line.starts_with("@@") {
            let added = line
                .split_whitespace()
                .find(|part| part.starts_with('+'))
                .context("diff hunk 缺少新增范围")?;
            next = added[1..]
                .split(',')
                .next()
                .context("新增范围为空")?
                .parse()?;
        } else if line.starts_with('+') && !line.starts_with("+++") {
            if let Some(path) = &file {
                result
                    .entry(path.clone())
                    .or_insert_with(Vec::new)
                    .push(next);
            }
            next += 1;
        } else if !line.starts_with('-') && !line.starts_with("\\") && next > 0 {
            next += 1;
        }
    }
    Ok(result)
}

pub fn report(json: &str, changed: &BTreeMap<String, Vec<u64>>, root: &Path) -> Result<Report> {
    let export: Export = serde_json::from_str(json)?;
    let mut covered_lines: BTreeMap<String, BTreeSet<u64>> = BTreeMap::new();
    for data in export.data {
        for file in data.files {
            let path = Path::new(&file.filename)
                .strip_prefix(root)
                .unwrap_or(Path::new(&file.filename))
                .to_string_lossy()
                .to_string();
            for (line, _, count, has_count, is_entry, _) in file.segments {
                if has_count && is_entry && count > 0 {
                    covered_lines.entry(path.clone()).or_default().insert(line);
                }
            }
        }
    }
    let mut total = 0;
    let mut covered = 0;
    let mut missing = Vec::new();
    for (path, lines) in changed {
        for line in lines {
            total += 1;
            if covered_lines
                .get(path)
                .is_some_and(|set| set.contains(line))
            {
                covered += 1
            } else {
                missing.push(format!("{path}:{line}"));
            }
        }
    }
    Ok(Report {
        changed: total,
        covered,
        missing,
    })
}
