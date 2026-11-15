//! Running Git for vault sync (see `sync.rs` for what is synced).
//!
//! The repository lives outside the vault (`GIT_DIR` in the app's data
//! folder, `GIT_WORK_TREE` = the vault), so the vault gets no `.git`
//! folder. One sync: commit local changes → fetch → merge → push.
//!
//! Sync never loses a file on this device:
//! - both sides changed a file: this device's version stays, the other one
//!   is kept next to it as `name (충돌 …).ext` (no conflict markers);
//! - a file arriving where this device has an unsynced file with other
//!   content: the local one is first renamed to `name (충돌 … 이 기기).ext`;
//! - a folder taken off the sync list on another device: its files leave
//!   the repository but stay on this disk.

use crate::sync::{self, SyncConfig};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Marks commits that only stop syncing files (they are not deletions).
const UNTRACK_TRAILER: &str = "Rusidian-Untracked: yes";

pub struct Repo {
    pub git_dir: PathBuf,
    pub work_tree: PathBuf,
}

/// Installed Git's version, or `None` if Git can't be run.
pub fn git_version() -> Option<String> {
    let out = base_command().arg("--version").output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn base_command() -> Command {
    let mut c = Command::new("git");
    // Never wait for a terminal prompt (credentials come from Git's own helper).
    c.env("GIT_TERMINAL_PROMPT", "0");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

/// What one sync did.
#[derive(Debug, Clone, Default, serde::Serialize, PartialEq)]
pub struct Outcome {
    /// Local changes were committed.
    pub committed: bool,
    /// Files changed by the other device(s).
    pub pulled: Vec<String>,
    /// Copies made so that no version of a file was lost.
    pub conflicts: Vec<String>,
    pub pushed: bool,
    /// Files left out for being too large.
    pub large: Vec<String>,
}

impl Repo {
    /// The repository for `vault` under `data_dir` (one per vault).
    pub fn for_vault(data_dir: &Path, vault: &Path) -> Repo {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in vault.to_string_lossy().to_lowercase().bytes() {
            h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
        }
        Repo { git_dir: data_dir.join(format!("{h:016x}.git")), work_tree: vault.to_path_buf() }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut c = base_command();
        c.args(["-c", "core.quotepath=false"])
            .args(args)
            .env("GIT_DIR", &self.git_dir)
            .env("GIT_WORK_TREE", &self.work_tree)
            .current_dir(&self.work_tree);
        c
    }

    fn run(&self, args: &[&str]) -> Result<String, String> {
        let out = self.command(args).output().map_err(|e| format!("Git을 실행할 수 없습니다: {e}"))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            Err(if err.is_empty() { format!("git {} 실패", args.join(" ")) } else { err })
        }
    }

    fn ok(&self, args: &[&str]) -> bool {
        self.command(args).output().is_ok_and(|o| o.status.success())
    }

    fn bytes(&self, args: &[&str]) -> Option<Vec<u8>> {
        self.command(args).output().ok().filter(|o| o.status.success()).map(|o| o.stdout)
    }

    fn lines(&self, args: &[&str]) -> Result<Vec<String>, String> {
        Ok(self.run(args)?.lines().filter(|l| !l.is_empty()).map(str::to_string).collect())
    }

    /// Create the repository if needed and bring its settings up to date:
    /// allow-list (`info/exclude`), remote, identity. Returns the files
    /// left out for size.
    pub fn prepare(&self, cfg: &SyncConfig) -> Result<Vec<String>, String> {
        if !self.git_dir.join("HEAD").exists() {
            fs::create_dir_all(&self.git_dir).map_err(|e| e.to_string())?;
            self.run(&["init", "-q", "-b", &cfg.branch])?;
        }
        let large = self.write_rules(cfg)?;
        // Files stay byte for byte as they are (no line-ending conversion).
        self.run(&["config", "core.autocrlf", "false"])?;
        match self.run(&["remote", "get-url", "origin"]) {
            Ok(url) if url.trim() == cfg.remote => {}
            Ok(_) => {
                self.run(&["remote", "set-url", "origin", &cfg.remote])?;
            }
            Err(_) => {
                self.run(&["remote", "add", "origin", &cfg.remote])?;
            }
        }
        if self.run(&["config", "user.email"]).map(|e| e.trim().is_empty()).unwrap_or(true) {
            self.run(&["config", "user.name", "Rusidian"])?;
            self.run(&["config", "user.email", "rusidian@localhost"])?;
        }
        Ok(large)
    }

    /// Write the allow-list; returns the files left out for size.
    fn write_rules(&self, cfg: &SyncConfig) -> Result<Vec<String>, String> {
        let large: Vec<String> = sync::scan(&self.work_tree, &cfg.folders).large.into_iter().map(|(p, _)| p).collect();
        let info = self.git_dir.join("info");
        fs::create_dir_all(&info).map_err(|e| e.to_string())?;
        fs::write(info.join("exclude"), sync::exclude_rules(&cfg.folders, &large)).map_err(|e| e.to_string())?;
        Ok(large)
    }

    /// Stop tracking files the allow-list no longer covers, in a commit
    /// marked so other devices keep their copies.
    fn untrack_excluded(&self, stamp: &str) -> Result<bool, String> {
        let excluded = self.lines(&["ls-files", "-ci", "--exclude-standard"])?;
        if excluded.is_empty() {
            return Ok(false);
        }
        let mut args = vec!["rm", "-q", "--cached", "--"];
        args.extend(excluded.iter().map(String::as_str));
        self.run(&args)?;
        let message = format!("Rusidian 동기화 제외 {stamp}\n\n{UNTRACK_TRAILER}");
        self.run(&["commit", "-q", "-m", &message])?;
        Ok(true)
    }

    /// Before taking `target`: move aside this device's unsynced files that
    /// an incoming file would overwrite (Git replaces ignored files silently).
    fn protect_unsynced(&self, target: &str, has_local: bool, stamp: &str) -> Result<Vec<String>, String> {
        let incoming = if has_local {
            self.lines(&["diff", "--name-only", "--diff-filter=AM", "HEAD", target])?
        } else {
            self.lines(&["ls-tree", "-r", "--name-only", target])?
        };
        let mut copies = Vec::new();
        for path in incoming {
            let local = self.work_tree.join(&path);
            if !local.is_file() || self.ok(&["ls-files", "--error-unmatch", "--", &path]) {
                continue; // nothing there, or tracked (Git handles it)
            }
            let theirs = self.bytes(&["show", &format!("{target}:{path}")]);
            if fs::read(&local).ok() == theirs {
                continue; // the same file
            }
            let copy = sync::conflict_name(&path, &format!("{stamp} 이 기기"));
            fs::rename(&local, self.work_tree.join(&copy)).map_err(|e| format!("{path}을(를) 옮기지 못했습니다: {e}"))?;
            copies.push(copy);
        }
        Ok(copies)
    }

    /// After merging: files other devices stopped syncing were removed from
    /// the index and the disk; put them back on the disk (from `before`).
    fn restore_untracked(&self, before: &str) -> Result<(), String> {
        let commits = self.lines(&["log", "--format=%H", &format!("--grep={UNTRACK_TRAILER}"), "--fixed-strings", &format!("{before}..HEAD")])?;
        for commit in commits {
            for path in self.lines(&["diff-tree", "--no-commit-id", "--name-only", "-r", "--diff-filter=D", &commit])? {
                let local = self.work_tree.join(&path);
                if local.exists() {
                    continue;
                }
                if let Some(content) = self.bytes(&["show", &format!("{before}:{path}")]) {
                    if let Some(dir) = local.parent() {
                        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                    }
                    fs::write(&local, content).map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    }

    /// Commit, fetch, merge (keeping every version of a file), push.
    /// `load` gives the current settings; it is asked again after pulling,
    /// since the shared part (folders) may have arrived from another device.
    pub fn sync(&self, load: &dyn Fn() -> SyncConfig, stamp: &str) -> Result<Outcome, String> {
        let mut cfg = load();
        let mut out = Outcome { large: self.prepare(&cfg)?, ..Default::default() };

        out.committed |= self.untrack_excluded(stamp)?;
        self.run(&["add", "-A"])?;
        if !self.ok(&["diff", "--cached", "--quiet"]) {
            self.run(&["commit", "-q", "-m", &format!("Rusidian 동기화 {stamp}")])?;
            out.committed = true;
        }

        self.run(&["fetch", "-q", "origin"]).map_err(|e| format!("원격 저장소에서 받지 못했습니다: {e}"))?;
        let remote = format!("origin/{}", cfg.branch);
        let has_remote = self.ok(&["rev-parse", "--verify", "-q", &remote]);
        let has_local = self.ok(&["rev-parse", "--verify", "-q", "HEAD"]);
        if has_remote {
            out.conflicts = self.protect_unsynced(&remote, has_local, stamp)?;
            if !has_local {
                // Nothing here yet: take the other devices' history as is.
                self.run(&["checkout", "-q", "-B", &cfg.branch, &remote])?;
                out.pulled = self.lines(&["ls-files"])?;
            } else {
                let before = self.run(&["rev-parse", "HEAD"])?.trim().to_string();
                let message = format!("Rusidian 병합 {stamp}");
                if let Err(e) = self.run(&["merge", "-q", "--no-edit", "--allow-unrelated-histories", "-m", &message, &remote]) {
                    let copies = self.resolve_conflicts(stamp).map_err(|r| {
                        let _ = self.run(&["merge", "--abort"]);
                        format!("병합하지 못했습니다: {e} / {r}")
                    })?;
                    out.conflicts.extend(copies);
                    self.run(&["commit", "-q", "--no-edit"])?;
                }
                self.restore_untracked(&before)?;
                out.pulled = self.lines(&["diff", "--name-only", &before, "HEAD"])?;
            }
            // The shared settings may have changed: apply the new allow-list now,
            // so files another device stopped syncing stay out from here on.
            let fresh = load();
            if fresh.folders != cfg.folders {
                cfg = fresh;
                self.write_rules(&cfg)?;
                if self.untrack_excluded(stamp)? {
                    out.committed = true;
                }
            }
        }
        if self.ok(&["rev-parse", "--verify", "-q", "HEAD"]) {
            let ahead = !has_remote || !self.ok(&["merge-base", "--is-ancestor", "HEAD", &remote]);
            if ahead {
                self.run(&["push", "-q", "-u", "origin", &format!("HEAD:refs/heads/{}", cfg.branch)])
                    .map_err(|e| format!("원격 저장소에 올리지 못했습니다: {e}"))?;
                out.pushed = true;
            }
        }
        Ok(out)
    }

    /// For each conflicted file keep this device's version and save the
    /// other side's as a copy. Returns the copies made.
    fn resolve_conflicts(&self, stamp: &str) -> Result<Vec<String>, String> {
        let paths = self.lines(&["diff", "--name-only", "--diff-filter=U"])?;
        if paths.is_empty() {
            return Err("충돌한 파일을 찾지 못했습니다.".into());
        }
        let mut copies = Vec::new();
        for path in &paths {
            let ours = self.ok(&["cat-file", "-e", &format!(":2:{path}")]);
            let theirs = self.bytes(&["show", &format!(":3:{path}")]);
            match (ours, theirs) {
                (true, Some(t)) => {
                    let copy = sync::conflict_name(path, stamp);
                    fs::write(self.work_tree.join(&copy), &t).map_err(|e| e.to_string())?;
                    self.run(&["checkout", "--ours", "--", path])?;
                    self.run(&["add", "--", path, &copy])?;
                    copies.push(copy);
                }
                // Deleted here, changed there: keep their version.
                (false, Some(_)) => {
                    self.run(&["checkout", "--theirs", "--", path])?;
                    self.run(&["add", "--", path])?;
                }
                // Changed here, deleted there: keep ours.
                (true, None) => {
                    self.run(&["checkout", "--ours", "--", path])?;
                    self.run(&["add", "--", path])?;
                }
                (false, None) => {
                    self.run(&["rm", "-q", "--cached", "--", path])?;
                }
            }
        }
        Ok(copies)
    }

    #[cfg(test)]
    fn tracked(&self) -> Vec<String> {
        self.lines(&["ls-files"]).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Env {
        _tmp: tempfile::TempDir,
        remote: String,
        root: PathBuf,
    }

    /// A bare "server" repository; `None` when Git isn't installed.
    fn env() -> Option<Env> {
        git_version()?;
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        assert!(Command::new("git").args(["init", "-q", "--bare", "-b", "main"]).arg(&remote).status().unwrap().success());
        let root = tmp.path().to_path_buf();
        Some(Env { remote: remote.to_string_lossy().into_owned(), root, _tmp: tmp })
    }

    /// One device: a vault, its repository and its (changeable) folder list.
    struct Device {
        vault: PathBuf,
        repo: Repo,
        cfg: RefCell<SyncConfig>,
    }

    impl Device {
        fn new(env: &Env, name: &str, folders: &[&str]) -> Device {
            let vault = env.root.join(name);
            fs::create_dir_all(&vault).unwrap();
            let cfg = SyncConfig {
                remote: env.remote.clone(),
                branch: "main".into(),
                folders: folders.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            };
            Device { repo: Repo::for_vault(&env.root.join(format!("data-{name}")), &vault), vault, cfg: RefCell::new(cfg) }
        }
        fn write(&self, p: &str, c: &str) {
            let f = self.vault.join(p);
            fs::create_dir_all(f.parent().unwrap()).unwrap();
            fs::write(f, c).unwrap();
        }
        fn read(&self, p: &str) -> Option<String> {
            fs::read_to_string(self.vault.join(p)).ok()
        }
        fn sync(&self, stamp: &str) -> Outcome {
            self.repo.sync(&|| self.cfg.borrow().clone(), stamp).unwrap()
        }
    }

    /// Two devices syncing through a local bare repository, end to end.
    #[test]
    fn two_devices_sync_and_keep_both_sides_of_a_conflict() {
        let Some(env) = env() else { return };
        let a = Device::new(&env, "A", &["Notes"]);
        a.write("Notes/plan.md", "v1");
        a.write("Secret/pw.txt", "hunter2");
        a.write(".obsidian/app.json", "{}");
        let r = a.sync("1");
        assert!(r.committed && r.pushed);

        // B starts empty and receives exactly the allowed files.
        let b = Device::new(&env, "B", &["Notes"]);
        let r = b.sync("2");
        assert_eq!(r.pulled, [".obsidian/app.json", "Notes/plan.md"]);
        assert_eq!(b.read("Notes/plan.md").as_deref(), Some("v1"));
        assert!(!b.vault.join("Secret").exists(), "secret must not travel");

        // Both edit the same note; A syncs first.
        a.write("Notes/plan.md", "from A");
        b.write("Notes/plan.md", "from B");
        a.sync("3");
        let r = b.sync("2026-11-14 0400");
        assert_eq!(r.conflicts, ["Notes/plan (충돌 2026-11-14 0400).md"]);
        assert_eq!(b.read("Notes/plan.md").as_deref(), Some("from B"));
        assert_eq!(b.read("Notes/plan (충돌 2026-11-14 0400).md").as_deref(), Some("from A"));
        assert!(r.pushed);

        // A gets B's version and the copy; no conflict markers anywhere.
        let r = a.sync("4");
        assert!(r.conflicts.is_empty());
        assert_eq!(a.read("Notes/plan.md").as_deref(), Some("from B"));
        assert_eq!(a.read("Notes/plan (충돌 2026-11-14 0400).md").as_deref(), Some("from A"));
    }

    /// A new device that already has its own note at an incoming path keeps it.
    #[test]
    fn unsynced_local_files_are_not_overwritten() {
        let Some(env) = env() else { return };
        let a = Device::new(&env, "A", &["Notes"]);
        a.write("Notes/todo.md", "A's list");
        a.sync("1");

        // B has a different note at the same path, not synced yet.
        let b = Device::new(&env, "B", &[]);
        b.write("Notes/todo.md", "B's list");
        let r = b.sync("2");
        assert_eq!(r.conflicts, ["Notes/todo (충돌 2 이 기기).md"]);
        assert_eq!(b.read("Notes/todo.md").as_deref(), Some("A's list"));
        assert_eq!(b.read("Notes/todo (충돌 2 이 기기).md").as_deref(), Some("B's list"));
    }

    /// Taking a folder off the list on one device never deletes it elsewhere.
    #[test]
    fn unlisting_a_folder_keeps_files_on_every_device() {
        let Some(env) = env() else { return };
        let a = Device::new(&env, "A", &["Notes", "Work"]);
        a.write("Notes/n.md", "n");
        a.write("Work/w.md", "w");
        a.sync("1");
        let b = Device::new(&env, "B", &["Notes", "Work"]);
        b.sync("2");
        assert_eq!(b.read("Work/w.md").as_deref(), Some("w"));

        // A stops syncing Work (the shared list, as both devices would see it).
        a.cfg.borrow_mut().folders = vec!["Notes".into()];
        a.sync("3");
        assert_eq!(a.repo.tracked(), ["Notes/n.md"]);
        assert_eq!(a.read("Work/w.md").as_deref(), Some("w"));

        // B pulls that change: Work stays on B's disk and is no longer synced.
        b.cfg.borrow_mut().folders = vec!["Notes".into()];
        b.sync("4");
        assert_eq!(b.read("Work/w.md").as_deref(), Some("w"));
        assert_eq!(b.repo.tracked(), ["Notes/n.md"]);

        // Later edits to Work stay local.
        b.write("Work/w.md", "local only");
        b.sync("5");
        a.sync("6");
        assert_eq!(a.read("Work/w.md").as_deref(), Some("w"));
    }

    /// The shared list arriving in the same sync is applied at once.
    #[test]
    fn folder_list_from_another_device_applies_in_the_same_sync() {
        let Some(env) = env() else { return };
        let a = Device::new(&env, "A", &["Notes", "Work"]);
        a.write("Notes/n.md", "n");
        a.write("Work/w.md", "w");
        a.sync("1");
        let b = Device::new(&env, "B", &["Notes", "Work"]);
        b.sync("2");

        a.cfg.borrow_mut().folders = vec!["Notes".into()];
        a.sync("3");
        // B's settings change only when the pull brings them (simulated by
        // `load` returning the new list on its second call).
        let calls = RefCell::new(0);
        let load = || {
            *calls.borrow_mut() += 1;
            let mut c = b.cfg.borrow().clone();
            if *calls.borrow() > 1 {
                c.folders = vec!["Notes".into()];
            }
            c
        };
        b.repo.sync(&load, "4").unwrap();
        assert_eq!(b.read("Work/w.md").as_deref(), Some("w"));
        assert_eq!(b.repo.tracked(), ["Notes/n.md"]);
    }
}
