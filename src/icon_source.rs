//! Finding the file that holds an item's real icon, the way Explorer does.
//! Used when the shell's image factory only has a generic icon for an item.

use crate::folder::extname;

/// A file with icon resources and an index into them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconSource {
    pub file: String,
    /// Icon index, or a negative resource ID (the same convention as ExtractIconEx).
    pub index: i32,
}

/// Files whose icon is stored inside the file itself.
const ICON_CONTAINERS: [&str; 7] = [".exe", ".dll", ".ico", ".cpl", ".ocx", ".scr", ".icl"];

pub fn is_icon_container(path: &str) -> bool {
    ICON_CONTAINERS.contains(&extname(path).to_lowercase().as_str())
}

/// Expands Windows-style `%VAR%` references, leaving unknown ones as they are.
pub fn expand_env(value: &str, env: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('%').filter(|&e| e > 0) else {
            break;
        };
        out.push_str(&rest[..start]);
        match env(&after[..end]) {
            Some(v) => out.push_str(&v),
            None => out.push_str(&rest[start..start + end + 2]),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn unquote(s: &str) -> &str {
    s.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(s)
}

/// Reads `IconFile` / `IconIndex` from the `[InternetShortcut]` section of a .url file.
pub fn parse_url_shortcut(text: &str) -> (Option<String>, i32) {
    let mut in_section = false;
    let mut icon_file = None;
    let mut icon_index = 0;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_section = line.eq_ignore_ascii_case("[internetshortcut]");
            continue;
        }
        let Some((key, value)) = line.split_once('=').filter(|_| in_section) else {
            continue;
        };
        let (key, value) = (key.trim().to_ascii_lowercase(), value.trim());
        if key == "iconfile" && !value.is_empty() {
            icon_file = Some(unquote(value).to_owned());
        } else if key == "iconindex" {
            icon_index = parse_int_prefix(value);
        }
    }
    (icon_file, icon_index)
}

