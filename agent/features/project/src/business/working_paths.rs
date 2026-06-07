use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::git_ops::GitWorktreeOps;

pub fn new_working_paths(cwd: PathBuf) -> (PathBuf, Arc<Mutex<PathBuf>>, Arc<Mutex<PathBuf>>) {
    let working_root = Arc::new(Mutex::new(cwd.clone()));
    let path_base = Arc::new(Mutex::new(cwd.clone()));
    (cwd, working_root, path_base)
}

pub fn current_path(path: &Arc<Mutex<PathBuf>>) -> PathBuf {
    path.lock()
        .map(|p| p.clone())
        .unwrap_or_else(|e| e.into_inner().clone())
}

pub fn set_working_directory(
    working_root: &Arc<Mutex<PathBuf>>,
    path_base: &Arc<Mutex<PathBuf>>,
    path: PathBuf,
) {
    let detected_root = detect_working_root(&path);
    set_path(working_root, detected_root);
    set_path(path_base, path);
}

fn set_path(target: &Arc<Mutex<PathBuf>>, path: PathBuf) {
    match target.lock() {
        Ok(mut current) => *current = path,
        Err(poisoned) => *poisoned.into_inner() = path,
    }
}

fn detect_working_root(path: &std::path::Path) -> PathBuf {
    crate::business::git_ops::GitCli
        .show_toplevel(path)
        .unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_working_paths_initializes_all_paths_from_cwd() {
        let cwd = PathBuf::from("/tmp/project");
        let (returned_cwd, working_root, path_base) = new_working_paths(cwd.clone());

        assert_eq!(returned_cwd, cwd);
        assert_eq!(current_path(&working_root), cwd);
        assert_eq!(current_path(&path_base), PathBuf::from("/tmp/project"));
    }

    #[test]
    fn test_current_path_reads_poisoned_mutex() {
        let path = Arc::new(Mutex::new(PathBuf::from("/tmp/project")));
        let cloned = Arc::clone(&path);
        let _ = std::panic::catch_unwind(move || {
            let _guard = cloned.lock().unwrap();
            panic!("poison mutex");
        });

        assert_eq!(current_path(&path), PathBuf::from("/tmp/project"));
    }

    #[test]
    fn test_set_working_directory_updates_path_base() {
        let cwd = PathBuf::from("/tmp/project");
        let (_, working_root, path_base) = new_working_paths(cwd);
        let next = PathBuf::from("/tmp/project/subdir");

        set_working_directory(&working_root, &path_base, next.clone());

        assert_eq!(current_path(&path_base), next);
    }
}
