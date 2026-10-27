//! Loading, validating and saving `config.json`.
//!
//! The file format is the one the Electron app wrote, so an existing config
//! keeps working. Validation is hand-written, like the old `validate.ts`:
//! missing fields get defaults, unknown fields are dropped, and a panel with
//! a bad field is dropped on its own so one bad entry doesn't wipe the rest.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::model::{
    AddMode, AppConfig, BgEffect, BgTexture, Bounds, CollapseAlign, CollapseDirection,
    DEFAULT_EFFECT_STRENGTH, DEFAULT_IMAGE_OPACITY, DEFAULT_TEXTURE_COLOR, DEFAULT_TEXTURE_OPACITY,
    HeaderShape, IconSize, ImageFit, MAX_PANEL_ITEMS, MAX_TEXTURE_SCALE, MIN_PANEL_HEIGHT,
    MIN_PANEL_WIDTH, MIN_TEXTURE_SCALE, PanelConfig, PlacedItem, SortBy, TitleBarStyle,
};

/// How long after a change the config is written, so a burst of changes is one write.
pub const SAVE_DEBOUNCE_MS: u32 = 300;

/// Where `config.json` lives: `DESKTOP_SORTER_USER_DATA` (a throwaway profile
/// for tests and measuring) or the folder the Electron app used.
pub fn config_path() -> PathBuf {
    user_data_dir().join("config.json")
}

pub fn user_data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("DESKTOP_SORTER_USER_DATA") {
        return PathBuf::from(dir);
    }
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default();
    appdata.join("desktop-icons-sorter")
}

/// Tells instances apart: empty for the normal profile, a short hash of the
/// folder for a throwaway one (`DESKTOP_SORTER_USER_DATA`). One instance runs
/// per profile, so a test profile can run next to the real one.
pub fn instance_suffix() -> String {
    if std::env::var_os("DESKTOP_SORTER_USER_DATA").is_none() {
        return String::new();
    }
    // FNV-1a: stable across builds, unlike the standard library's hasher.
    let path = user_data_dir().to_string_lossy().to_lowercase();
    let hash = path
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3));
    format!(".{hash:016x}")
}

/// Parent folder for new panels' folders.
pub fn panels_root_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("DESKTOP_SORTER_PANELS_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default();
    home.join("DesktopSorter")
}

// ------------------------------------------------------------------ validation

/// A value failed validation; the details don't matter, only that it's rejected.
#[derive(Debug, PartialEq)]
pub struct Invalid;

type Parsed<T> = Result<T, Invalid>;

/// JS string length (UTF-16 units), which is what the old limits counted.
fn js_len(s: &str) -> usize {
    s.encode_utf16().count()
}

fn string(v: &Value, min: usize, max: usize) -> Parsed<String> {
    let s = v.as_str().ok_or(Invalid)?;
    let len = js_len(s);
    if len < min || len > max {
        return Err(Invalid);
    }
    Ok(s.to_owned())
}

fn hex_color(v: &Value) -> Parsed<String> {
    let s = v.as_str().ok_or(Invalid)?;
    let ok = s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit());
    if ok { Ok(s.to_owned()) } else { Err(Invalid) }
}

fn number(v: &Value, min: f64, max: f64) -> Parsed<f64> {
    let n = v.as_f64().filter(|n| n.is_finite()).ok_or(Invalid)?;
    if n < min || n > max {
        return Err(Invalid);
    }
    Ok(n)
}

fn integer(v: &Value, min: f64, max: f64) -> Parsed<u32> {
    let n = number(v, min, max)?;
    if n.fract() != 0.0 {
        return Err(Invalid);
    }
    Ok(n as u32)
}

fn boolean(v: &Value) -> Parsed<bool> {
    v.as_bool().ok_or(Invalid)
}

/// One of a string enum's values (the serde names in `model.rs`).
fn one_of<T: DeserializeOwned>(v: &Value) -> Parsed<T> {
    if !v.is_string() {
        return Err(Invalid);
    }
    serde_json::from_value(v.clone()).map_err(|_| Invalid)
}

fn array<T>(v: &Value, max: usize, item: impl Fn(&Value) -> Parsed<T>) -> Parsed<Vec<T>> {
    let items = v.as_array().ok_or(Invalid)?;
    if items.len() > max {
        return Err(Invalid);
    }
    items.iter().map(item).collect()
}

/// A field that must be there.
fn required<T>(
    o: &Map<String, Value>,
    key: &str,
    parse: impl Fn(&Value) -> Parsed<T>,
) -> Parsed<T> {
    parse(o.get(key).ok_or(Invalid)?)
}