/// JS `parseInt(value, 10) || 0`: leading sign and digits, anything after ignored.
fn parse_int_prefix(value: &str) -> i32 {
    let digits_end = value
        .char_indices()
        .find(|&(i, c)| !(c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+'))))
        .map_or(value.len(), |(i, _)| i);
    value[..digits_end].parse().unwrap_or(0)
}

/// What a .lnk file says about itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShortcutDetails {
    pub target: Option<String>,
    pub icon: Option<String>,
    pub icon_index: i32,
}

/// What resolving needs from the system, so it can be tested without one.
pub trait IconSourceDeps {
    fn read_shortcut_link(&self, path: &str) -> Option<ShortcutDetails>;
    fn read_file(&self, path: &str) -> Option<String>;
    fn exists(&self, path: &str) -> bool;
    fn env(&self, name: &str) -> Option<String>;
}

/// Finds the file that holds `path`'s real icon:
/// - .lnk: the shortcut's icon location, else its target
/// - .url: its IconFile/IconIndex (Steam and other launchers set these)
/// - .exe/.dll/.ico…: the file itself
///
/// None when there's nothing better than the generic icon (for example a
/// Store app shortcut with no target), so callers can fall back.
pub fn resolve_icon_source(path: &str, deps: &impl IconSourceDeps) -> Option<IconSource> {
    let usable = |file: Option<&str>, index: i32| {
        let file = file.filter(|f| !f.is_empty())?;
        let expanded = expand_env(file, |n| deps.env(n));
        deps.exists(&expanded).then_some(IconSource { file: expanded, index })
    };
    match extname(path).to_lowercase().as_str() {
        ".lnk" => {
            let link = deps.read_shortcut_link(path)?;
            usable(link.icon.as_deref(), link.icon_index)
                // No explicit icon: Explorer uses the target's own icon.
                .or_else(|| usable(link.target.as_deref(), 0))
        }
        ".url" => {
            let (icon_file, icon_index) = parse_url_shortcut(&deps.read_file(path)?);
            usable(icon_file.as_deref(), icon_index)
        }
        _ if is_icon_container(path) => usable(Some(path), 0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    #[derive(Default)]
    struct Fake {
        files: HashSet<String>,
        links: HashMap<String, ShortcutDetails>,
        text: Option<String>,
        env: HashMap<String, String>,
    }

    impl IconSourceDeps for Fake {
        fn read_shortcut_link(&self, path: &str) -> Option<ShortcutDetails> {
            self.links.get(path).cloned()
        }
        fn read_file(&self, _: &str) -> Option<String> {
            self.text.clone()
        }
        fn exists(&self, path: &str) -> bool {
            self.files.contains(path)
        }
        fn env(&self, name: &str) -> Option<String> {
            self.env.get(&name.to_lowercase()).cloned()
        }
    }

    fn files(f: &[&str]) -> HashSet<String> {
        f.iter().map(|s| s.to_string()).collect()
    }

    fn link(target: &str, icon: Option<&str>, icon_index: i32) -> ShortcutDetails {
        ShortcutDetails { target: Some(target.into()), icon: icon.map(Into::into), icon_index }
    }

    fn src(file: &str, index: i32) -> Option<IconSource> {
        Some(IconSource { file: file.into(), index })
    }

    const BRAVE: &str = "C:\\Program Files\\BraveSoftware\\Brave-Browser\\Application\\brave.exe";

    #[test]
    fn expands_env_case_insensitively_and_keeps_unknown_ones() {
        let env = |n: &str| match n.to_lowercase().as_str() {
            "systemroot" => Some("C:\\Windows".to_string()),
            "programfiles" => Some("C:\\Program Files".to_string()),
            _ => None,
        };
        assert_eq!(
            expand_env("%systemroot%\\System32\\shell32.dll", env),
            "C:\\Windows\\System32\\shell32.dll"
        );
        assert_eq!(
            expand_env("%PROGRAMFILES%\\App\\%NOPE%\\a.exe", env),
            "C:\\Program Files\\App\\%NOPE%\\a.exe"
        );
        assert_eq!(expand_env("100% sure", env), "100% sure");
    }

    #[test]
    fn reads_icon_file_and_index_from_a_steam_shortcut() {
        let text = [
            "[{000214A0-0000-0000-C000-000000000046}]",
            "Prop3=19,0",
            "[InternetShortcut]",
            "IDList=",
            "IconIndex=0",
            "URL=steam://rungameid/2849080",
            "IconFile=C:\\Program Files (x86)\\Steam\\steam\\games\\abc.ico",
        ]
        .join("\r\n");
        assert_eq!(
            parse_url_shortcut(&text),
            (Some("C:\\Program Files (x86)\\Steam\\steam\\games\\abc.ico".into()), 0)
        );
    }

    #[test]
    fn handles_quotes_negative_indexes_and_keys_outside_the_section() {
        let text = "IconFile=ignored.ico\n[InternetShortcut]\nIconFile=\"C:\\a b\\x.dll\"\nIconIndex=-101\n";
        assert_eq!(parse_url_shortcut(text), (Some("C:\\a b\\x.dll".into()), -101));
    }

    #[test]
    fn a_plain_web_link_has_no_icon_file() {
        assert_eq!(parse_url_shortcut("[InternetShortcut]\nURL=https://example.com\n"), (None, 0));
    }

    #[test]
    fn recognises_files_with_icon_resources() {
        assert!(is_icon_container("C:\\x\\Steam.EXE"));
        assert!(is_icon_container("shell32.dll"));
        assert!(!is_icon_container("notes.txt"));
    }

    #[test]
    fn uses_a_shortcuts_explicit_icon_location() {
        let d = Fake {
            files: files(&[BRAVE, "C:\\Windows\\System32\\imageres.dll"]),
            env: HashMap::from([("systemroot".into(), "C:\\Windows".into())]),
            links: HashMap::from([(
                "D:\\Apps\\Brave.lnk".into(),
                link(BRAVE, Some("%SystemRoot%\\System32\\imageres.dll"), -5),
            )]),
            ..Default::default()
        };
        assert_eq!(
            resolve_icon_source("D:\\Apps\\Brave.lnk", &d),
            src("C:\\Windows\\System32\\imageres.dll", -5)
        );
    }

    #[test]
    fn falls_back_to_the_target_without_a_usable_icon() {
        let d = Fake {
            files: files(&[BRAVE]),
            links: HashMap::from([
                ("D:\\Brave.lnk".into(), link(BRAVE, Some(""), 0)),
                ("D:\\Gone.lnk".into(), link(BRAVE, Some("C:\\gone.ico"), 2)),
            ]),
            ..Default::default()
        };
        assert_eq!(resolve_icon_source("D:\\Brave.lnk", &d), src(BRAVE, 0));
        assert_eq!(resolve_icon_source("D:\\Gone.lnk", &d), src(BRAVE, 0));
    }

    #[test]
    fn gives_up_on_unreadable_or_targetless_shortcuts() {
        assert_eq!(resolve_icon_source("D:\\Broken.lnk", &Fake::default()), None);
        let d = Fake {
            links: HashMap::from([("D:\\Store.lnk".into(), link("", None, 0))]),
            ..Default::default()
        };
        assert_eq!(resolve_icon_source("D:\\Store.lnk", &d), None);
    }

    #[test]
    fn uses_a_url_files_icon_file() {
        let ico = "C:\\Steam\\games\\kr6.ico";
        let d = Fake {
            files: files(&[ico]),
            text: Some(format!(
                "[InternetShortcut]\nURL=steam://rungameid/1\nIconFile={ico}\nIconIndex=0"
            )),
            ..Default::default()
        };
        assert_eq!(resolve_icon_source("D:\\Games\\Kingdom Rush.url", &d), src(ico, 0));
    }

    #[test]
    fn plain_files_and_urls_without_icons_have_none() {
        let d = Fake {
            text: Some("[InternetShortcut]\nURL=https://example.com".into()),
            ..Default::default()
        };
        assert_eq!(resolve_icon_source("D:\\Site.url", &d), None);
        let d = Fake { files: files(&["D:\\notes.txt"]), ..Default::default() };
        assert_eq!(resolve_icon_source("D:\\notes.txt", &d), None);
    }

    #[test]
    fn uses_an_exes_own_icon() {
        let d = Fake { files: files(&[BRAVE]), ..Default::default() };
        assert_eq!(resolve_icon_source(BRAVE, &d), src(BRAVE, 0));
    }
}
