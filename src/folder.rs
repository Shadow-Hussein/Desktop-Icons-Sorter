//! A panel's folder: listing and sorting items, keeping the custom order in
//! step, and collision-safe moves, copies and renames. Nothing is ever deleted.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::model::{MAX_PANEL_ITEMS, PlacedItem, SortBy};

/// Extensions Explorer hides on the desktop even with "show extensions" on.
const HIDDEN_EXTENSIONS: [&str; 3] = [".lnk", ".url", ".pif"];
/// Files that should never show up as panel items.
const IGNORED_NAMES: [&str; 3] = ["desktop.ini", "thumbs.db", ".ds_store"];

#[derive(Clone, Debug, PartialEq)]
pub struct PanelItem {
    /// File name on disk, including extension.
    pub file_name: String,
    /// Name shown under the icon (.lnk/.url extensions hidden, like Explorer).
    pub display_name: String,
    pub path: PathBuf,
    pub is_directory: bool,
    /// .lnk/.url/.pif: drawn with the shortcut arrow, like on the desktop.
    pub is_shortcut: bool,
    pub modified_ms: f64,
}

/// Node's `path.extname`: the last dot and what follows, unless the name starts with it.
pub fn extname(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i > 0 => &name[i..],
        _ => "",
    }
}

fn has_hidden_extension(name: &str) -> bool {
    let ext = extname(name).to_ascii_lowercase();
    HIDDEN_EXTENSIONS.contains(&ext.as_str())
}

pub fn is_shortcut(file_name: &str) -> bool {
    has_hidden_extension(file_name)
}

pub fn display_name_for(file_name: &str) -> String {
    if has_hidden_extension(file_name) {
        file_name[..file_name.len() - extname(file_name).len()].to_owned()
    } else {
        file_name.to_owned()
    }
}

/// Key for matching file names the way the file system does.
fn name_key(name: &str) -> String {
    name.to_lowercase()
}

/// An absolute, lower-cased form of `p` without a trailing separator.
fn normalized(p: &Path) -> String {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let s = abs.to_string_lossy().replace('/', "\\").to_lowercase();
    let trimmed = s.trim_end_matches('\\');
    // Keep the separator of a drive root ("c:\").
    if trimmed.ends_with(':') { s } else { trimmed.to_owned() }
}

pub fn same_path(a: &Path, b: &Path) -> bool {
    normalized(a) == normalized(b)
}

/// True when `child` is `parent` itself or somewhere below it.
pub fn is_inside(child: &Path, parent: &Path) -> bool {
    let (c, p) = (normalized(child), normalized(parent));
    c == p || c.strip_prefix(&p).is_some_and(|rest| p.ends_with('\\') || rest.starts_with('\\'))
}

/// Compares names like Explorer: case-insensitive, with runs of digits by value.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(ch) = it.next_if(char::is_ascii_digit) {
                        digits.push(ch);
                    }
                    digits
                };
                let (m, n) = (take(&mut x), take(&mut y));
                let (m, n) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                let ord = m.len().cmp(&n.len()).then_with(|| m.cmp(n));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(c), Some(d)) => {
                if c != d {
                    return c.cmp(&d);
                }
                x.next();
                y.next();
            }
        }
    }
}

/// Sorts items. `Custom` reads the spots in `cells` row by row, then follows
/// `order` (file names) for items without a spot, then names.
pub fn sort_items(
    items: &mut [PanelItem],
    sort_by: SortBy,
    order: &[String],
    cells: &[PlacedItem],
) {
    let by_name = |a: &PanelItem, b: &PanelItem| natural_cmp(&a.display_name, &b.display_name);
    match sort_by {
        SortBy::Custom => {
            let mut spots: Vec<&PlacedItem> = cells.iter().collect();
            spots.sort_by_key(|p| (p.row, p.col));
            let mut rank: HashMap<String, usize> = HashMap::new();
            for (i, p) in spots.iter().enumerate() {
                rank.entry(name_key(&p.name)).or_insert(i);
            }
            for (i, name) in order.iter().enumerate() {
                rank.entry(name_key(name)).or_insert(spots.len() + i);
            }
            let at =
                |i: &PanelItem| rank.get(&name_key(&i.file_name)).copied().unwrap_or(usize::MAX);
            items.sort_by(|a, b| at(a).cmp(&at(b)).then_with(|| by_name(a, b)));
        }
        SortBy::Type => items.sort_by(|a, b| {
            b.is_directory.cmp(&a.is_directory).then_with(|| {
                let ea = extname(&a.file_name).to_lowercase();
                let eb = extname(&b.file_name).to_lowercase();
                if ea == eb { by_name(a, b) } else { ea.cmp(&eb) }
            })
        }),
        SortBy::Modified => items
            .sort_by(|a, b| b.modified_ms.total_cmp(&a.modified_ms).then_with(|| by_name(a, b))),
        SortBy::Name => items.sort_by(by_name),
    }
}

