//! Catching new desktop items for the panel that has "Catch new desktop items"
//! on. This is the decision logic only: the UI feeds it the Desktop folder's
//! change notifications and the current time, and asks it which items are
//! due to be checked. Renames of items already there are not new items, and
//! empty new files and folders are left alone: they were most likely just
//! made from the desktop's New menu and are being named.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::folder::extname;

/// Partial downloads and temp files; the real item shows up under its own name when done.
const TEMP_EXTENSIONS: [&str; 7] =
    [".crdownload", ".part", ".partial", ".download", ".opdownload", ".tmp", ".temp"];
const IGNORED_NAMES: [&str; 2] = ["desktop.ini", "thumbs.db"];

/// How long after an item stops changing it counts as finished (downloads, copies).
pub const SETTLE_MS: u64 = 3000;
/// A name that appears this soon after another vanished is that item being renamed.
const RENAME_WINDOW_MS: u64 = 1500;
/// How often an item that couldn't be taken (still locked, say) is tried.
const MAX_ATTEMPTS: u32 = 3;
/// Bounds the pending checks if something floods the desktop with files.
const MAX_PENDING: usize = 200;
/// How long an item the app puts on the desktop itself is left alone.
pub const IGNORE_MS: u64 = 30_000;

fn key(name: &str) -> String {
    name.to_lowercase()
}

/// Whether a new desktop item with this name may be taken into a panel.
pub fn is_catchable(name: &str) -> bool {
    let lower = name.to_lowercase();
    !IGNORED_NAMES.contains(&lower.as_str())
        && !lower.starts_with("~$")
        && !TEMP_EXTENSIONS.contains(&extname(&lower))
        && !name.contains(['\\', '/'])
}

/// A change in the Desktop folder, as ReadDirectoryChangesW reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// Added, or the new name of a rename.
    Appeared(String),
    /// Removed, or the old name of a rename.
    Vanished(String),
    Modified(String),
}

struct Pending {
    name: String,
    due: u64,
    attempt: u32,
}

pub struct Catcher {
    dir: PathBuf,
    settle_ms: u64,
    /// Names on the desktop, to tell renames from new items.
    known: HashSet<String>,
    last_vanished: Option<u64>,
    /// Items being taken into a panel; their vanishing is not half of a rename.
    moving_out: HashSet<String>,
    pending: HashMap<String, Pending>,
    /// Names the app itself put on the desktop → when to stop ignoring them.
    ignored: HashMap<String, u64>,
}

