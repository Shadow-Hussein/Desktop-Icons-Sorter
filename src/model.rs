//! Config types and size constants, shared by the logic and the UI.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IconSize {
    Small,
    Medium,
    Large,
}

/// `Custom` keeps the order the user arranged by dragging (`PanelConfig::cells`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortBy {
    Custom,
    Name,
    Type,
    Modified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CollapseDirection {
    Up,
    Down,
    Left,
    Right,
}

impl CollapseDirection {
    pub const ALL: [CollapseDirection; 4] = [Self::Up, Self::Down, Self::Left, Self::Right];

    pub fn opposite(self) -> Self {
        match self {
            Self::Up => Self::Down,
            Self::Down => Self::Up,
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }

    /// Left and right panels have a vertical title bar and collapse sideways.
    pub fn is_sideways(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

/// What a panel collapses to: its title bar, a card with a row of its apps
/// (phone, "Dock") or a square with four of them (folder, "Tile").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HeaderShape {
    Default,
    Phone,
    Folder,
}

/// How the title bar looks, on any shape. `Soft` fades the bar out instead of hard edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TitleBarStyle {
    Standard,
    Pill,
    Minimal,
    Soft,
}

/// Where a collapsed tile sits along its title bar's edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CollapseAlign {
    Start,
    Center,
    End,
}

/// How a panel takes in items from outside: copies them, or moves them out of where they were.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AddMode {
    Copy,
    Move,
}

/// A finish drawn over a panel's background. One at a time, at a strength.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BgEffect {
    #[default]
    None,
    /// Frosted glass: the wallpaper behind the panel and its picture, blurred.
    Blur,
    /// Big square pixels: the wallpaper behind the panel, its picture and its texture.
    Pixelate,
    Grain,
    Vignette,
    Gradient,
    Scanlines,
}

impl BgEffect {
    pub const ALL: [BgEffect; 7] = [
        Self::None,
        Self::Blur,
        Self::Pixelate,
        Self::Grain,
        Self::Vignette,
        Self::Gradient,
        Self::Scanlines,
    ];

    /// Blur and pixelate change the wallpaper behind the panel, so the panel shows it.
    pub fn uses_wallpaper(self) -> bool {
        matches!(self, Self::Blur | Self::Pixelate)
    }
}

/// A pattern tiled over a panel's background, in one color.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BgTexture {
    #[default]
    None,
    Flowers,
    Vines,
    Leaves,
    Dots,
    Stripes,
    Waves,
    Stars,
    Hearts,
    Cracks,
}

impl BgTexture {
    pub const ALL: [BgTexture; 10] = [
        Self::None,
        Self::Flowers,
        Self::Vines,
        Self::Leaves,
        Self::Dots,
        Self::Stripes,
        Self::Waves,
        Self::Stars,
        Self::Hearts,
        Self::Cracks,
    ];
}

/// How a background image fills the panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFit {
    /// Covers the panel, cropping what doesn't fit.
    #[default]
    Fill,
    /// Shows all of it, centered.
    Fit,
    Stretch,
    /// Repeats it at its own size.
    Tile,
}

/// A window edge or corner being dragged to resize.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResizeEdge {
    pub n: bool,
    pub s: bool,
    pub e: bool,
    pub w: bool,
}

impl ResizeEdge {
    pub const N: Self = Self::new(true, false, false, false);
    pub const S: Self = Self::new(false, true, false, false);
    pub const E: Self = Self::new(false, false, true, false);
    pub const W: Self = Self::new(false, false, false, true);
    pub const NE: Self = Self::new(true, false, true, false);
    pub const NW: Self = Self::new(true, false, false, true);
    pub const SE: Self = Self::new(false, true, true, false);
    pub const SW: Self = Self::new(false, true, false, true);

    pub const fn new(n: bool, s: bool, e: bool, w: bool) -> Self {
        Self { n, s, e, w }
    }

    /// The same edge without its north/south part.
    pub fn horizontal_only(self) -> Self {
        Self { n: false, s: false, ..self }
    }

    /// The same edge without its east/west part.
    pub fn vertical_only(self) -> Self {
        Self { e: false, w: false, ..self }
    }
}

/// An item the user put in a spot of a panel's icon grid, counted from the top left.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacedItem {
    /// File name.
    pub name: String,
    pub col: u32,
    pub row: u32,
}

impl PlacedItem {
    pub fn new(name: impl Into<String>, col: u32, row: u32) -> Self {
        Self { name: name.into(), col, row }
    }
}