/// The custom order for a folder now holding `names` (in display order): drops
/// names that are gone and appends new ones. None when nothing changed, so
/// callers only save real changes.
pub fn sync_order(order: &[String], names: &[String]) -> Option<Vec<String>> {
    let present: HashSet<String> = names.iter().map(|n| name_key(n)).collect();
    let kept: Vec<String> =
        order.iter().filter(|n| present.contains(&name_key(n))).cloned().collect();
    let known: HashSet<String> = kept.iter().map(|n| name_key(n)).collect();
    let mut next = kept;
    let kept_len = next.len();
    next.extend(names.iter().filter(|n| !known.contains(&name_key(n))).cloned());
    // Past the cap, the newest items go unlisted and sort by name after the rest.
    next.truncate(MAX_PANEL_ITEMS);
    if kept_len == order.len() && next.len() == order.len() {
        return None;
    }
    Some(next)
}

/// Moves `names` to just before `before` in `order` (or to the end when
/// `before` is None or not in it), adding any that aren't there yet.
pub fn place_in_order(order: &[String], names: &[String], before: Option<&str>) -> Vec<String> {
    let moving: HashSet<String> = names.iter().map(|n| name_key(n)).collect();
    let mut next: Vec<String> =
        order.iter().filter(|n| !moving.contains(&name_key(n))).cloned().collect();
    let at = before.and_then(|b| next.iter().position(|n| name_key(n) == name_key(b)));
    match at {
        Some(i) => {
            next.splice(i..i, names.iter().cloned());
        }
        None => next.extend(names.iter().cloned()),
    }
    if next.len() > MAX_PANEL_ITEMS {
        next.drain(..next.len() - MAX_PANEL_ITEMS);
    }
    next
}

/// Forgets the spots of items that are gone; None when none are.
pub fn prune_cells(cells: &[PlacedItem], names: &[String]) -> Option<Vec<PlacedItem>> {
    let present: HashSet<String> = names.iter().map(|n| name_key(n)).collect();
    let kept: Vec<PlacedItem> =
        cells.iter().filter(|p| present.contains(&name_key(&p.name))).cloned().collect();
    (kept.len() != cells.len()).then_some(kept)
}

/// Moves a renamed item's spot to its new name.
pub fn rename_in_cells(cells: &[PlacedItem], from: &str, to: &str) -> Vec<PlacedItem> {
    cells
        .iter()
        .map(|p| {
            if name_key(&p.name) == name_key(from) {
                PlacedItem { name: to.into(), ..p.clone() }
            } else {
                p.clone()
            }
        })
        .collect()
}

/// Replaces a renamed item's name in `order`, keeping its place.
pub fn rename_in_order(order: &[String], from: &str, to: &str) -> Vec<String> {
    order
        .iter()
        .map(|n| if name_key(n) == name_key(from) { to.to_owned() } else { n.clone() })
        .collect()
}

fn is_listed(name: &str) -> bool {
    !IGNORED_NAMES.contains(&name.to_lowercase().as_str()) && !name.starts_with("~$")
}

/// The folder's items, sorted. A missing folder is empty.
pub fn list_items(
    folder: &Path,
    sort_by: SortBy,
    order: &[String],
    cells: &[PlacedItem],
) -> io::Result<Vec<PanelItem>> {
    let entries = match std::fs::read_dir(folder) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut items: Vec<PanelItem> = entries
        .filter_map(|e| {
            let e = e.ok()?;
            let file_name = e.file_name().to_string_lossy().into_owned();
            if !is_listed(&file_name) {
                return None;
            }
            let path = e.path();
            // Broken symlinks and files removed while listing are skipped.
            let meta = std::fs::metadata(&path).ok()?;
            let modified_ms = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0.0, |d| d.as_secs_f64() * 1000.0);
            let is_directory = meta.is_dir();
            Some(PanelItem {
                display_name: display_name_for(&file_name),
                is_shortcut: !is_directory && is_shortcut(&file_name),
                file_name,
                path,
                is_directory,
                modified_ms,
            })
        })
        .collect();
    sort_items(&mut items, sort_by, order, cells);
    Ok(items)
}