/// A field that falls back to `default` when missing. A JSON null is not missing.
fn optional<T>(
    o: &Map<String, Value>,
    key: &str,
    default: T,
    parse: impl Fn(&Value) -> Parsed<T>,
) -> Parsed<T> {
    match o.get(key) {
        None => Ok(default),
        Some(v) => parse(v),
    }
}

fn bounds(v: &Value) -> Parsed<Bounds> {
    let o = v.as_object().ok_or(Invalid)?;
    let any = |v: &Value| number(v, f64::MIN, f64::MAX);
    Ok(Bounds {
        x: required(o, "x", any)?.round() as i32,
        y: required(o, "y", any)?.round() as i32,
        width: required(o, "width", |v| number(v, MIN_PANEL_WIDTH as f64, f64::MAX))?.round()
            as i32,
        height: required(o, "height", |v| number(v, MIN_PANEL_HEIGHT as f64, f64::MAX))?.round()
            as i32,
    })
}

/// A file name, as Windows allows it.
fn file_name(v: &Value) -> Parsed<String> {
    string(v, 1, 255)
}

fn placed_item(v: &Value) -> Parsed<PlacedItem> {
    let o = v.as_object().ok_or(Invalid)?;
    Ok(PlacedItem {
        name: required(o, "name", file_name)?,
        col: required(o, "col", |v| integer(v, 0.0, 999.0))?,
        row: required(o, "row", |v| integer(v, 0.0, 99_999.0))?,
    })
}

fn opacity(v: &Value) -> Parsed<f64> {
    number(v, 0.0, 1.0)
}

/// Pill and minimal used to be title bar shapes of their own. Now they are
/// title bar styles that work with any shape, so a panel saved with either
/// becomes the default shape in that style, and looks the same.
fn upgrade_panel(o: &Map<String, Value>) -> Map<String, Value> {
    let mut o = o.clone();
    if let Some(old @ ("pill" | "minimal")) = o.get("headerShape").and_then(Value::as_str) {
        let style = Value::String(old.to_owned());
        o.insert("headerShape".into(), Value::String("default".into()));
        o.insert("titleBarStyle".into(), style);
    }
    // The first test build called the blur effect "glossy".
    if o.get("bgEffect").and_then(Value::as_str) == Some("glossy") {
        o.insert("bgEffect".into(), Value::String("blur".into()));
    }
    o
}

pub fn parse_panel(v: &Value) -> Parsed<PanelConfig> {
    let o = upgrade_panel(v.as_object().ok_or(Invalid)?);
    let o = &o;
    Ok(PanelConfig {
        id: required(o, "id", |v| string(v, 1, usize::MAX))?,
        title: required(o, "title", |v| string(v, 0, 200))?,
        bounds: required(o, "bounds", bounds)?,
        collapsed: optional(o, "collapsed", false, boolean)?,
        collapse_direction: optional(o, "collapseDirection", CollapseDirection::Up, one_of)?,
        header_shape: optional(o, "headerShape", HeaderShape::Default, one_of)?,
        title_bar_style: optional(o, "titleBarStyle", TitleBarStyle::Standard, one_of)?,
        collapse_align: optional(o, "collapseAlign", CollapseAlign::Start, one_of)?,
        stackable: optional(o, "stackable", true, boolean)?,
        bg_color: optional(o, "bgColor", "#1e1e1e".into(), hex_color)?,
        bg_opacity: optional(o, "bgOpacity", 0.45, opacity)?,
        title_bar_color: optional(o, "titleBarColor", "#000000".into(), hex_color)?,
        title_bar_opacity: optional(o, "titleBarOpacity", 0.55, opacity)?,
        title_text_color: optional(o, "titleTextColor", "#ffffff".into(), hex_color)?,
        folder_path: required(o, "folderPath", |v| string(v, 1, usize::MAX))?,
        icon_size: optional(o, "iconSize", IconSize::Medium, one_of)?,
        sort_by: optional(o, "sortBy", SortBy::Custom, one_of)?,
        cells: optional(o, "cells", Vec::new(), |v| array(v, MAX_PANEL_ITEMS, placed_item))?,
        order: optional(o, "order", Vec::new(), |v| array(v, MAX_PANEL_ITEMS, file_name))?,
        catch_new_items: optional(o, "catchNewItems", false, boolean)?,
        bg_image: optional(o, "bgImage", String::new(), |v| string(v, 0, 32_767))?,
        bg_image_opacity: optional(o, "bgImageOpacity", DEFAULT_IMAGE_OPACITY, opacity)?,
        bg_image_fit: optional(o, "bgImageFit", ImageFit::Fill, one_of)?,
        texture: optional(o, "texture", BgTexture::None, one_of)?,
        texture_color: optional(o, "textureColor", DEFAULT_TEXTURE_COLOR.into(), hex_color)?,
        texture_opacity: optional(o, "textureOpacity", DEFAULT_TEXTURE_OPACITY, opacity)?,
        texture_scale: optional(o, "textureScale", 1.0, |v| {
            number(v, MIN_TEXTURE_SCALE, MAX_TEXTURE_SCALE)
        })?,
        texture_random: optional(o, "textureRandom", false, boolean)?,
        bg_effect: optional(o, "bgEffect", BgEffect::None, one_of)?,
        bg_effect_strength: optional(o, "bgEffectStrength", DEFAULT_EFFECT_STRENGTH, opacity)?,
    })
}

