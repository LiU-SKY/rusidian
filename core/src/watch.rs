//! Watches the vault folder so edits made elsewhere (Obsidian, sync tools,
//! the file explorer) update the index and reach the UI.

use crate::index::{VaultIndex, rel_string};
use crate::vault::is_hidden_path;
use notify_debouncer_mini::notify::{self, RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Events closer together than this are merged into one batch.
const DEBOUNCE: Duration = Duration::from_millis(250);

/// Stops watching when dropped.
pub struct VaultWatcher {
    _debouncer: Debouncer<RecommendedWatcher>,
}

/// Watch `root` recursively. For every batch of changes the index is
/// refreshed first, then `on_change` gets the changed vault-relative paths
/// (files and folders; hidden paths like `.obsidian/` are skipped).
pub fn watch<F>(
    root: PathBuf,
    index: Arc<RwLock<VaultIndex>>,
    on_change: F,
) -> Result<VaultWatcher, notify::Error>
where
    F: Fn(Vec<String>) + Send + 'static,
{
    let event_root = root.clone();
    let mut debouncer = new_debouncer(DEBOUNCE, move |result: DebounceEventResult| {
        let Ok(events) = result else { return };
        let mut changed = BTreeSet::new();
        {
            let mut idx = index.write().unwrap_or_else(|poisoned| poisoned.into_inner());
            for event in events {
                if !event.path.starts_with(&event_root) {
                    continue;
                }
                let rel = rel_string(&event_root, &event.path);
                if rel.is_empty() || is_hidden_path(&rel) {
                    continue;
                }
                idx.refresh_path(&event_root, &rel);
                changed.insert(rel);
            }
        }
        if !changed.is_empty() {
            on_change(changed.into_iter().collect());
        }
    })?;
    debouncer.watcher().watch(&root, RecursiveMode::Recursive)?;
    Ok(VaultWatcher { _debouncer: debouncer })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::mpsc;
    use std::time::Instant;

    /// Wait until a batch mentioning `path` arrives (or time out).
    fn wait_for(rx: &mpsc::Receiver<Vec<String>>, path: &str) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(batch) if batch.iter().any(|p| p == path) => return true,
                Ok(_) => continue,
                Err(_) => return false,
            }
        }
        false
    }

    #[test]
    fn external_changes_update_index_and_notify() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fs::write(root.join("Target.md"), "# target").unwrap();
        let index = Arc::new(RwLock::new(VaultIndex::build(&root).unwrap()));

        let (tx, rx) = mpsc::channel();
        let _watcher = watch(root.clone(), index.clone(), move |paths| {
            let _ = tx.send(paths);
        })
        .unwrap();

        // Created elsewhere
        fs::write(root.join("Linker.md"), "points to [[Target]]").unwrap();
        assert!(wait_for(&rx, "Linker.md"), "no event for created file");
        assert_eq!(index.read().unwrap().backlinks("Target.md").len(), 1);

        // Edited elsewhere
        fs::write(root.join("Linker.md"), "link removed").unwrap();
        assert!(wait_for(&rx, "Linker.md"), "no event for modified file");
        assert!(index.read().unwrap().backlinks("Target.md").is_empty());

        // Hidden folders never reach the callback
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        fs::write(root.join(".obsidian/workspace.json"), "{}").unwrap();

        // Deleted elsewhere. Under load a late event of the edit above can
        // arrive first, so wait for the index to show the deletion.
        fs::remove_file(root.join("Linker.md")).unwrap();
        assert!(wait_for(&rx, "Linker.md"), "no event for deleted file");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while index.read().unwrap().resolve("x.md", "Linker").is_some() && std::time::Instant::now() < deadline {
            let _ = rx.recv_timeout(Duration::from_millis(100));
        }
        let idx = index.read().unwrap();
        assert_eq!(idx.resolve("x.md", "Linker"), None);
        assert!(idx.search("workspace", 10).is_empty());
    }
}