fn exists(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok()
}

fn file_name_of(p: &Path) -> io::Result<String> {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))
}

/// A path in `dir` for `file_name` that doesn't exist yet: "name (2).ext", …
pub fn unique_destination(dir: &Path, file_name: &str) -> io::Result<PathBuf> {
    let candidate = dir.join(file_name);
    if !exists(&candidate) {
        return Ok(candidate);
    }
    let ext = extname(file_name);
    let stem = &file_name[..file_name.len() - ext.len()];
    (2..10_000)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|c| !exists(c))
        .ok_or_else(|| io::Error::other(format!("Could not find a free name for {file_name}")))
}

fn copy_recursive(src: &Path, dest: &Path) -> io::Result<()> {
    if std::fs::metadata(src)?.is_dir() {
        std::fs::create_dir(dest)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dest.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(src, dest).map(|_| ())
    }
}

fn parent_of(p: &Path) -> &Path {
    p.parent().unwrap_or(p)
}

/// Windows reports a rename to another drive as ERROR_NOT_SAME_DEVICE.
fn crosses_devices(e: &io::Error) -> bool {
    e.raw_os_error() == Some(17) || e.kind() == io::ErrorKind::CrossesDevices
}

/// Moves a file or folder into `dest_dir`, renaming on collision. Falls back to
/// copy + delete when the source is on another drive. Returns the new path.
pub fn move_into(src: &Path, dest_dir: &Path) -> io::Result<PathBuf> {
    move_into_with(src, dest_dir, |a, b| std::fs::rename(a, b))
}

fn move_into_with(
    src: &Path,
    dest_dir: &Path,
    rename: impl Fn(&Path, &Path) -> io::Result<()>,
) -> io::Result<PathBuf> {
    if same_path(parent_of(src), dest_dir) {
        return Ok(src.to_path_buf());
    }
    if is_inside(dest_dir, src) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Cannot move a folder into itself.",
        ));
    }
    std::fs::create_dir_all(dest_dir)?;
    let dest = unique_destination(dest_dir, &file_name_of(src)?)?;
    match rename(src, &dest) {
        Ok(()) => {}
        Err(e) if crosses_devices(&e) => {
            copy_recursive(src, &dest)?;
            if std::fs::metadata(src)?.is_dir() {
                std::fs::remove_dir_all(src)?;
            } else {
                std::fs::remove_file(src)?;
            }
        }
        Err(e) => return Err(e),
    }
    Ok(dest)
}

/// Copies a file or folder into `dest_dir`, renaming on collision. The
/// original stays. Returns the new path (or `src` when it's already there).
pub fn copy_into(src: &Path, dest_dir: &Path) -> io::Result<PathBuf> {
    if same_path(parent_of(src), dest_dir) {
        return Ok(src.to_path_buf());
    }
    if is_inside(dest_dir, src) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Cannot copy a folder into itself.",
        ));
    }
    std::fs::create_dir_all(dest_dir)?;
    let dest = unique_destination(dest_dir, &file_name_of(src)?)?;
    copy_recursive(src, &dest)?;
    Ok(dest)
}

/// Characters Windows does not allow in file names.
fn is_invalid_name_char(c: char) -> bool {
    "<>:\"/\\|?*".contains(c) || (c as u32) < 0x20
}