/// The app-wide fields, with `panels` parsed by `panels`.
fn parse_app(
    o: &Map<String, Value>,
    panels: impl Fn(&Value) -> Parsed<Vec<PanelConfig>>,
) -> Parsed<AppConfig> {
    let d = AppConfig::default();
    Ok(AppConfig {
        version: optional(o, "version", 2, |v| match v.as_f64() {
            Some(n) if n == 1.0 || n == 2.0 => Ok(n as u8),
            _ => Err(Invalid),
        })?,
        panels: optional(o, "panels", Vec::new(), panels)?,
        start_with_windows: optional(o, "startWithWindows", d.start_with_windows, boolean)?,
        locked: optional(o, "locked", d.locked, boolean)?,
        snapping: optional(o, "snapping", d.snapping, boolean)?,
        stacking: optional(o, "stacking", d.stacking, boolean)?,
        add_mode: optional(o, "addMode", AddMode::Copy, one_of)?,
    })
}

/// Parses raw config JSON (None when the file wasn't JSON at all). Invalid
/// panels are dropped individually; a structurally broken file yields defaults.
/// `recovered` is true when anything had to be dropped.
pub fn parse_config(raw: Option<&Value>) -> (AppConfig, bool) {
    let Some(obj) = raw.and_then(Value::as_object) else {
        return (AppConfig::default(), true);
    };
    let all_panels = |v: &Value| array(v, usize::MAX, parse_panel);
    if let Ok(config) = parse_app(obj, all_panels) {
        return (migrate(config), false);
    }
    let panels: Vec<PanelConfig> = obj
        .get("panels")
        .and_then(Value::as_array)
        .map(|ps| ps.iter().filter_map(|p| parse_panel(p).ok()).collect())
        .unwrap_or_default();
    let base = parse_app(obj, |_| Ok(Vec::new())).unwrap_or_default();
    (migrate(AppConfig { panels, ..base }), true)
}

/// Version 2 made custom order (drag to arrange) the default. Panels saved by
/// version 1 sorted by name switch to it without moving an icon, since an
/// empty custom order sorts by name. Type and date sorts are kept.
pub fn migrate(mut config: AppConfig) -> AppConfig {
    if config.version == 2 {
        return config;
    }
    config.version = 2;
    for p in &mut config.panels {
        if p.sort_by == SortBy::Name {
            p.sort_by = SortBy::Custom;
        }
    }
    config
}

// ---------------------------------------------------------------- new panels

/// A random version 4 UUID. `RandomState` is seeded from the OS per instance,
/// which is plenty for panel ids and saves a dependency.
pub fn random_uuid() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let word = || {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        h.write_u128(nanos);
        h.finish()
    };
    let (hi, lo) = (word(), word());
    let hi = (hi & !0xf000) | 0x4000; // version 4
    let lo = (lo & !(0xc000 << 48)) | (0x8000 << 48); // RFC 4122 variant
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        hi >> 32,
        (hi >> 16) & 0xffff,
        hi & 0xffff,
        lo >> 48,
        lo & 0xffff_ffff_ffff
    )
}

pub fn create_panel(title: &str, bounds: Bounds, folder_path: &str) -> PanelConfig {
    let raw = serde_json::json!({
        "id": random_uuid(),
        "title": title,
        "bounds": bounds,
        "folderPath": folder_path,
    });
    parse_panel(&raw).expect("a new panel is valid")
}

