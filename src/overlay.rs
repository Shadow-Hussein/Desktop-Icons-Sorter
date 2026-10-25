//! Where the shortcut arrow comes from. Explorer draws the icon named by the
//! `29` value of `HKLM|HKCU\...\Explorer\Shell Icons` when there is one (people
//! use a blank icon there to hide arrows), else imageres.dll's own arrow.

use crate::icon_source::IconSource;

pub const SHELL_ICONS_KEY: &str =
    "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Shell Icons";
pub const SHORTCUT_OVERLAY_VALUE: &str = "29";

/// Windows' built-in shortcut arrow, used when no custom one is configured.
pub fn default_arrow() -> IconSource {
    IconSource { file: "%SystemRoot%\\System32\\imageres.dll".into(), index: -163 }
}

fn unquote(s: &str) -> &str {
    s.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(s)
}

/// Parses an icon location like `C:\path\file.ico,0`. The index after the
/// last comma is optional, and the path may itself hold commas.
pub fn parse_icon_location(value: &str) -> Option<IconSource> {
    let value = unquote(value.trim());
    if let Some((file, index)) = value.rsplit_once(',')
        && !file.is_empty()
        && let Ok(index) = index.trim().parse::<i32>()
    {
        return Some(IconSource { file: unquote(file.trim()).to_owned(), index });
    }
    (!value.is_empty()).then(|| IconSource { file: value.to_owned(), index: 0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(file: &str, index: i32) -> Option<IconSource> {
        Some(IconSource { file: file.into(), index })
    }

    #[test]
    fn reads_a_custom_arrow_with_an_index() {
        assert_eq!(
            parse_icon_location("C:\\Icons\\purple-arrow.ico,0"),
            src("C:\\Icons\\purple-arrow.ico", 0)
        );
    }

    #[test]
    fn handles_negative_resource_ids_and_commas_in_paths() {
        assert_eq!(
            parse_icon_location("%SystemRoot%\\a,b\\imageres.dll,-163"),
            src("%SystemRoot%\\a,b\\imageres.dll", -163)
        );
    }

    #[test]
    fn a_value_without_an_index_is_index_0_and_loses_its_quotes() {
        assert_eq!(
            parse_icon_location("\"C:\\Program Files\\Arrows\\blank.ico\""),
            src("C:\\Program Files\\Arrows\\blank.ico", 0)
        );
        assert_eq!(parse_icon_location("\"C:\\a b\\x.dll\",3"), src("C:\\a b\\x.dll", 3));
    }

    #[test]
    fn an_empty_value_is_none() {
        assert_eq!(parse_icon_location("  "), None);
    }

    #[test]
    fn defaults_to_the_built_in_imageres_arrow() {
        assert_eq!(
            default_arrow(),
            IconSource { file: "%SystemRoot%\\System32\\imageres.dll".into(), index: -163 }
        );
    }
}