/// Renames an item in place. If the user typed the display name of a shortcut
/// (no .lnk), the hidden extension is kept. Returns the new path.
pub fn rename_item(path: &Path, new_name: &str) -> io::Result<PathBuf> {
    let trimmed = new_name.trim().trim_end_matches(['.', ' ']);
    if trimmed.is_empty() || trimmed.chars().any(is_invalid_name_char) {
        let msg = format!("\"{new_name}\" is not a valid file name.");
        return Err(io::Error::new(io::ErrorKind::InvalidInput, msg));
    }
    let old_name = file_name_of(path)?;
    let old_ext = extname(&old_name);
    let mut file_name = trimmed.to_owned();
    if has_hidden_extension(&old_name) && !extname(trimmed).eq_ignore_ascii_case(old_ext) {
        file_name.push_str(old_ext);
    }
    let dest = parent_of(path).join(&file_name);
    if same_path(&dest, path) {
        // Case-only rename, which a plain rename handles fine.
        if dest != path {
            std::fs::rename(path, &dest)?;
        }
        return Ok(dest);
    }
    if exists(&dest) {
        let msg = format!("An item named \"{file_name}\" already exists.");
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, msg));
    }
    std::fs::rename(path, &dest)?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let p =
                std::env::temp_dir().join(format!("sorter-fs-{}", crate::config::random_uuid()));
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn join(&self, s: &str) -> PathBuf {
            self.0.join(s)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn touch(p: &Path, content: &str) {
        std::fs::write(p, content).unwrap();
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    fn item(file_name: &str, modified_ms: f64, is_directory: bool) -> PanelItem {
        PanelItem {
            file_name: file_name.into(),
            display_name: display_name_for(file_name),
            path: file_name.into(),
            is_directory,
            is_shortcut: false,
            modified_ms,
        }
    }

    fn sorted(sort_by: SortBy, order: &[&str], cells: &[PlacedItem]) -> Vec<String> {
        let mut items = vec![
            item("z.txt", 1.0, false),
            item("a.lnk", 3.0, false),
            item("m.exe", 2.0, false),
            item("dir", 0.0, true),
        ];
        sort_items(&mut items, sort_by, &s(order), cells);
        items.into_iter().map(|i| i.file_name).collect()
    }

    #[test]
    fn display_name_hides_shortcut_extensions_only() {
        assert_eq!(display_name_for("Steam.lnk"), "Steam");
        assert_eq!(display_name_for("Site.URL"), "Site");
        assert_eq!(display_name_for("notes.txt"), "notes.txt");
        assert_eq!(display_name_for(".lnk"), ".lnk");
    }

    #[test]
    fn is_inside_detects_descendants() {
        let root = TempDir::new();
        assert!(is_inside(&root.join("a\\b"), &root.0));
        assert!(is_inside(&root.0, &root.0));
        assert!(!is_inside(&root.join("..\\other"), &root.0));
        assert!(!is_inside(Path::new("C:\\Apps2"), Path::new("C:\\Apps")));
        assert!(is_inside(Path::new("C:\\Apps"), Path::new("C:\\")));
        assert!(same_path(Path::new("C:\\A\\"), Path::new("c:/a")));
    }

    #[test]
    fn lists_files_hides_desktop_ini_and_sorts_by_name() {
        let root = TempDir::new();
        touch(&root.join("b.txt"), "x");
        touch(&root.join("A.lnk"), "x");
        touch(&root.join("desktop.ini"), "x");
        std::fs::create_dir(root.join("Folder")).unwrap();
        let items = list_items(&root.0, SortBy::Name, &[], &[]).unwrap();
        let names: Vec<_> = items.iter().map(|i| i.display_name.as_str()).collect();
        assert_eq!(names, ["A", "b.txt", "Folder"]);
        assert!(items.iter().find(|i| i.file_name == "Folder").unwrap().is_directory);
    }

    #[test]
    fn lists_a_missing_folder_as_empty() {
        let root = TempDir::new();
        assert!(list_items(&root.join("missing"), SortBy::Name, &[], &[]).unwrap().is_empty());
    }

    #[test]
    fn flags_shortcuts_but_not_folders_named_like_them() {
        let root = TempDir::new();
        touch(&root.join("Steam.lnk"), "x");
        touch(&root.join("Game.URL"), "x");
        touch(&root.join("tool.exe"), "x");
        std::fs::create_dir(root.join("Folder.lnk")).unwrap();
        let items = list_items(&root.0, SortBy::Name, &[], &[]).unwrap();
        let flags: HashMap<_, _> =
            items.iter().map(|i| (i.file_name.as_str(), i.is_shortcut)).collect();
        assert_eq!(
            flags,
            HashMap::from([
                ("Steam.lnk", true),
                ("Game.URL", true),
                ("tool.exe", false),
                ("Folder.lnk", false)
            ])
        );
    }

    #[test]
    fn sorts_by_type_with_folders_first() {
        assert_eq!(sorted(SortBy::Type, &[], &[]), ["dir", "m.exe", "a.lnk", "z.txt"]);
    }

    #[test]
    fn sorts_by_modified_newest_first() {
        assert_eq!(sorted(SortBy::Modified, &[], &[]), ["a.lnk", "m.exe", "z.txt", "dir"]);
    }

    #[test]
    fn sorts_names_naturally() {
        let mut n =
            vec![item("file10", 0.0, false), item("file2", 0.0, false), item("File1", 0.0, false)];
        sort_items(&mut n, SortBy::Name, &[], &[]);
        let names: Vec<_> = n.iter().map(|i| i.file_name.as_str()).collect();
        assert_eq!(names, ["File1", "file2", "file10"]);
    }

    #[test]
    fn follows_the_custom_order_then_the_rest_by_name() {
        assert_eq!(
            sorted(SortBy::Custom, &["m.exe", "Z.TXT"], &[]),
            ["m.exe", "z.txt", "a.lnk", "dir"]
        );
    }

    #[test]
    fn reads_spots_row_by_row_before_the_custom_order() {
        let cells = [PlacedItem::new("dir", 2, 1), PlacedItem::new("a.lnk", 3, 0)];
        assert_eq!(sorted(SortBy::Custom, &["z.txt"], &cells), ["a.lnk", "dir", "z.txt", "m.exe"]);
    }

    #[test]
    fn custom_without_an_order_sorts_by_name() {
        assert_eq!(sorted(SortBy::Custom, &[], &[]), sorted(SortBy::Name, &[], &[]));
    }

    #[test]
    fn sync_order_drops_gone_names_and_appends_new_ones() {
        assert_eq!(
            sync_order(&s(&["b", "gone", "a"]), &s(&["b", "a", "new"])),
            Some(s(&["b", "a", "new"]))
        );
        assert_eq!(sync_order(&s(&["b", "a"]), &s(&["b", "a"])), None);
        assert_eq!(sync_order(&s(&["b", "a"]), &s(&["a", "b"])), None);
    }

    #[test]
    fn order_is_capped_and_stable_once_full() {
        let many: Vec<String> = (0..MAX_PANEL_ITEMS + 5).map(|i| format!("f{i}")).collect();
        let order = sync_order(&[], &many).unwrap();
        assert_eq!(order.len(), MAX_PANEL_ITEMS);
        assert_eq!(sync_order(&order, &many), None);
        let placed = place_in_order(&many[..MAX_PANEL_ITEMS], &s(&["new"]), None);
        assert_eq!(placed.len(), MAX_PANEL_ITEMS);
        assert_eq!(placed.last().unwrap(), "new");
        assert_eq!(placed[0], "f1");
    }

    #[test]
    fn place_in_order_moves_names_before_another() {
        assert_eq!(
            place_in_order(&s(&["a", "b", "c", "d"]), &s(&["d"]), Some("b")),
            s(&["a", "d", "b", "c"])
        );
        assert_eq!(
            place_in_order(&s(&["a", "b", "c"]), &s(&["a"]), Some("c")),
            s(&["b", "a", "c"])
        );
        assert_eq!(
            place_in_order(&s(&["a", "b"]), &s(&["x", "y"]), None),
            s(&["a", "b", "x", "y"])
        );
        assert_eq!(place_in_order(&s(&["a", "b"]), &s(&["x"]), Some("nope")), s(&["a", "b", "x"]));
    }

    #[test]
    fn prunes_and_renames_cells_and_order() {
        let cells = [PlacedItem::new("a", 0, 0), PlacedItem::new("gone", 1, 0)];
        assert_eq!(prune_cells(&cells, &s(&["A"])), Some(vec![PlacedItem::new("a", 0, 0)]));
        assert_eq!(prune_cells(&cells, &s(&["a", "gone"])), None);
        assert_eq!(
            rename_in_cells(&[PlacedItem::new("b", 2, 3)], "b", "bee"),
            [PlacedItem::new("bee", 2, 3)]
        );
        assert_eq!(rename_in_order(&s(&["a", "b", "c"]), "b", "bee"), s(&["a", "bee", "c"]));
    }

    #[test]
    fn unique_destination_appends_a_number_before_the_extension() {
        let root = TempDir::new();
        touch(&root.join("app.lnk"), "x");
        touch(&root.join("app (2).lnk"), "x");
        assert_eq!(unique_destination(&root.0, "app.lnk").unwrap(), root.join("app (3).lnk"));
        assert_eq!(unique_destination(&root.0, "new.lnk").unwrap(), root.join("new.lnk"));
    }

    #[test]
    fn copy_into_keeps_the_original_and_renames_on_collision() {
        let root = TempDir::new();
        let (desk, panel) = (root.join("Desktop"), root.join("Panel"));
        std::fs::create_dir(&desk).unwrap();
        std::fs::create_dir(&panel).unwrap();
        touch(&desk.join("Game.lnk"), "new");
        touch(&panel.join("Game.lnk"), "old");
        let dest = copy_into(&desk.join("Game.lnk"), &panel).unwrap();
        assert_eq!(dest, panel.join("Game (2).lnk"));
        assert_eq!(std::fs::read_to_string(desk.join("Game.lnk")).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(dest).unwrap(), "new");
    }

    #[test]
    fn copy_into_copies_folders_with_their_contents() {
        let root = TempDir::new();
        std::fs::create_dir(root.join("f")).unwrap();
        touch(&root.join("f\\inner.txt"), "hi");
        let dest = copy_into(&root.join("f"), &root.join("panel")).unwrap();
        assert_eq!(std::fs::read_to_string(dest.join("inner.txt")).unwrap(), "hi");
        assert!(root.join("f\\inner.txt").exists());
    }

    #[test]
    fn copy_and_move_are_no_ops_in_place_and_refuse_a_folder_into_itself() {
        let root = TempDir::new();
        touch(&root.join("a.txt"), "x");
        assert_eq!(copy_into(&root.join("a.txt"), &root.0).unwrap(), root.join("a.txt"));
        assert_eq!(move_into(&root.join("a.txt"), &root.0).unwrap(), root.join("a.txt"));
        std::fs::create_dir(root.join("f")).unwrap();
        let err = copy_into(&root.join("f"), &root.join("f\\sub")).unwrap_err();
        assert!(err.to_string().contains("itself"));
        let err = move_into(&root.join("f"), &root.join("f\\sub")).unwrap_err();
        assert!(err.to_string().contains("itself"));
    }

    #[test]
    fn move_into_renames_on_collision_and_creates_the_folder() {
        let root = TempDir::new();
        let (desk, panel) = (root.join("Desktop"), root.join("Panel"));
        std::fs::create_dir(&desk).unwrap();
        std::fs::create_dir(&panel).unwrap();
        touch(&desk.join("Game.lnk"), "new");
        touch(&panel.join("Game.lnk"), "old");
        let dest = move_into(&desk.join("Game.lnk"), &panel).unwrap();
        assert_eq!(dest, panel.join("Game (2).lnk"));
        assert!(!desk.join("Game.lnk").exists());
        assert_eq!(std::fs::read_to_string(dest).unwrap(), "new");
        touch(&root.join("a.txt"), "x");
        assert!(move_into(&root.join("a.txt"), &root.join("new\\panel")).unwrap().exists());
    }

    #[test]
    fn move_into_copies_and_deletes_across_drives() {
        let root = TempDir::new();
        let src = root.join("src");
        std::fs::create_dir_all(src.join("Tools")).unwrap();
        touch(&src.join("Tools\\tool.exe"), "bin");
        let other_drive = |_: &Path, _: &Path| Err(io::Error::from_raw_os_error(17));
        let dest = move_into_with(&src.join("Tools"), &root.join("dst"), other_drive).unwrap();
        assert_eq!(std::fs::read_to_string(dest.join("tool.exe")).unwrap(), "bin");
        assert!(!src.join("Tools").exists());
    }

    #[test]
    fn rename_keeps_the_hidden_lnk_extension() {
        let root = TempDir::new();
        touch(&root.join("Old.lnk"), "x");
        assert_eq!(rename_item(&root.join("Old.lnk"), "New").unwrap(), root.join("New.lnk"));
        assert!(root.join("New.lnk").exists());
    }

    #[test]
    fn rename_rejects_invalid_names_and_existing_targets() {
        let root = TempDir::new();
        touch(&root.join("a.txt"), "x");
        touch(&root.join("b.txt"), "x");
        let err = rename_item(&root.join("a.txt"), "bad:name").unwrap_err();
        assert!(err.to_string().contains("not a valid"));
        let err = rename_item(&root.join("a.txt"), "b.txt").unwrap_err();
        assert!(err.to_string().contains("already exists"));
        assert!(rename_item(&root.join("a.txt"), "..").is_err());
    }
}