/// A new panel that looks like `src`: same size, colors, shape and background,
/// with its own id and folder and none of `src`'s items. It doesn't catch new
/// desktop items, since only one panel may.
pub fn duplicate_panel(src: &PanelConfig, bounds: Bounds, folder_path: &str) -> PanelConfig {
    let title: String = if src.title.is_empty() {
        String::new()
    } else {
        // Titles are at most 200 UTF-16 units; make room for " copy".
        let mut t = String::new();
        for c in src.title.chars() {
            if js_len(&t) + c.len_utf16() > 195 {
                break;
            }
            t.push(c);
        }
        format!("{} copy", t.trim_end())
    };
    PanelConfig {
        id: random_uuid(),
        title,
        bounds,
        folder_path: folder_path.to_owned(),
        cells: Vec::new(),
        order: Vec::new(),
        catch_new_items: false,
        ..src.clone()
    }
}

/// Panel `to` with the look of `from`: colors, title bar, icon size and
/// background. What's its own stays: title, folder and items, where it is
/// and which way it collapses, sort order, stacking and catching.
pub fn with_look_of(to: &PanelConfig, from: &PanelConfig) -> PanelConfig {
    PanelConfig {
        header_shape: from.header_shape,
        title_bar_style: from.title_bar_style,
        collapse_align: from.collapse_align,
        bg_color: from.bg_color.clone(),
        bg_opacity: from.bg_opacity,
        title_bar_color: from.title_bar_color.clone(),
        title_bar_opacity: from.title_bar_opacity,
        title_text_color: from.title_text_color.clone(),
        icon_size: from.icon_size,
        bg_image: from.bg_image.clone(),
        bg_image_opacity: from.bg_image_opacity,
        bg_image_fit: from.bg_image_fit,
        texture: from.texture,
        texture_color: from.texture_color.clone(),
        texture_opacity: from.texture_opacity,
        texture_scale: from.texture_scale,
        texture_random: from.texture_random,
        bg_effect: from.bg_effect,
        bg_effect_strength: from.bg_effect_strength,
        ..to.clone()
    }
}

/// Turns a panel title into a safe Windows folder name.
pub fn folder_name_from_title(title: &str) -> String {
    let cleaned: String =
        title.chars().filter(|c| !"<>:\"/\\|?*".contains(*c) && !c.is_ascii_control()).collect();
    let cleaned = cleaned.trim_end_matches(['.', ' ']).trim();
    let name = if cleaned.is_empty() { "Panel" } else { cleaned };
    let lower = name.to_ascii_lowercase();
    let reserved = matches!(lower.as_str(), "con" | "prn" | "aux" | "nul")
        || ((lower.starts_with("com") || lower.starts_with("lpt"))
            && lower.len() == 4
            && lower.as_bytes()[3].is_ascii_digit());
    if reserved { format!("{name}_") } else { name.to_owned() }
}

// --------------------------------------------------------------------- store

/// Holds the config and saves it atomically (temp file, then rename), so a
/// crash never leaves a half-written file. The UI debounces saves by calling
/// `flush` `SAVE_DEBOUNCE_MS` after the last change, and on quit.
pub struct ConfigStore {
    path: PathBuf,
    config: AppConfig,
    dirty: bool,
}

impl ConfigStore {
    /// Loads the config. A file that fails validation is renamed to
    /// `config.bad.json` and the valid parts are kept and saved.
    pub fn load(path: PathBuf) -> Self {
        let raw = match std::fs::read_to_string(&path) {
            // Notepad and PowerShell can save a byte order mark; it's not part of the JSON.
            Ok(text) => serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).ok(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Self { path, config: AppConfig::default(), dirty: false };
            }
            Err(_) => None,
        };
        let (config, recovered) = parse_config(raw.as_ref());
        let mut store = Self { path, config, dirty: recovered };
        if recovered {
            // Keep going with what's valid even if the backup fails.
            let _ = std::fs::rename(&store.path, bad_path(&store.path));
            let _ = store.flush();
        }
        store
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn panel(&self, id: &str) -> Option<&PanelConfig> {
        self.config.panel(id)
    }

    /// Changes the config; it's written on the next `flush`.
    pub fn update(&mut self, f: impl FnOnce(&mut AppConfig)) -> &AppConfig {
        f(&mut self.config);
        self.dirty = true;
        &self.config
    }