/// Screen rectangle in DIPs, as Electron saved it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Bounds {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self { x, y, width, height }
    }

    pub fn right(&self) -> i32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.height
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelConfig {
    pub id: String,
    pub title: String,
    /// Expanded bounds. When collapsed, the window shrinks to the title bar only.
    pub bounds: Bounds,
    pub collapsed: bool,
    /// Which edge the title bar sits on; the panel collapses toward it.
    pub collapse_direction: CollapseDirection,
    pub header_shape: HeaderShape,
    pub title_bar_style: TitleBarStyle,
    pub collapse_align: CollapseAlign,
    /// Takes part in stacking (see `AppConfig::stacking`).
    pub stackable: bool,
    /// Background color as #rrggbb.
    pub bg_color: String,
    /// Background opacity 0–1. Icons and text are not affected.
    #[serde(serialize_with = "js_number")]
    pub bg_opacity: f64,
    pub title_bar_color: String,
    #[serde(serialize_with = "js_number")]
    pub title_bar_opacity: f64,
    pub title_text_color: String,
    /// Real folder whose contents this panel shows.
    pub folder_path: String,
    pub icon_size: IconSize,
    pub sort_by: SortBy,
    /// Where the user put items in custom order, gaps and all.
    pub cells: Vec<PlacedItem>,
    /// File names in the order they arrived, for items without a spot in `cells`.
    pub order: Vec<String>,
    /// New items that appear on the user's Desktop go into this panel. At most one panel has it.
    pub catch_new_items: bool,
    // The background's image, texture and effect. Left out of the file while
    // at their defaults, so a config without them stays as it was.
    /// Image file drawn over the background color; empty for none.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub bg_image: String,
    #[serde(serialize_with = "js_number", skip_serializing_if = "is_default_image_opacity")]
    pub bg_image_opacity: f64,
    #[serde(skip_serializing_if = "is_default")]
    pub bg_image_fit: ImageFit,
    #[serde(skip_serializing_if = "is_default")]
    pub texture: BgTexture,
    #[serde(skip_serializing_if = "is_default_texture_color")]
    pub texture_color: String,
    #[serde(serialize_with = "js_number", skip_serializing_if = "is_default_texture_opacity")]
    pub texture_opacity: f64,
    /// Texture size, `MIN_TEXTURE_SCALE`–`MAX_TEXTURE_SCALE` (1 is its normal size).
    #[serde(serialize_with = "js_number", skip_serializing_if = "is_default_texture_scale")]
    pub texture_scale: f64,
    /// Scatters the texture: each shape a little off its spot and a little bigger or smaller.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub texture_random: bool,
    #[serde(skip_serializing_if = "is_default")]
    pub bg_effect: BgEffect,
    /// How strong the effect is, 0–1.
    #[serde(serialize_with = "js_number", skip_serializing_if = "is_default_effect_strength")]
    pub bg_effect_strength: f64,
}

pub const DEFAULT_IMAGE_OPACITY: f64 = 1.0;
pub const DEFAULT_TEXTURE_COLOR: &str = "#808080";
pub const DEFAULT_TEXTURE_OPACITY: f64 = 0.35;
pub const DEFAULT_EFFECT_STRENGTH: f64 = 0.5;
pub const MIN_TEXTURE_SCALE: f64 = 0.5;
pub const MAX_TEXTURE_SCALE: f64 = 2.0;

fn is_default_texture_scale(v: &f64) -> bool {
    *v == 1.0
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

fn is_default_image_opacity(v: &f64) -> bool {
    *v == DEFAULT_IMAGE_OPACITY
}

fn is_default_texture_color(v: &str) -> bool {
    v.eq_ignore_ascii_case(DEFAULT_TEXTURE_COLOR)
}

fn is_default_texture_opacity(v: &f64) -> bool {
    *v == DEFAULT_TEXTURE_OPACITY
}

fn is_default_effect_strength(v: &f64) -> bool {
    *v == DEFAULT_EFFECT_STRENGTH
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    /// 1: before custom order existed (see `config::migrate`).
    pub version: u8,
    pub panels: Vec<PanelConfig>,
    pub start_with_windows: bool,
    /// When locked, panels cannot be moved or resized.
    pub locked: bool,
    /// Moving or resizing a panel snaps its edges to screen edges and other panels.
    pub snapping: bool,
    /// Collapsing a panel moves the panels stacked against its far edge to fill the space.
    pub stacking: bool,
    /// Whether items dropped on a panel or caught from the desktop are copied or moved.
    /// Items from another panel always move.
    pub add_mode: AddMode,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: 2,
            panels: Vec::new(),
            start_with_windows: false,
            locked: false,
            snapping: true,
            stacking: true,
            add_mode: AddMode::Copy,
        }
    }
}

impl AppConfig {
    pub fn panel(&self, id: &str) -> Option<&PanelConfig> {
        self.panels.iter().find(|p| p.id == id)
    }

    pub fn panel_mut(&mut self, id: &str) -> Option<&mut PanelConfig> {
        self.panels.iter_mut().find(|p| p.id == id)
    }
}

/// Writes whole numbers without a fraction, as `JSON.stringify` did (1, not 1.0).
fn js_number<S: serde::Serializer>(n: &f64, s: S) -> Result<S::Ok, S::Error> {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        s.serialize_i64(*n as i64)
    } else {
        s.serialize_f64(*n)
    }
}

pub const TITLE_BAR_HEIGHT: i32 = 30;
/// How long a panel takes to collapse or expand.
pub const COLLAPSE_ANIMATION_MS: u32 = 200;
/// The row of apps under a collapsed phone panel's title: a 32 px icon plus padding.
pub const PHONE_PEEK_SIZE: i32 = 48;
/// A collapsed folder panel's square: the title bar over a 2 x 2 grid of 36 px apps.
pub const FOLDER_CARD_SIZE: i32 = 122;
pub const MIN_PANEL_WIDTH: i32 = 140;
pub const MIN_PANEL_HEIGHT: i32 = 90;
/// Most names a panel's saved order or spots keep, so config.json stays small and valid.
pub const MAX_PANEL_ITEMS: usize = 10_000;

pub fn icon_pixels(size: IconSize) -> i32 {
    match size {
        IconSize::Small => 32,
        IconSize::Medium => 48,
        IconSize::Large => 64,
    }
}

/// How far a collapsed panel reaches from its title bar's edge.
pub fn collapsed_bar_size(shape: HeaderShape) -> i32 {
    match shape {
        HeaderShape::Folder => FOLDER_CARD_SIZE,
        HeaderShape::Phone => TITLE_BAR_HEIGHT + PHONE_PEEK_SIZE,
        HeaderShape::Default => TITLE_BAR_HEIGHT,
    }
}

/// A collapsed panel's length along its title bar, or None to keep the panel's own.
pub fn collapsed_length(shape: HeaderShape) -> Option<i32> {
    (shape == HeaderShape::Folder).then_some(FOLDER_CARD_SIZE)
}