impl Catcher {
    /// Starts from what's on the desktop now; none of it is new.
    pub fn new(dir: PathBuf, settle_ms: u64) -> Self {
        let known = std::fs::read_dir(&dir)
            .map(|entries| {
                entries.flatten().map(|e| key(&e.file_name().to_string_lossy())).collect()
            })
            .unwrap_or_default();
        Self {
            dir,
            settle_ms,
            known,
            last_vanished: None,
            moving_out: HashSet::new(),
            pending: HashMap::new(),
            ignored: HashMap::new(),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Leaves an item the app puts on the desktop itself alone for a while.
    pub fn ignore(&mut self, name: &str, ms: u64, now: u64) {
        self.ignored.retain(|_, until| *until > now);
        self.ignored.insert(key(name), now + ms);
    }

    pub fn saw(&mut self, change: Change, now: u64) {
        match change {
            Change::Vanished(name) => {
                let k = key(&name);
                // A finished download (.crdownload renamed to its real name) is
                // new, and so is anything that appears while an item moves out.
                if self.known.remove(&k) && is_catchable(&name) && !self.moving_out.remove(&k) {
                    self.last_vanished = Some(now);
                }
            }
            Change::Appeared(name) => {
                let k = key(&name);
                if !self.known.insert(k) {
                    return;
                }
                let renamed = self.last_vanished.is_some_and(|t| now - t < RENAME_WINDOW_MS);
                if !renamed && is_catchable(&name) {
                    self.schedule(name, 1, now);
                }
            }
            // Writes to items that aren't new don't matter; a new one settles later.
            Change::Modified(name) => {
                if self.pending.contains_key(&key(&name)) && is_catchable(&name) {
                    self.schedule(name, 1, now);
                }
            }
        }
    }

    fn schedule(&mut self, name: String, attempt: u32, now: u64) {
        let k = key(&name);
        if !self.pending.contains_key(&k) && self.pending.len() >= MAX_PENDING {
            return;
        }
        let due = now + self.settle_ms * attempt as u64;
        self.pending.insert(k, Pending { name, due, attempt });
    }

    /// When the next item is due to be checked, for the UI's one-shot timer.
    pub fn next_due(&self) -> Option<u64> {
        self.pending.values().map(|p| p.due).min()
    }

    /// Checks every item that has settled and hands the ones to take to
    /// `take`, which copies or moves them into the catching panel and returns
    /// false when that failed and is worth another try.
    pub fn run_due(&mut self, now: u64, mut take: impl FnMut(&Path) -> bool) {
        let mut due: Vec<Pending> = Vec::new();
        self.pending.retain(|_, p| {
            if p.due > now {
                return true;
            }
            due.push(Pending { name: std::mem::take(&mut p.name), ..*p });
            false
        });
        due.sort_by_key(|p| p.due);
        for Pending { name, attempt, .. } in due {
            let k = key(&name);
            if self.ignored.get(&k).is_some_and(|until| *until > now) {
                continue;
            }
            let path = self.dir.join(&name);
            // Gone already, or empty and probably still being named.
            if is_empty(&path).unwrap_or(true) {
                continue;
            }
            self.moving_out.insert(k.clone());
            let handled = take(&path);
            // Copied, or failed: the item stays, so it won't vanish and clear this.
            if !handled || path.exists() {
                self.moving_out.remove(&k);
            }
            if !handled && attempt < MAX_ATTEMPTS {
                self.schedule(name, attempt + 1, now);
            }
        }
    }
}

fn is_empty(path: &Path) -> std::io::Result<bool> {
    let meta = std::fs::metadata(path)?;
    Ok(if meta.is_dir() { std::fs::read_dir(path)?.next().is_none() } else { meta.len() == 0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETTLE: u64 = 100;

    struct Desk {
        dir: PathBuf,
        catcher: Catcher,
        now: u64,
        caught: Vec<String>,
    }

    impl Desk {
        fn new() -> Self {
            let dir =
                std::env::temp_dir().join(format!("sorter-desk-{}", crate::config::random_uuid()));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("existing.txt"), "x").unwrap();
            let catcher = Catcher::new(dir.clone(), SETTLE);
            Self { dir, catcher, now: 1_000_000, caught: Vec::new() }
        }

        fn write(&mut self, name: &str, content: &str) {
            std::fs::write(self.dir.join(name), content).unwrap();
            self.catcher.saw(Change::Appeared(name.into()), self.now);
            self.catcher.saw(Change::Modified(name.into()), self.now);
        }

        fn rename(&mut self, from: &str, to: &str) {
            std::fs::rename(self.dir.join(from), self.dir.join(to)).unwrap();
            self.catcher.saw(Change::Vanished(from.into()), self.now);
            self.catcher.saw(Change::Appeared(to.into()), self.now);
        }

        /// Lets time pass, checking items as the UI's timer would. Takes copy
        /// the item (it stays on the desktop), like the default add mode.
        fn settle(&mut self) {
            for _ in 0..6 {
                self.now += SETTLE;
                let caught = &mut self.caught;
                self.catcher.run_due(self.now, |p| {
                    caught.push(p.file_name().unwrap().to_string_lossy().into_owned());
                    true
                });
            }
        }
    }

    impl Drop for Desk {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn takes_ordinary_items() {
        assert!(is_catchable("Steam.lnk"));
        assert!(is_catchable("report.pdf"));
        assert!(is_catchable("Photos"));
    }

    #[test]
    fn skips_partial_downloads_temp_and_system_files() {
        for name in
            ["setup.exe.crdownload", "movie.mkv.part", "x.TMP", "~$report.docx", "desktop.ini"]
        {
            assert!(!is_catchable(name), "{name}");
        }
    }

    #[test]
    fn hands_over_new_items_once_they_settle() {
        let mut d = Desk::new();
        d.write("new.txt", "hello");
        assert_eq!(d.catcher.next_due(), Some(d.now + SETTLE));
        d.settle();
        assert_eq!(d.caught, ["new.txt"]);
        assert_eq!(d.catcher.next_due(), None);
    }

    #[test]
    fn waits_while_an_item_keeps_changing() {
        let mut d = Desk::new();
        d.write("big.zip", "part");
        d.now += SETTLE / 2;
        d.catcher.saw(Change::Modified("big.zip".into()), d.now);
        assert_eq!(d.catcher.next_due(), Some(d.now + SETTLE));
    }

    #[test]
    fn catches_a_finished_download_but_not_its_partial_file() {
        let mut d = Desk::new();
        d.write("setup.exe.crdownload", "data");
        d.settle();
        d.rename("setup.exe.crdownload", "setup.exe");
        d.settle();
        assert_eq!(d.caught, ["setup.exe"]);
    }

    #[test]
    fn leaves_renamed_items_empty_new_items_and_ignored_names_alone() {
        let mut d = Desk::new();
        d.rename("existing.txt", "renamed.txt");
        // Past the rename window, so the rest are judged on their own.
        d.now += RENAME_WINDOW_MS;
        d.write("empty.txt", "");
        std::fs::create_dir(d.dir.join("New folder")).unwrap();
        d.catcher.saw(Change::Appeared("New folder".into()), d.now);
        d.catcher.ignore("sent-back.txt", IGNORE_MS, d.now);
        d.write("sent-back.txt", "x");
        d.settle();
        assert!(d.caught.is_empty(), "{:?}", d.caught);
    }

    #[test]
    fn hands_over_a_copied_item_once_and_catches_the_next_one() {
        let mut d = Desk::new();
        d.write("one.txt", "x");
        d.settle();
        d.write("two.txt", "x");
        d.settle();
        assert_eq!(d.caught, ["one.txt", "two.txt"]);
    }

    #[test]
    fn tries_again_when_taking_fails() {
        let mut d = Desk::new();
        d.write("locked.txt", "x");
        let mut tries = 0;
        for _ in 0..20 {
            d.now += SETTLE;
            d.catcher.run_due(d.now, |_| {
                tries += 1;
                false
            });
        }
        assert_eq!(tries, MAX_ATTEMPTS);
    }

    #[test]
    fn an_item_moved_out_is_not_half_of_a_rename() {
        let mut d = Desk::new();
        d.write("moved.txt", "x");
        d.now += SETTLE;
        let dir = d.dir.clone();
        d.catcher.run_due(d.now, |p| {
            std::fs::remove_file(p).unwrap();
            true
        });
        d.catcher.saw(Change::Vanished("moved.txt".into()), d.now);
        // A new item right after the move is still new.
        d.write("next.txt", "x");
        d.settle();
        assert_eq!(d.caught, ["next.txt"]);
        assert!(!dir.join("moved.txt").exists());
    }
}