    pub fn update_panel(
        &mut self,
        id: &str,
        f: impl FnOnce(&mut PanelConfig),
    ) -> Option<&PanelConfig> {
        let panel = self.config.panel_mut(id)?;
        f(panel);
        self.dirty = true;
        Some(panel)
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Writes the config if it changed since the last write.
    pub fn flush(&mut self) -> io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(&self.config).map_err(io::Error::other)?;
        let mut tmp = self.path.clone().into_os_string();
        tmp.push(".tmp");
        std::fs::write(&tmp, json)?;
        // Replaces the old file in one step (MoveFileEx with REPLACE_EXISTING).
        std::fs::rename(&tmp, &self.path)?;
        self.dirty = false;
        Ok(())
    }
}

fn bad_path(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    PathBuf::from(format!("{}.bad.json", s.strip_suffix(".json").unwrap_or(&s)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn bounds_json() -> Value {
        json!({ "x": 10, "y": 20, "width": 300, "height": 200 })
    }

    fn parse(v: Value) -> (AppConfig, bool) {
        parse_config(Some(&v))
    }

    fn panel(extra: Value) -> Value {
        let mut p =
            json!({ "id": "a", "title": "x", "bounds": bounds_json(), "folderPath": "C:\\x" });
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        p
    }

    #[test]
    fn fills_defaults_for_a_minimal_panel() {
        let (config, recovered) = parse(json!({ "panels": [panel(json!({ "title": "Games" }))] }));
        assert!(!recovered);
        assert_eq!(config.version, 2);
        let p = &config.panels[0];
        assert!(!p.collapsed);
        assert_eq!(p.collapse_direction, CollapseDirection::Up);
        assert_eq!(p.header_shape, HeaderShape::Default);
        assert_eq!(p.title_bar_style, TitleBarStyle::Standard);
        assert_eq!(p.collapse_align, CollapseAlign::Start);
        assert!(p.stackable);
        assert_eq!(p.bg_color, "#1e1e1e");
        assert_eq!(p.bg_opacity, 0.45);
        assert_eq!(p.icon_size, IconSize::Medium);
        assert_eq!(p.sort_by, SortBy::Custom);
        assert!(p.cells.is_empty() && p.order.is_empty());
        assert!(!p.catch_new_items);
        assert_eq!(p.bg_image, "");
        assert_eq!(p.bg_image_opacity, 1.0);
        assert_eq!(p.bg_image_fit, ImageFit::Fill);
        assert_eq!(p.texture, BgTexture::None);
        assert_eq!(p.texture_color, "#808080");
        assert_eq!(p.texture_opacity, 0.35);
        assert_eq!(p.texture_scale, 1.0);
        assert!(!p.texture_random);
        assert_eq!(p.bg_effect, BgEffect::None);
        assert_eq!(p.bg_effect_strength, 0.5);
    }

    #[test]
    fn reads_background_styling() {
        let (config, recovered) = parse(json!({ "panels": [panel(json!({
            "bgImage": "C:\\pics\\sky.png",
            "bgImageOpacity": 0.6,
            "bgImageFit": "tile",
            "texture": "vines",
            "textureColor": "#112233",
            "textureOpacity": 0.2,
            "textureScale": 1.5,
            "textureRandom": true,
            "bgEffect": "pixelate",
            "bgEffectStrength": 1,
        }))] }));
        assert!(!recovered);
        let p = &config.panels[0];
        assert_eq!(p.bg_image, "C:\\pics\\sky.png");
        assert_eq!(p.bg_image_opacity, 0.6);
        assert_eq!(p.bg_image_fit, ImageFit::Tile);
        assert_eq!(p.texture, BgTexture::Vines);
        assert_eq!(p.texture_color, "#112233");
        assert_eq!(p.texture_opacity, 0.2);
        assert_eq!(p.texture_scale, 1.5);
        assert!(p.texture_random);
        assert_eq!(p.bg_effect, BgEffect::Pixelate);
        assert_eq!(p.bg_effect_strength, 1.0);
    }

    #[test]
    fn turns_the_old_glossy_effect_into_blur() {
        let (config, recovered) =
            parse(json!({ "panels": [panel(json!({ "bgEffect": "glossy" }))] }));
        assert!(!recovered);
        assert_eq!(config.panels[0].bg_effect, BgEffect::Blur);
    }

    #[test]
    fn drops_a_panel_with_bad_background_styling() {
        for bad in [
            json!({ "bgImage": 3 }),
            json!({ "bgImageOpacity": 1.5 }),
            json!({ "bgImageFit": "cover" }),
            json!({ "texture": "plaid" }),
            json!({ "textureColor": "grey" }),
            json!({ "textureOpacity": -0.1 }),
            json!({ "textureScale": 0.2 }),
            json!({ "textureScale": 3 }),
            json!({ "textureRandom": "yes" }),
            json!({ "bgEffect": "shiny" }),
            json!({ "bgEffectStrength": "high" }),
        ] {
            assert!(parse(json!({ "panels": [panel(bad.clone())] })).0.panels.is_empty(), "{bad}");
        }
    }

    #[test]
    fn leaves_default_background_styling_out_of_the_file() {
        let (config, _) = parse(json!({ "panels": [panel(json!({}))] }));
        let saved = serde_json::to_value(&config.panels[0]).unwrap();
        for key in ["bgImage", "bgImageOpacity", "texture", "textureColor", "bgEffect"] {
            assert!(saved.get(key).is_none(), "{key}");
        }
        let mut p = config.panels[0].clone();
        p.texture = BgTexture::Stars;
        p.bg_effect_strength = 0.8;
        let saved = serde_json::to_value(&p).unwrap();
        assert_eq!(saved["texture"], "stars");
        assert_eq!(saved["bgEffectStrength"], 0.8);
        assert_eq!(parse_panel(&saved).unwrap(), p);
    }

    #[test]
    fn duplicates_a_panel_look_without_its_items() {
        let (config, _) = parse(json!({ "panels": [panel(json!({
            "title": "Games",
            "bgColor": "#123456",
            "headerShape": "folder",
            "texture": "flowers",
            "catchNewItems": true,
            "cells": [{ "name": "a.lnk", "col": 0, "row": 0 }],
            "order": ["a.lnk"],
        }))] }));
        let src = &config.panels[0];
        let b = Bounds::new(50, 60, 300, 200);
        let copy = duplicate_panel(src, b, "C:\\Games (2)");
        assert_ne!(copy.id, src.id);
        assert_eq!(copy.title, "Games copy");
        assert_eq!(copy.bounds, b);
        assert_eq!(copy.folder_path, "C:\\Games (2)");
        assert_eq!(copy.bg_color, "#123456");
        assert_eq!(copy.header_shape, HeaderShape::Folder);
        assert_eq!(copy.texture, BgTexture::Flowers);
        assert!(copy.cells.is_empty() && copy.order.is_empty());
        assert!(!copy.catch_new_items);
        assert!(parse_panel(&serde_json::to_value(&copy).unwrap()).is_ok());
    }

    #[test]
    fn copies_another_panels_look_and_keeps_its_own_things() {
        let (config, _) = parse(json!({ "panels": [
            panel(json!({
                "id": "from", "title": "Games", "folderPath": "C:\\Games",
                "bgColor": "#123456", "bgOpacity": 0.8, "titleBarStyle": "pill",
                "headerShape": "folder", "iconSize": "large", "texture": "cracks",
                "textureScale": 1.5, "bgEffect": "blur", "bgImage": "C:\\sky.png",
                "collapseDirection": "left", "sortBy": "name", "catchNewItems": true,
            })),
            panel(json!({
                "id": "to", "title": "Work", "folderPath": "C:\\Work", "collapsed": true,
                "collapseDirection": "down", "sortBy": "type",
                "bounds": { "x": 500, "y": 400, "width": 300, "height": 200 },
                "cells": [{ "name": "a.lnk", "col": 1, "row": 0 }],
            })),
        ] }));
        let (from, to) = (&config.panels[0], &config.panels[1]);
        let p = with_look_of(to, from);
        // The look comes from `from`...
        assert_eq!(p.bg_color, "#123456");
        assert_eq!(p.bg_opacity, 0.8);
        assert_eq!(p.title_bar_style, TitleBarStyle::Pill);
        assert_eq!(p.header_shape, HeaderShape::Folder);
        assert_eq!(p.icon_size, IconSize::Large);
        assert_eq!(p.texture, BgTexture::Cracks);
        assert_eq!(p.texture_scale, 1.5);
        assert_eq!(p.bg_effect, BgEffect::Blur);
        assert_eq!(p.bg_image, "C:\\sky.png");
        // ...and the rest stays `to`'s.
        assert_eq!(
            (p.id.as_str(), p.title.as_str(), p.folder_path.as_str()),
            ("to", "Work", "C:\\Work")
        );
        assert_eq!(p.bounds, to.bounds);
        assert!(p.collapsed && !p.catch_new_items);
        assert_eq!(p.collapse_direction, CollapseDirection::Down);
        assert_eq!(p.sort_by, SortBy::Type);
        assert_eq!(p.cells, to.cells);
    }

    #[test]
    fn duplicate_keeps_a_long_title_within_the_limit() {
        let (config, _) = parse(json!({ "panels": [panel(json!({ "title": "é".repeat(200) }))] }));
        let copy = duplicate_panel(&config.panels[0], Bounds::new(0, 0, 300, 200), "C:\\x");
        assert!(js_len(&copy.title) <= 200);
        assert!(copy.title.ends_with(" copy"));
        let empty = PanelConfig { title: String::new(), ..config.panels[0].clone() };
        assert_eq!(duplicate_panel(&empty, Bounds::new(0, 0, 300, 200), "C:\\x").title, "");
    }

    #[test]
    fn switches_name_sorted_v1_panels_to_custom_keeping_other_sorts() {
        let (config, _) = parse(json!({
            "version": 1,
            "panels": [panel(json!({ "id": "a", "sortBy": "name" })), panel(json!({ "id": "b", "sortBy": "type" }))]
        }));
        assert_eq!(config.version, 2);
        let sorts: Vec<_> = config.panels.iter().map(|p| p.sort_by).collect();
        assert_eq!(sorts, [SortBy::Custom, SortBy::Type]);
    }

    #[test]
    fn leaves_a_version_2_config_as_it_is() {
        let (config, _) =
            parse(json!({ "version": 2, "panels": [panel(json!({ "sortBy": "name" }))] }));
        assert_eq!(config.panels[0].sort_by, SortBy::Name);
    }

    #[test]
    fn turns_old_pill_and_minimal_shapes_into_title_bar_styles() {
        let (config, recovered) = parse(json!({
            "version": 2,
            "panels": [
                panel(json!({ "id": "a", "headerShape": "pill" })),
                panel(json!({ "id": "b", "headerShape": "minimal" })),
                panel(json!({ "id": "c", "headerShape": "phone", "titleBarStyle": "soft" })),
            ]
        }));
        assert!(!recovered);
        let looks: Vec<_> =
            config.panels.iter().map(|p| (p.header_shape, p.title_bar_style)).collect();
        assert_eq!(
            looks,
            [
                (HeaderShape::Default, TitleBarStyle::Pill),
                (HeaderShape::Default, TitleBarStyle::Minimal),
                (HeaderShape::Phone, TitleBarStyle::Soft),
            ]
        );
    }

    #[test]
    fn drops_a_panel_with_an_unknown_shape_or_style() {
        assert!(
            parse(json!({ "panels": [panel(json!({ "headerShape": "round" }))] }))
                .0
                .panels
                .is_empty()
        );
        assert!(
            parse(json!({ "panels": [panel(json!({ "titleBarStyle": "glow" }))] }))
                .0
                .panels
                .is_empty()
        );
    }

    #[test]
    fn rejects_bad_spots() {
        let ok = json!([{ "name": "a.lnk", "col": 2, "row": 5 }]);
        let (config, _) = parse(json!({ "panels": [panel(json!({ "cells": ok }))] }));
        assert_eq!(config.panels[0].cells, [PlacedItem::new("a.lnk", 2, 5)]);
        for bad in [
            json!([{ "name": "a", "col": -1, "row": 0 }]),
            json!([{ "name": "a", "col": 1.5, "row": 0 }]),
            json!([{ "name": "", "col": 0, "row": 0 }]),
            json!([{ "name": "a", "col": 0 }]),
        ] {
            assert!(
                parse(json!({ "panels": [panel(json!({ "cells": bad }))] })).0.panels.is_empty()
            );
        }
    }

    #[test]
    fn rejects_a_bad_custom_order() {
        assert!(parse(json!({ "panels": [panel(json!({ "order": [""] }))] })).0.panels.is_empty());
        assert!(parse(json!({ "panels": [panel(json!({ "order": "a" }))] })).0.panels.is_empty());
    }

    #[test]
    fn drops_only_the_invalid_panels() {
        let (config, recovered) = parse(json!({
            "locked": true,
            "panels": [panel(json!({ "id": "ok" })), panel(json!({ "id": "bad", "bgOpacity": 7 }))]
        }));
        assert!(recovered);
        assert!(config.locked);
        assert_eq!(config.panels.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["ok"]);
    }

    #[test]
    fn rejects_bad_colors_and_null_fields() {
        assert!(
            parse(json!({ "panels": [panel(json!({ "bgColor": "red" }))] })).0.panels.is_empty()
        );
        assert!(
            parse(json!({ "panels": [panel(json!({ "collapsed": null }))] })).0.panels.is_empty()
        );
        assert!(parse(json!({ "panels": [panel(json!({ "bounds": { "x": 0, "y": 0, "width": 10, "height": 200 } }))] })).0.panels.is_empty());
    }

    #[test]
    fn turns_snapping_and_stacking_on_by_default() {
        let (config, _) = parse(json!({ "panels": [] }));
        assert!(config.snapping && config.stacking);
    }

    #[test]
    fn copies_added_items_by_default_and_rejects_an_unknown_mode() {
        assert_eq!(parse(json!({ "panels": [] })).0.add_mode, AddMode::Copy);
        assert_eq!(parse(json!({ "panels": [], "addMode": "move" })).0.add_mode, AddMode::Move);
        let (bad, recovered) = parse(json!({ "panels": [], "addMode": "link" }));
        assert!(recovered);
        assert_eq!(bad.add_mode, AddMode::Copy);
    }

    #[test]
    fn falls_back_to_defaults_for_garbage() {
        assert!(parse(json!("nope")).0.panels.is_empty());
        assert!(parse_config(None).1);
    }

    #[test]
    fn loads_a_config_written_by_the_electron_app() {
        let text = include_str!("../fixtures/electron-config.json");
        let raw: Value = serde_json::from_str(text).unwrap();
        let (config, recovered) = parse_config(Some(&raw));
        assert!(!recovered);
        assert_eq!(config.panels.len(), 3);
        let apps = &config.panels[0];
        assert_eq!(apps.bounds, Bounds::new(17, 14, 329, 535));
        assert_eq!(apps.header_shape, HeaderShape::Phone);
        assert_eq!(apps.title_bar_style, TitleBarStyle::Pill);
        assert_eq!(apps.collapse_align, CollapseAlign::Center);
        assert_eq!(apps.cells[1], PlacedItem::new("Syncthing.lnk", 1, 0));
        assert_eq!(config.add_mode, AddMode::Move);
        // Writing it back gives the same JSON, so the Electron app could still read it.
        let rewritten: Value = serde_json::to_value(&config).unwrap();
        assert_eq!(rewritten, raw);
    }

    #[test]
    fn makes_valid_unique_uuids() {
        let a = random_uuid();
        let b = random_uuid();
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        assert_eq!(&a[14..15], "4");
        assert!(matches!(&a[19..20], "8" | "9" | "a" | "b"));
    }

    #[test]
    fn folder_name_strips_characters_windows_forbids() {
        assert_eq!(folder_name_from_title("Work: \"Q3\" <stuff>?"), "Work Q3 stuff");
    }

    #[test]
    fn folder_name_handles_empty_and_reserved_names() {
        assert_eq!(folder_name_from_title("  ...  "), "Panel");
        assert_eq!(folder_name_from_title("CON"), "CON_");
        assert_eq!(folder_name_from_title("com3"), "com3_");
        assert_eq!(folder_name_from_title("Computers"), "Computers");
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sorter-{name}-{}", random_uuid()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn store_starts_empty_then_persists_updates() {
        let dir = temp_dir("store");
        let file = dir.join("config.json");
        let mut store = ConfigStore::load(file.clone());
        assert!(store.config().panels.is_empty());
        assert!(!store.is_dirty());

        let p =
            create_panel("Apps", Bounds::new(10, 20, 300, 200), dir.join("Apps").to_str().unwrap());
        let id = p.id.clone();
        store.update(|c| c.panels.push(p));
        store.update_panel(&id, |p| p.collapsed = true);
        store.flush().unwrap();
        assert!(!store.is_dirty());

        let reloaded = ConfigStore::load(file);
        assert_eq!(reloaded.config().panels.len(), 1);
        assert!(reloaded.config().panels[0].collapsed);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn store_reads_a_file_with_a_byte_order_mark() {
        let dir = temp_dir("bom");
        let file = dir.join("config.json");
        std::fs::write(&file, "\u{feff}{\"locked\": true}").unwrap();
        let store = ConfigStore::load(file);
        assert!(store.config().locked);
        assert!(!dir.join("config.bad.json").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn store_backs_up_a_corrupt_file_and_recovers() {
        let dir = temp_dir("corrupt");
        let file = dir.join("config.json");
        std::fs::write(&file, "{ not json").unwrap();
        let store = ConfigStore::load(file.clone());
        assert!(store.config().panels.is_empty());
        assert_eq!(std::fs::read_to_string(dir.join("config.bad.json")).unwrap(), "{ not json");
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(saved["version"], 2);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
