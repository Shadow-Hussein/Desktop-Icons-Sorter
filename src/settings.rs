//! The panel settings and app settings windows, drawn with the same Direct2D
//! code as the panels: picture tiles, segmented buttons, switches, sliders
//! and swatches, in the Windows light or dark theme.
//!
//! A page is laid out from the config every time it's drawn or clicked, so
//! what's drawn and what's hit tested always agree.

use std::ffi::c_void;

use sorter::model::{
    AddMode, AppConfig, CollapseAlign, CollapseDirection, HeaderShape, IconSize, PanelConfig,
    SortBy, TitleBarStyle,
};
use sorter::panel_layout::Rect;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreateSolidBrush,
    DEFAULT_CHARSET, DeleteObject, EndPaint, FF_DONTCARE, FW_NORMAL, HBRUSH, HDC, HFONT, HGDIOBJ,
    InvalidateRect, OUT_DEFAULT_PRECIS, PAINTSTRUCT, SetBkColor, SetTextColor,
};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::Controls::{EM_SETLIMITTEXT, WM_MOUSELEAVE};
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use crate::app::{App, Deferred, hinstance, run_deferred, with_app};
use crate::win::render::{Canvas, Font, Rgba};
use crate::win::{Wide, dpi, point_of, wheel_delta};

pub const SETTINGS_CLASS: PCWSTR = w!("DesktopIconsSorter.Settings");
const EN_CHANGE: u32 = 0x0300;
const TITLE_EDIT_ID: usize = 100;

const WIDTH: f32 = 360.0;
const PAD: f32 = 14.0;
const CARD_PAD_X: f32 = 12.0;
const CARD_PAD_Y: f32 = 10.0;

// ------------------------------------------------------------------- theme

#[derive(Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    bg_top: Rgba,
    bg_bottom: Rgba,
    text: Rgba,
    muted: Rgba,
    surface: Rgba,
    surface_border: Rgba,
    line: Rgba,
    field: Rgba,
    field_hover: Rgba,
    track: Rgba,
    btn: Rgba,
    btn_hover: Rgba,
    art: Rgba,
    art_bar: Rgba,
    /// The title field's color, opaque, for the native edit control.
    field_solid: u32,
}

const ACCENT_2: Rgba = Rgba(0x7b as f32 / 255.0, 0x61 as f32 / 255.0, 1.0, 1.0);
const DANGER: Rgba = Rgba(0xe5 as f32 / 255.0, 0x48 as f32 / 255.0, 0x4d as f32 / 255.0, 1.0);

impl Theme {
    pub fn current() -> Self {
        if apps_use_dark_theme() { Self::dark() } else { Self::light() }
    }

    fn light() -> Self {
        let ink = |a| Rgba(20.0 / 255.0, 30.0 / 255.0, 60.0 / 255.0, a);
        Self {
            dark: false,
            bg_top: Rgba::hex("#eef3fb", 1.0),
            bg_bottom: Rgba::hex("#f3effb", 1.0),
            text: Rgba::hex("#1a1c23", 1.0),
            muted: Rgba::hex("#1a1c23", 0.6),
            surface: Rgba::white(0.6),
            surface_border: Rgba::white(0.95),
            line: ink(0.07),
            field: ink(0.06),
            field_hover: ink(0.1),
            track: ink(0.16),
            btn: Rgba::white(0.9),
            btn_hover: Rgba::white(1.0),
            art: ink(0.12),
            art_bar: ink(0.38),
            field_solid: 0x00f1edea,
        }
    }

    fn dark() -> Self {
        Self {
            dark: true,
            bg_top: Rgba::hex("#131827", 1.0),
            bg_bottom: Rgba::hex("#1b1528", 1.0),
            text: Rgba::hex("#eceef4", 1.0),
            muted: Rgba::hex("#eceef4", 0.6),
            surface: Rgba::white(0.055),
            surface_border: Rgba::white(0.09),
            line: Rgba::white(0.07),
            field: Rgba::white(0.07),
            field_hover: Rgba::white(0.12),
            track: Rgba::white(0.18),
            btn: Rgba::white(0.1),
            btn_hover: Rgba::white(0.16),
            art: Rgba::white(0.1),
            art_bar: Rgba::white(0.36),
            field_solid: 0x004a3b33,
        }
    }
}

fn apps_use_dark_theme() -> bool {
    let (k, v) = (
        Wide::new("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
        Wide::new("AppsUseLightTheme"),
    );
    let mut value = 1u32;
    let mut len = 4u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            k.pcwstr(),
            v.pcwstr(),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut _ as *mut c_void),
            Some(&mut len),
        )
    }
    .is_ok();
    ok && value == 0
}

// ------------------------------------------------------------------- page

/// What clicking a control does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Act {
    None,
    Shape(HeaderShape),
    Style(TitleBarStyle),
    Size(IconSize),
    Direction(CollapseDirection),
    Align(CollapseAlign),
    Collapsed,
    Stackable,
    Catch,
    Opacity(Layer),
    Color(Layer),
    Preset(usize),
    SortMenu,
    ChangeFolder,
    Delete,
    AddMode(AddMode),
    Snapping,
    Locked,
    Stacking,
    StartWithWindows,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Layer {
    Background,
    Bar,
    Text,
}

enum Look {
    Text { text: String, font: Font, muted: bool },
    Card,
    Line,
    Switch { on: bool, enabled: bool },
    SegGroup,
    Segment { label: SegLabel, selected: bool },
    Tile { art: Art, label: &'static str, selected: bool },
    Slider { value: f32 },
    Badge(String),
    Swatch(String),
    Preset { bg: &'static str, bar: &'static str, text: &'static str, active: bool },
    Button { label: &'static str, danger: bool },
    Dropdown(&'static str),
    Field,
    Path(String),
    Preview,
}

#[derive(Clone, Copy)]
enum SegLabel {
    Text(&'static str),
    Arrow(f32),
}

#[derive(Clone, Copy)]
enum Art {
    Shape(HeaderShape),
    Style(TitleBarStyle),
}

struct Ctl {
    r: Rect,
    look: Look,
    act: Act,
}

const PRESETS: [(&str, &str, &str, &str); 6] = [
    ("Charcoal", "#1e1e1e", "#000000", "#ffffff"),
    ("Ocean", "#0b3d91", "#06265c", "#ffffff"),
    ("Forest", "#1f5130", "#123520", "#ffffff"),
    ("Plum", "#4a1f5c", "#2f1239", "#ffffff"),
    ("Ember", "#7a2e12", "#4d1c0a", "#ffffff"),
    ("Frost", "#f2f4f7", "#d5dae1", "#111111"),
];

const SHAPES: [(HeaderShape, &str, &str); 3] = [
    (HeaderShape::Default, "Default", "A title bar along the edge."),
    (HeaderShape::Phone, "Dock", "Collapses to a card with a row of its apps."),
    (HeaderShape::Folder, "Tile", "Collapses to a square with a 2 × 2 grid of its apps."),
];

const STYLES: [(TitleBarStyle, &str, &str); 4] = [
    (TitleBarStyle::Standard, "Standard", "A solid bar with a line under it."),
    (TitleBarStyle::Pill, "Pill", "A rounded capsule, inset from the panel's edges."),
    (
        TitleBarStyle::Minimal,
        "Minimal",
        "Just the title, no bar. The collapse button shows when you point at the panel.",
    ),
    (
        TitleBarStyle::Soft,
        "Soft",
        "No hard edges: the bar fades out at its ends and blends into the panel.",
    ),
];

/// Lays out a page top to bottom.
struct Builder<'a> {
    gfx: &'a crate::win::render::Gfx,
    items: Vec<Ctl>,
    y: f32,
    /// Inside the current card.
    x: f32,
    w: f32,
    first_row: bool,
}

impl Builder<'_> {
    fn push(&mut self, r: Rect, look: Look, act: Act) {
        self.items.push(Ctl { r, look, act });
    }

    fn text_h(&self, text: &str, font: Font, w: f32) -> f32 {
        self.gfx.measure(text, font, w).1
    }

    /// A card with a heading; `body` adds its rows.
    fn section(&mut self, heading: &str, body: impl FnOnce(&mut Self)) {
        let top = self.y + 12.0;
        let card_at = self.items.len();
        self.x = PAD + CARD_PAD_X;
        self.w = WIDTH - 2.0 * PAD - 2.0 * CARD_PAD_X;
        self.y = top + CARD_PAD_Y;
        self.push(
            Rect::new(self.x, self.y, self.w, 14.0),
            Look::Text { text: heading.to_uppercase(), font: Font::Heading, muted: true },
            Act::None,
        );
        self.y += 18.0;
        self.first_row = true;
        body(self);
        self.y += CARD_PAD_Y;
        let card = Rect::new(PAD, top, WIDTH - 2.0 * PAD, self.y - top);
        self.items.insert(card_at, Ctl { r: card, look: Look::Card, act: Act::None });
    }

    fn separator(&mut self) {
        if !self.first_row {
            self.push(Rect::new(self.x, self.y, self.w, 1.0), Look::Line, Act::None);
        }
        self.first_row = false;
    }

    /// Label (and hint) on the left, a control `cw` x `ch` on the right.
    fn row(
        &mut self,
        label: &str,
        hint: Option<&str>,
        cw: f32,
        ch: f32,
        make: impl FnOnce(&mut Self, Rect),
    ) {
        self.separator();
        let lw = self.w - cw - 12.0;
        let lh = self.text_h(label, Font::Body, lw);
        let hh = hint.map_or(0.0, |h| self.text_h(h, Font::Small, lw) + 2.0);
        let h = if hint.is_some() { (lh + hh + 14.0).max(38.0) } else { 38.0f32.max(ch + 8.0) };
        let ty = self.y + (h - lh - hh) / 2.0;
        self.push(
            Rect::new(self.x, ty, lw, lh + 1.0),
            Look::Text { text: label.into(), font: Font::Body, muted: false },
            Act::None,
        );
        if let Some(hint) = hint {
            self.push(
                Rect::new(self.x, ty + lh + 2.0, lw, hh),
                Look::Text { text: hint.into(), font: Font::Small, muted: true },
                Act::None,
            );
        }
        let c = Rect::new(self.x + self.w - cw, self.y + (h - ch) / 2.0, cw, ch);
        make(self, c);
        self.y += h;
    }

    /// Label and hint on top, the control full width under them.
    fn stacked(&mut self, label: &str, hint: &str, ch: f32, make: impl FnOnce(&mut Self, Rect)) {
        self.separator();
        self.y += 9.0;
        let lh = self.text_h(label, Font::Body, self.w);
        self.push(
            Rect::new(self.x, self.y, self.w, lh + 1.0),
            Look::Text { text: label.into(), font: Font::Body, muted: false },
            Act::None,
        );
        self.y += lh + 2.0;
        let hh = self.text_h(hint, Font::Small, self.w);
        self.push(
            Rect::new(self.x, self.y, self.w, hh + 1.0),
            Look::Text { text: hint.into(), font: Font::Small, muted: true },
            Act::None,
        );
        self.y += hh + 8.0;
        let c = Rect::new(self.x, self.y, self.w, ch);
        make(self, c);
        self.y += ch + 10.0;
    }

    fn switch(&mut self, label: &str, hint: Option<&str>, on: bool, enabled: bool, act: Act) {
        self.row(label, hint, 38.0, 22.0, |b, r| {
            b.push(r, Look::Switch { on, enabled }, if enabled { act } else { Act::None })
        });
    }

    fn seg_width(&self, labels: &[SegLabel]) -> Vec<f32> {
        labels
            .iter()
            .map(|l| match l {
                SegLabel::Text(t) => {
                    (self.gfx.measure(t, Font::ButtonStrong, 500.0).0 + 20.0).max(30.0)
                }
                SegLabel::Arrow(_) => 30.0,
            })
            .collect()
    }

    /// A segmented control: one choice of several, in a row.
    fn segmented(&mut self, label: &str, hint: Option<&str>, options: Vec<(SegLabel, Act, bool)>) {
        let labels: Vec<SegLabel> = options.iter().map(|o| o.0).collect();
        let widths = self.seg_width(&labels);
        let total = widths.iter().sum::<f32>() + 2.0 * (widths.len() - 1) as f32 + 6.0;
        self.row(label, hint, total, 32.0, |b, r| {
            b.push(r, Look::SegGroup, Act::None);
            let mut x = r.x + 3.0;
            for ((label, act, selected), w) in options.into_iter().zip(widths) {
                b.push(Rect::new(x, r.y + 3.0, w, 26.0), Look::Segment { label, selected }, act);
                x += w + 2.0;
            }
        });
    }
}

fn panel_page(
    gfx: &crate::win::render::Gfx,
    p: &PanelConfig,
    app: &AppConfig,
    scroll: f32,
) -> (Vec<Ctl>, f32) {
    let mut b = Builder {
        gfx,
        items: Vec::new(),
        y: PAD - scroll,
        x: PAD,
        w: WIDTH - 2.0 * PAD,
        first_row: true,
    };
    b.push(Rect::new(PAD, b.y, WIDTH - 2.0 * PAD, 68.0), Look::Preview, Act::None);
    b.y += 68.0;

    b.section("Title", |b| {
        b.push(Rect::new(b.x, b.y, b.w, 34.0), Look::Field, Act::None);
        b.y += 34.0;
    });

    b.section("Presets", |b| {
        let mut x = b.x;
        for (n, (_, bg, bar, text)) in PRESETS.iter().enumerate() {
            let active = p.bg_color.eq_ignore_ascii_case(bg)
                && p.title_bar_color.eq_ignore_ascii_case(bar)
                && p.title_text_color.eq_ignore_ascii_case(text);
            b.push(
                Rect::new(x, b.y + 2.0, 32.0, 32.0),
                Look::Preset { bg, bar, text, active },
                Act::Preset(n),
            );
            x += 32.0 + 12.0;
        }
        b.y += 36.0;
    });

    b.section("Look", |b| {
        let shape = SHAPES.iter().find(|s| s.0 == p.header_shape).unwrap_or(&SHAPES[0]);
        b.stacked("Title bar shape", shape.2, 62.0, |b, r| {
            let w = (r.w - 2.0 * 6.0) / 3.0;
            for (n, (v, name, _)) in SHAPES.iter().enumerate() {
                let t = Rect::new(r.x + n as f32 * (w + 6.0), r.y, w, r.h);
                b.push(
                    t,
                    Look::Tile { art: Art::Shape(*v), label: name, selected: *v == p.header_shape },
                    Act::Shape(*v),
                );
            }
        });
        let style = STYLES.iter().find(|s| s.0 == p.title_bar_style).unwrap_or(&STYLES[0]);
        b.stacked("Title bar style", style.2, 62.0, |b, r| {
            let w = (r.w - 3.0 * 6.0) / 4.0;
            for (n, (v, name, _)) in STYLES.iter().enumerate() {
                let t = Rect::new(r.x + n as f32 * (w + 6.0), r.y, w, r.h);
                b.push(
                    t,
                    Look::Tile {
                        art: Art::Style(*v),
                        label: name,
                        selected: *v == p.title_bar_style,
                    },
                    Act::Style(*v),
                );
            }
        });
        let color_row =
            |b: &mut Builder, label: &str, hex: &str, opacity: Option<f64>, layer: Layer| {
                let cw = if opacity.is_some() { 26.0 + 10.0 + 120.0 + 10.0 + 46.0 } else { 26.0 };
                let hex = hex.to_string();
                b.row(label, None, cw, 26.0, |b, r| {
                    b.push(Rect::new(r.x, r.y, 26.0, 26.0), Look::Swatch(hex), Act::Color(layer));
                    if let Some(o) = opacity {
                        b.push(
                            Rect::new(r.x + 36.0, r.y + 2.0, 120.0, 22.0),
                            Look::Slider { value: o as f32 },
                            Act::Opacity(layer),
                        );
                        let pct = format!("{}%", (o * 100.0).round());
                        b.push(
                            Rect::new(r.x + 166.0, r.y + 4.0, 46.0, 18.0),
                            Look::Badge(pct),
                            Act::None,
                        );
                    }
                });
            };
        color_row(b, "Background", &p.bg_color, Some(p.bg_opacity), Layer::Background);
        color_row(b, "Title bar", &p.title_bar_color, Some(p.title_bar_opacity), Layer::Bar);
        color_row(b, "Title text", &p.title_text_color, None, Layer::Text);
    });

    b.section("Icons", |b| {
        let sizes =
            [(IconSize::Small, "Small"), (IconSize::Medium, "Medium"), (IconSize::Large, "Large")];
        b.segmented(
            "Size",
            None,
            sizes
                .iter()
                .map(|(v, l)| (SegLabel::Text(l), Act::Size(*v), *v == p.icon_size))
                .collect(),
        );
        let hint = if p.sort_by == SortBy::Custom {
            "Drag icons to arrange them. New ones go at the end."
        } else {
            "Dragging an icon to a new spot switches to custom order."
        };
        let label = match p.sort_by {
            SortBy::Custom => "Custom",
            SortBy::Name => "Name",
            SortBy::Type => "Type",
            SortBy::Modified => "Date modified",
        };
        b.row("Sort by", Some(hint), 150.0, 30.0, |b, r| {
            b.push(r, Look::Dropdown(label), Act::SortMenu)
        });
    });

    b.section("Collapsing", |b| {
        b.switch("Collapsed", None, p.collapsed, true, Act::Collapsed);
        let dirs = [
            (CollapseDirection::Up, 0.0),
            (CollapseDirection::Down, 180.0),
            (CollapseDirection::Left, -90.0),
            (CollapseDirection::Right, 90.0),
        ];
        b.segmented(
            "Collapse toward",
            None,
            dirs.iter()
                .map(|(v, a)| (SegLabel::Arrow(*a), Act::Direction(*v), *v == p.collapse_direction))
                .collect(),
        );
        if p.header_shape == HeaderShape::Folder {
            let side = p.collapse_direction.is_sideways();
            let aligns = [
                (CollapseAlign::Start, if side { "Top" } else { "Left" }),
                (CollapseAlign::Center, "Center"),
                (CollapseAlign::End, if side { "Bottom" } else { "Right" }),
            ];
            b.segmented(
                "Tile position",
                None,
                aligns
                    .iter()
                    .map(|(v, l)| (SegLabel::Text(l), Act::Align(*v), *v == p.collapse_align))
                    .collect(),
            );
        }
        let hint = if app.stacking {
            "Moves the panels stacked against it as it collapses, and moves with them."
        } else {
            "Stacking is off for all panels in app settings."
        };
        b.switch("Stack with other panels", Some(hint), p.stackable, app.stacking, Act::Stackable);
    });

    b.section("New items", |b| {
        let verb = if app.add_mode == AddMode::Copy { "copied" } else { "moved" };
        let hint = format!(
            "Shortcuts, downloads and files that appear on your desktop are {verb} into this panel (change that in app settings). Only one panel catches them."
        );
        b.switch("Catch new desktop items", Some(&hint), p.catch_new_items, true, Act::Catch);
    });

    b.section("Folder", |b| {
        let bw = 80.0;
        b.push(
            Rect::new(b.x, b.y + 2.0, b.w - bw - 10.0, 28.0),
            Look::Path(p.folder_path.clone()),
            Act::None,
        );
        b.push(
            Rect::new(b.x + b.w - bw, b.y + 1.0, bw, 30.0),
            Look::Button { label: "Change…", danger: false },
            Act::ChangeFolder,
        );
        b.y += 32.0;
    });

    b.y += 12.0;
    b.push(
        Rect::new(PAD, b.y, WIDTH - 2.0 * PAD, 34.0),
        Look::Button { label: "Delete panel…", danger: true },
        Act::Delete,
    );
    b.y += 34.0 + 18.0;
    (b.items, b.y + scroll)
}

fn app_page(gfx: &crate::win::render::Gfx, c: &AppConfig, scroll: f32) -> (Vec<Ctl>, f32) {
    let mut b = Builder {
        gfx,
        items: Vec::new(),
        y: 2.0 - scroll,
        x: PAD,
        w: WIDTH - 2.0 * PAD,
        first_row: true,
    };
    b.section("Adding items", |b| {
        let hint = if c.add_mode == AddMode::Copy {
            "Items you drop on a panel, or that it catches from the desktop, are copied. The originals stay where they were."
        } else {
            "Items you drop on a panel, or that it catches from the desktop, move into its folder and leave where they were."
        };
        let modes = [(AddMode::Copy, "Copy"), (AddMode::Move, "Move")];
        b.segmented(
            "Items added to a panel",
            Some(hint),
            modes.iter().map(|(v, l)| (SegLabel::Text(l), Act::AddMode(*v), *v == c.add_mode)).collect(),
        );
    });
    b.section("Moving panels", |b| {
        b.switch(
            "Snap while moving",
            Some("Edges stick to screen edges and to other panels' edges."),
            c.snapping,
            true,
            Act::Snapping,
        );
        b.switch(
            "Lock panels",
            Some("Panels can't be moved or resized."),
            c.locked,
            true,
            Act::Locked,
        );
    });
    b.section("Collapsing", |b| {
        b.switch(
            "Stack panels",
            Some("When a panel collapses, the panels against its far edge move in to fill the space, and move back when it expands."),
            c.stacking,
            true,
            Act::Stacking,
        );
    });
    b.section("Startup", |b| {
        b.switch("Start with Windows", None, c.start_with_windows, true, Act::StartWithWindows);
    });
    b.y += 18.0;
    (b.items, b.y + scroll)
}

// ---------------------------------------------------------------- drawing

fn accent_fill(c: &mut Canvas, r: Rect, radius: f32) {
    c.fill_gradient(
        r,
        radius,
        (r.x, r.y),
        (r.right(), r.bottom()),
        &[(0.0, Rgba::ACCENT), (1.0, ACCENT_2)],
    );
}

fn draw_arrow(c: &mut Canvas, r: Rect, angle: f32, color: Rgba) {
    let (cx, cy) = r.center();
    let k = 14.0 / 16.0;
    let pt = |x: f32, y: f32| (cx + (x - 8.0) * k, cy + (y - 8.0) * k);
    c.rotate(angle, (cx, cy));
    c.line(pt(8.0, 13.0), pt(8.0, 3.0), color, 1.8 * k);
    c.line(pt(4.0, 7.0), pt(8.0, 3.0), color, 1.8 * k);
    c.line(pt(8.0, 3.0), pt(12.0, 7.0), color, 1.8 * k);
    c.reset();
}

/// The little picture on a shape or style tile.
fn draw_art(c: &mut Canvas, t: &Theme, r: Rect, art: Art) {
    let card = Rect::new(r.x + r.w * 0.18, r.y, r.w * 0.64, r.h);
    match art {
        Art::Shape(shape) => {
            let body = match shape {
                HeaderShape::Folder => {
                    let s = card.h;
                    Rect::new(card.x + (card.w - s) / 2.0, card.y, s, s)
                }
                _ => card,
            };
            c.fill_round(body, 4.0, t.art);
            c.fill_round(Rect::new(body.x, body.y, body.w, 6.0), 3.0, t.art_bar);
            let dot = |c: &mut Canvas, x: f32, y: f32, s: f32| {
                c.fill_round(Rect::new(x, y, s, s), 1.5, t.art_bar)
            };
            match shape {
                HeaderShape::Phone => {
                    for n in 0..3 {
                        dot(c, body.x + 4.0 + n as f32 * 9.0, body.y + 10.0, 6.0);
                    }
                }
                HeaderShape::Folder => {
                    for n in 0..4 {
                        let (cx, cy) = ((n % 2) as f32, (n / 2) as f32);
                        dot(c, body.x + 5.0 + cx * 9.0, body.y + 10.0 + cy * 8.0, 6.0);
                    }
                }
                HeaderShape::Default => {}
            }
        }
        Art::Style(style) => {
            c.fill_round(card, 4.0, t.art);
            let bar = Rect::new(card.x, card.y, card.w, 7.0);
            match style {
                TitleBarStyle::Standard => c.fill_round(bar, 3.0, t.art_bar),
                TitleBarStyle::Pill => c.fill_round(
                    Rect::new(card.x + 2.0, card.y + 2.0, card.w - 4.0, 6.0),
                    3.0,
                    t.art_bar,
                ),
                TitleBarStyle::Minimal => c.fill_round(
                    Rect::new(card.x + card.w * 0.3, card.y + 3.0, card.w * 0.4, 2.5),
                    1.0,
                    t.art_bar,
                ),
                TitleBarStyle::Soft => {
                    let clear = t.art_bar.with_alpha(0.0);
                    c.fill_gradient(
                        Rect::new(card.x, card.y, card.w, 10.0),
                        3.0,
                        (card.x, card.y),
                        (card.x, card.y + 10.0),
                        &[(0.0, t.art_bar), (0.5, t.art_bar), (1.0, clear)],
                    )
                }
            }
        }
    }
}

/// The preview at the top of a panel's settings: a small panel in its colors.
fn draw_preview(c: &mut Canvas, t: &Theme, r: Rect, p: &PanelConfig) {
    c.clip(r);
    // A checkerboard, so transparency shows.
    c.fill_rect(r, Rgba::hex("#888888", 1.0));
    let mut y = r.y;
    let mut row = 0;
    while y < r.bottom() {
        let mut x = r.x + if row % 2 == 0 { 0.0 } else { 8.0 };
        while x < r.right() {
            c.fill_rect(Rect::new(x, y, 8.0, 8.0), Rgba::hex("#aaaaaa", 1.0));
            x += 16.0;
        }
        y += 8.0;
        row += 1;
    }
    c.fill_rect(r, Rgba::hex(&p.bg_color, p.bg_opacity as f32));
    let side = p.collapse_direction;
    let depth = 26.0;
    let mut bar = match side {
        CollapseDirection::Up => Rect::new(r.x, r.y, r.w, depth),
        CollapseDirection::Down => Rect::new(r.x, r.bottom() - depth, r.w, depth),
        CollapseDirection::Left => Rect::new(r.x, r.y, depth, r.h),
        CollapseDirection::Right => Rect::new(r.right() - depth, r.y, depth, r.h),
    };
    let fill = Rgba::hex(&p.title_bar_color, p.title_bar_opacity as f32);
    match p.title_bar_style {
        TitleBarStyle::Standard => c.fill_rect(bar, fill),
        TitleBarStyle::Pill => {
            bar = bar.inset(3.0);
            c.fill_round(bar, bar.w.min(bar.h) / 2.0, fill);
            c.stroke_round(bar, bar.w.min(bar.h) / 2.0, Rgba::white(0.2), 1.0);
        }
        TitleBarStyle::Minimal => {}
        TitleBarStyle::Soft => {
            let (from, to) = match side {
                CollapseDirection::Up => ((bar.x, bar.y), (bar.x, bar.bottom() + 4.0)),
                CollapseDirection::Down => ((bar.x, bar.bottom()), (bar.x, bar.y - 4.0)),
                CollapseDirection::Left => ((bar.x, bar.y), (bar.right() + 4.0, bar.y)),
                CollapseDirection::Right => ((bar.right(), bar.y), (bar.x - 4.0, bar.y)),
            };
            let clear = fill.with_alpha(0.0);
            c.fill_gradient(
                bar,
                0.0,
                from,
                to,
                &[(0.0, clear), (0.15, fill), (0.7, fill), (1.0, clear)],
            );
        }
    }
    let title = if p.title.is_empty() { "Untitled" } else { &p.title };
    let text = Rgba::hex(&p.title_text_color, 1.0);
    if side.is_sideways() {
        let (cx, cy) = bar.center();
        let turned = Rect::new(cx - bar.h / 2.0, cy - bar.w / 2.0, bar.h, bar.w);
        c.rotate(if side == CollapseDirection::Left { -90.0 } else { 90.0 }, (cx, cy));
        c.text(title, Font::Title, turned, text);
        c.reset();
    } else {
        c.text(title, Font::Title, bar, text);
    }
    if p.header_shape != HeaderShape::Default {
        let content = match side {
            CollapseDirection::Up => Rect::new(r.x, r.y + depth, r.w, r.h - depth),
            CollapseDirection::Down => Rect::new(r.x, r.y, r.w, r.h - depth),
            CollapseDirection::Left => Rect::new(r.x + depth, r.y, r.w - depth, r.h),
            CollapseDirection::Right => Rect::new(r.x, r.y, r.w - depth, r.h),
        };
        let (s, gap) =
            if p.header_shape == HeaderShape::Folder { (11.0, 3.0) } else { (18.0, 5.0) };
        for n in 0..4 {
            let (dx, dy) = if p.header_shape == HeaderShape::Folder {
                ((n % 2) as f32 * (s + gap), (n / 2) as f32 * (s + gap))
            } else if side.is_sideways() {
                (0.0, n as f32 * (s + gap))
            } else {
                (n as f32 * (s + gap), 0.0)
            };
            let d = Rect::new(content.x + 8.0 + dx, content.y + 6.0 + dy, s, s);
            c.fill_round(d, if s > 12.0 { 5.0 } else { 3.0 }, Rgba::white(0.35));
        }
    }
    c.unclip();
    c.stroke_round(r, 0.0, t.surface_border, 1.0);
}

#[allow(clippy::too_many_arguments)]
fn draw_page(
    c: &mut Canvas,
    t: &Theme,
    items: &[Ctl],
    hover: Option<usize>,
    panel: Option<&PanelConfig>,
    w: f32,
    h: f32,
    content_h: f32,
    scroll: f32,
) {
    let all = Rect::new(0.0, 0.0, w, h);
    c.fill_gradient(all, 0.0, (0.0, 0.0), (w * 0.6, h), &[(0.0, t.bg_top), (1.0, t.bg_bottom)]);
    // The soft tints of the old window background.
    c.fill_gradient(
        all,
        0.0,
        (0.0, 0.0),
        (w * 0.5, h * 0.35),
        &[(0.0, Rgba::ACCENT.with_alpha(0.16)), (1.0, Rgba::ACCENT.with_alpha(0.0))],
    );
    c.fill_gradient(
        all,
        0.0,
        (w, h),
        (w * 0.5, h * 0.65),
        &[(0.0, ACCENT_2.with_alpha(0.14)), (1.0, ACCENT_2.with_alpha(0.0))],
    );
    for (n, ctl) in items.iter().enumerate() {
        let r = ctl.r;
        if r.bottom() < 0.0 || r.y > h {
            continue;
        }
        let hovered = hover == Some(n);
        match &ctl.look {
            Look::Text { text, font, muted } => {
                c.text(text, *font, r, if *muted { t.muted } else { t.text })
            }
            Look::Card => {
                c.fill_round(r, 14.0, t.surface);
                c.stroke_round(r, 14.0, t.surface_border, 1.0);
            }
            Look::Line => c.fill_rect(r, t.line),
            Look::Switch { on, enabled } => {
                let a = if *enabled { 1.0 } else { 0.4 };
                if *on {
                    accent_fill(c, r, r.h / 2.0);
                } else {
                    c.fill_round(r, r.h / 2.0, t.track.with_alpha(t.track.3 * a));
                }
                let x = if *on { r.x + 19.0 } else { r.x + 3.0 };
                c.fill_ellipse((x + 8.0, r.y + 12.0), 8.0, 8.0, Rgba::black(0.15 * a));
                c.fill_ellipse(
                    (x + 8.0, r.y + 11.0),
                    8.0,
                    8.0,
                    Rgba::white(if *enabled { 1.0 } else { 0.7 }),
                );
            }
            Look::SegGroup => c.fill_round(r, 10.0, t.field),
            Look::Segment { label, selected } => {
                if *selected {
                    accent_fill(c, r, 8.0);
                } else if hovered {
                    c.fill_round(r, 8.0, t.field);
                }
                let color = if *selected {
                    Rgba::white(1.0)
                } else if hovered {
                    t.text
                } else {
                    t.muted
                };
                match label {
                    SegLabel::Text(s) => c.text(
                        s,
                        if *selected { Font::ButtonStrong } else { Font::Button },
                        r,
                        color,
                    ),
                    SegLabel::Arrow(a) => draw_arrow(c, r, *a, color),
                }
            }
            Look::Tile { art, label, selected } => {
                if *selected {
                    c.fill_round(
                        r,
                        11.0,
                        Rgba::ACCENT.with_alpha(if t.dark { 0.18 } else { 0.12 }),
                    );
                    c.stroke_round(r, 11.0, Rgba::ACCENT, 1.0);
                } else if hovered {
                    c.fill_round(r, 11.0, t.field);
                }
                draw_art(c, t, Rect::new(r.x + 6.0, r.y + 7.0, r.w - 12.0, 30.0), *art);
                let font = if *selected { Font::ButtonStrong } else { Font::SmallCenter };
                let color = if *selected || hovered { t.text } else { t.muted };
                c.text(label, font, Rect::new(r.x, r.y + 40.0, r.w, 18.0), color);
            }
            Look::Slider { value } => {
                let track = Rect::new(r.x, r.y + 8.0, r.w, 6.0);
                c.fill_round(track, 3.0, t.track);
                let fill_w = 8.0 + (r.w - 16.0) * value;
                accent_fill(c, Rect::new(r.x, r.y + 8.0, fill_w, 6.0), 3.0);
                let (kx, ky) = (r.x + fill_w, r.y + 11.0);
                if hovered {
                    c.fill_ellipse((kx, ky), 12.0, 12.0, Rgba::ACCENT.with_alpha(0.2));
                }
                c.fill_ellipse((kx, ky + 1.0), 8.0, 8.0, Rgba::black(0.2));
                c.fill_ellipse((kx, ky), 8.0, 8.0, Rgba::white(1.0));
            }
            Look::Badge(s) => {
                c.fill_round(r, r.h / 2.0, t.field);
                c.text(s, Font::SmallCenter, r, t.text);
            }
            Look::Swatch(hex) => {
                let (cx, cy) = r.center();
                let ring = if hovered { Rgba::ACCENT } else { t.line.with_alpha(0.25) };
                c.fill_ellipse((cx, cy), 15.0, 15.0, ring);
                c.fill_ellipse(
                    (cx, cy),
                    13.5,
                    13.5,
                    if t.dark { Rgba::hex("#1a1d2a", 1.0) } else { Rgba::hex("#f4f6fc", 1.0) },
                );
                c.fill_ellipse((cx, cy), 12.0, 12.0, Rgba::hex(hex, 1.0));
                c.stroke_ellipse((cx, cy), 12.0, 12.0, Rgba::black(0.12), 1.0);
            }
            Look::Preset { bg, bar, text, active } => {
                let (cx, cy) = r.center();
                if *active || hovered {
                    c.fill_ellipse(
                        (cx, cy),
                        17.5,
                        17.5,
                        Rgba::ACCENT.with_alpha(if *active { 1.0 } else { 0.4 }),
                    );
                    c.fill_ellipse(
                        (cx, cy),
                        16.0,
                        16.0,
                        if t.dark { Rgba::hex("#1a1d2a", 1.0) } else { Rgba::hex("#f4f6fc", 1.0) },
                    );
                }
                c.fill_ellipse((cx, cy), 14.0, 14.0, Rgba::hex(bg, 1.0));
                // The bar color as the top half.
                c.clip(Rect::new(r.x, r.y, r.w, r.h / 2.0));
                c.fill_ellipse((cx, cy), 14.0, 14.0, Rgba::hex(bar, 1.0));
                c.unclip();
                c.stroke_ellipse((cx, cy), 14.0, 14.0, Rgba::black(0.12), 1.0);
                if *active {
                    let k = Rgba::hex(text, 1.0);
                    c.line((cx - 5.0, cy + 0.5), (cx - 1.5, cy + 4.0), k, 2.0);
                    c.line((cx - 1.5, cy + 4.0), (cx + 5.0, cy - 3.5), k, 2.0);
                }
            }
            Look::Button { label, danger } => {
                c.fill_round(r, 10.0, if hovered { t.btn_hover } else { t.btn });
                c.stroke_round(r, 10.0, t.line.with_alpha(t.line.3 * 2.0), 1.0);
                let color = if *danger { DANGER } else { t.text };
                c.text(label, if *danger { Font::ButtonStrong } else { Font::Button }, r, color);
            }
            Look::Dropdown(label) => {
                c.fill_round(r, 10.0, if hovered { t.field_hover } else { t.field });
                c.text(label, Font::Button, Rect::new(r.x + 11.0, r.y, r.w - 34.0, r.h), t.text);
                let (ax, ay) = (r.right() - 16.0, r.y + r.h / 2.0);
                c.line((ax - 4.0, ay - 2.0), (ax, ay + 2.0), t.muted, 1.6);
                c.line((ax, ay + 2.0), (ax + 4.0, ay - 2.0), t.muted, 1.6);
            }
            Look::Field => c.fill_round(r, 10.0, t.field),
            Look::Path(p) => {
                c.fill_round(r, 8.0, t.field);
                c.text(p, Font::Button, Rect::new(r.x + 8.0, r.y, r.w - 16.0, r.h), t.text);
            }
            Look::Preview => {
                if let Some(p) = panel {
                    draw_preview(c, t, r, p);
                }
            }
        }
    }
    // A slim scrollbar when the page is taller than the window.
    if content_h > h {
        let th = (h * h / content_h).max(40.0);
        let y = (h - th) * (scroll / (content_h - h));
        c.fill_round(Rect::new(w - 9.0, y + 3.0, 6.0, th - 6.0), 3.0, Rgba(0.5, 0.5, 0.5, 0.35));
    }
}

// ----------------------------------------------------------------- window

pub struct SettingsWin {
    pub hwnd: HWND,
    /// The panel these settings are for; None for the app settings window.
    pub panel: Option<String>,
    scroll: f32,
    hover: Option<usize>,
    /// A slider being dragged.
    sliding: Option<Act>,
    title_edit: Option<HWND>,
    font: HFONT,
    field_brush: HBRUSH,
    pub theme: Theme,
    scale: f32,
}

impl Drop for SettingsWin {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.font.0));
            let _ = DeleteObject(HGDIOBJ(self.field_brush.0));
        }
    }
}

fn set_dark_title_bar(hwnd: HWND, dark: bool) {
    let v: i32 = dark as i32;
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &v as *const _ as *const c_void,
            4,
        )
    };
}

fn make_font(scale: f32) -> HFONT {
    unsafe {
        CreateFontW(
            -((12.0 * scale).round() as i32),
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            FF_DONTCARE.0 as u32,
            w!("Segoe UI"),
        )
    }
}

pub fn register_class() {
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(settings_proc),
        hInstance: hinstance(),
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
        hIcon: unsafe { LoadIconW(Some(hinstance()), PCWSTR(1 as _)) }.unwrap_or_default(),
        lpszClassName: SETTINGS_CLASS,
        ..Default::default()
    };
    unsafe { RegisterClassExW(&class) };
}

impl App {
    fn settings_index(&self, hwnd: HWND) -> Option<usize> {
        self.settings.iter().position(|s| s.hwnd == hwnd)
    }

    /// Opens (or brings forward) the settings window for panel `i`, or the
    /// app settings window when `i` is None.
    pub fn open_settings_window(&mut self, i: Option<usize>) {
        let panel = i.map(|i| self.panels[i].id.clone());
        if let Some(s) = self.settings.iter().find(|s| s.panel == panel) {
            unsafe {
                let _ = ShowWindow(s.hwnd, SW_SHOWNORMAL);
                let _ = SetForegroundWindow(s.hwnd);
            }
            return;
        }
        // Next to the panel (or the primary monitor's corner), whole window on screen.
        let (anchor, mon) = match i {
            Some(i) => {
                let r = self.panels[i].rect;
                (
                    r,
                    dpi::monitor_for_point(
                        windows::Win32::Foundation::POINT { x: r.left, y: r.top },
                        &self.mons,
                    ),
                )
            }
            None => {
                let m = self.mons[0];
                let w = m.work;
                let r = RECT {
                    left: w.left + 80,
                    top: w.top + 60,
                    right: w.left + 80,
                    bottom: w.top + 60,
                };
                (r, m)
            }
        };
        let scale = mon.scale;
        let content_h = if i.is_some() { 520.0 } else { 560.0 };
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
        let mut outer = RECT {
            left: 0,
            top: 0,
            right: (WIDTH * scale) as i32,
            bottom: (content_h * scale) as i32,
        };
        unsafe {
            let _ =
                AdjustWindowRectExForDpi(&mut outer, style, false, WINDOW_EX_STYLE(0), mon.dpi());
        }
        let (ow, oh) = (outer.right - outer.left, outer.bottom - outer.top);
        let area = mon.work;
        let x = if i.is_some() { anchor.right + (12.0 * scale) as i32 } else { anchor.left };
        let x = x.min(area.right - ow).max(area.left);
        let y = anchor.top.min(area.bottom - oh).max(area.top);
        let title = match &panel {
            Some(id) => {
                format!("Panel settings: {}", self.store.panel(id).map_or("", |p| p.title.as_str()))
            }
            None => "Desktop Icons Sorter".into(),
        };
        let t = Wide::new(&title);
        let Ok(hwnd) = (unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                SETTINGS_CLASS,
                t.pcwstr(),
                style,
                x,
                y,
                ow,
                oh,
                None,
                None,
                Some(hinstance()),
                None,
            )
        }) else {
            return;
        };
        let theme = Theme::current();
        set_dark_title_bar(hwnd, theme.dark);
        let font = make_font(scale);
        let title_edit = panel.as_ref().and_then(|id| {
            let text = Wide::new(&self.store.panel(id)?.title);
            let style = WS_CHILD | WS_VISIBLE | WINDOW_STYLE(0x0080 /* ES_AUTOHSCROLL */);
            let e = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("EDIT"),
                    text.pcwstr(),
                    style,
                    0,
                    0,
                    10,
                    10,
                    Some(hwnd),
                    Some(HMENU(TITLE_EDIT_ID as *mut _)),
                    Some(hinstance()),
                    None,
                )
            }
            .ok()?;
            unsafe {
                SendMessageW(e, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
                SendMessageW(e, EM_SETLIMITTEXT, Some(WPARAM(200)), None);
            }
            Some(e)
        });
        let field_brush = unsafe { CreateSolidBrush(COLORREF(theme.field_solid)) };
        self.settings.push(SettingsWin {
            hwnd,
            panel,
            scroll: 0.0,
            hover: None,
            sliding: None,
            title_edit,
            font,
            field_brush,
            theme,
            scale,
        });
        self.place_title_edit(self.settings.len() - 1);
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
            let _ = SetForegroundWindow(hwnd);
        }
    }

    /// Redraws every open settings window (the config changed).
    pub fn refresh_settings(&mut self) {
        for n in 0..self.settings.len() {
            let s = &self.settings[n];
            unsafe {
                let _ = InvalidateRect(Some(s.hwnd), None, false);
            }
            // The window title follows the panel's.
            if let Some(p) = s.panel.as_ref().and_then(|id| self.store.panel(id)) {
                let t = Wide::new(&format!("Panel settings: {}", p.title));
                unsafe {
                    let _ = SetWindowTextW(s.hwnd, t.pcwstr());
                }
            }
            self.place_title_edit(n);
        }
    }

    /// The Windows theme changed: recolor the settings windows.
    pub fn theme_changed(&mut self) {
        crate::win::darkmode::refresh();
        let theme = Theme::current();
        for s in &mut self.settings {
            s.theme = theme;
            set_dark_title_bar(s.hwnd, theme.dark);
            unsafe {
                let _ = DeleteObject(HGDIOBJ(s.field_brush.0));
                s.field_brush = CreateSolidBrush(COLORREF(theme.field_solid));
                let _ = InvalidateRect(Some(s.hwnd), None, true);
            }
        }
    }

    pub fn close_settings_for(&mut self, panel: Option<&str>) {
        if let Some(n) = self.settings.iter().position(|s| s.panel.as_deref() == panel) {
            let s = self.settings.remove(n);
            unsafe {
                let _ = DestroyWindow(s.hwnd);
            }
        }
    }

    fn page(&self, n: usize) -> (Vec<Ctl>, f32) {
        let s = &self.settings[n];
        let c = self.store.config();
        match &s.panel {
            Some(id) => match c.panel(id) {
                Some(p) => panel_page(&self.gfx, p, c, s.scroll),
                None => (Vec::new(), 0.0),
            },
            None => app_page(&self.gfx, c, s.scroll),
        }
    }

    fn client_size(&self, n: usize) -> (f32, f32) {
        let s = &self.settings[n];
        let mut r = RECT::default();
        unsafe {
            let _ = GetClientRect(s.hwnd, &mut r);
        }
        (r.right as f32 / s.scale, r.bottom as f32 / s.scale)
    }

    /// Keeps the native title box over its field as the page scrolls.
    fn place_title_edit(&mut self, n: usize) {
        let Some(edit) = self.settings[n].title_edit else { return };
        let (items, _) = self.page(n);
        let s = &self.settings[n];
        if let Some(f) = items.iter().find(|c| matches!(c.look, Look::Field)) {
            let k = s.scale;
            let line = 18.0;
            let r = Rect::new(f.r.x + 11.0, f.r.y + (f.r.h - line) / 2.0, f.r.w - 22.0, line);
            unsafe {
                let _ = MoveWindow(
                    edit,
                    (r.x * k).round() as i32,
                    (r.y * k).round() as i32,
                    (r.w * k).round() as i32,
                    (r.h * k).round() as i32,
                    true,
                );
            }
        }
    }

    fn paint_settings(&mut self, n: usize, hdc: HDC) {
        let (w, h) = self.client_size(n);
        let (items, content_h) = self.page(n);
        let s = &self.settings[n];
        let theme = s.theme;
        let (hover, scroll, scale) = (s.hover, s.scroll, s.scale);
        let panel = s.panel.as_ref().and_then(|id| self.store.panel(id)).cloned();
        let (pw, ph) = ((w * scale).round() as i32, (h * scale).round() as i32);
        let r = self.gfx.paint_window(hdc, pw, ph, scale, |c| {
            draw_page(c, &theme, &items, hover, panel.as_ref(), w, h, content_h, scroll)
        });
        if let Err(e) = r {
            eprintln!("[settings] {e}");
        }
    }

    fn hit_settings(&self, n: usize, l: LPARAM) -> Option<(usize, Act, Rect)> {
        let (x, y) = point_of(l);
        let s = &self.settings[n];
        let (x, y) = (x as f32 / s.scale, y as f32 / s.scale);
        let (items, _) = self.page(n);
        items
            .iter()
            .enumerate()
            .rev()
            .find(|(_, c)| c.act != Act::None && c.r.contains(x, y))
            .map(|(i, c)| (i, c.act, c.r))
    }

    fn settings_panel(&self, n: usize) -> Option<usize> {
        self.settings[n].panel.as_ref().and_then(|id| self.panel_by_id(id))
    }

    fn set_opacity(&mut self, n: usize, layer: Layer, r: Rect, l: LPARAM) {
        let Some(i) = self.settings_panel(n) else { return };
        let x = point_of(l).0 as f32 / self.settings[n].scale;
        let v = (((x - r.x - 8.0) / (r.w - 16.0)).clamp(0.0, 1.0) * 100.0).round() as f64 / 100.0;
        self.update_panel(i, |p| match layer {
            Layer::Background => p.bg_opacity = v,
            _ => p.title_bar_opacity = v,
        });
    }

    fn act(&mut self, n: usize, act: Act, r: Rect, l: LPARAM) {
        let hwnd = self.settings[n].hwnd;
        let panel = self.settings[n].panel.clone();
        let i = self.settings_panel(n);
        match (act, i) {
            (Act::Shape(v), Some(i)) => self.update_panel(i, |p| p.header_shape = v),
            (Act::Style(v), Some(i)) => self.update_panel(i, |p| p.title_bar_style = v),
            (Act::Size(v), Some(i)) => self.update_panel(i, |p| p.icon_size = v),
            (Act::Direction(v), Some(i)) => self.update_panel(i, |p| p.collapse_direction = v),
            (Act::Align(v), Some(i)) => self.update_panel(i, |p| p.collapse_align = v),
            (Act::Collapsed, Some(i)) => self.toggle_collapsed(i),
            (Act::Stackable, Some(i)) => self.update_panel(i, |p| p.stackable = !p.stackable),
            (Act::Catch, Some(i)) => {
                let on = !self.config_of(i).is_some_and(|p| p.catch_new_items);
                self.set_catching(i, on);
            }
            (Act::Opacity(layer), Some(_)) => {
                self.settings[n].sliding = Some(act);
                unsafe { SetCapture(hwnd) };
                self.set_opacity(n, layer, r, l);
            }
            (Act::Color(layer), Some(_)) => self.defer(Deferred::ChooseColor {
                owner: hwnd,
                panel: panel.unwrap_or_default(),
                layer,
            }),
            (Act::Preset(k), Some(i)) => {
                let (_, bg, bar, text) = PRESETS[k];
                self.update_panel(i, |p| {
                    p.bg_color = bg.into();
                    p.title_bar_color = bar.into();
                    p.title_text_color = text.into();
                });
            }
            (Act::SortMenu, Some(_)) => {
                self.defer(Deferred::SortMenu { owner: hwnd, panel: panel.unwrap_or_default() })
            }
            (Act::ChangeFolder, Some(_)) => {
                self.defer(Deferred::ChooseFolder { owner: hwnd, panel: panel.unwrap_or_default() })
            }
            (Act::Delete, Some(_)) => self.defer(Deferred::DeletePanel(panel.unwrap_or_default())),
            (Act::AddMode(v), _) => self.update_app(|c| c.add_mode = v),
            (Act::Snapping, _) => self.update_app(|c| c.snapping = !c.snapping),
            (Act::Locked, _) => {
                let locked = !self.store.config().locked;
                self.set_locked(locked);
            }
            (Act::Stacking, _) => self.update_app(|c| c.stacking = !c.stacking),
            (Act::StartWithWindows, _) => {
                let on = !self.store.config().start_with_windows;
                self.set_start_with_windows(on);
            }
            _ => {}
        }
    }
}

/// The sort order menu under a settings window's "Sort by" control.
pub fn sort_menu(owner: HWND, panel: &str) {
    use crate::win::menu::{self, Item};
    let Some(current) = with_app(|a| a.store.panel(panel).map(|p| p.sort_by)).flatten() else {
        return;
    };
    let sorts = [
        (SortBy::Custom, "Custom"),
        (SortBy::Name, "Name"),
        (SortBy::Type, "Type"),
        (SortBy::Modified, "Date modified"),
    ];
    let items: Vec<Item> =
        sorts.iter().enumerate().map(|(n, (v, l))| Item::check(n + 1, l, *v == current)).collect();
    let cmd = menu::show(&items, owner, None);
    if (1..=sorts.len()).contains(&cmd) {
        let v = sorts[cmd - 1].0;
        with_app(|a| {
            if let Some(i) = a.panel_by_id(panel) {
                a.update_panel(i, |p| p.sort_by = v);
            }
        });
    }
}

extern "system" fn settings_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let handled = with_app(|a| {
        let n = a.settings_index(hwnd)?;
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
                a.paint_settings(n, hdc);
                unsafe {
                    let _ = EndPaint(hwnd, &ps);
                }
                Some(LRESULT(0))
            }
            WM_ERASEBKGND => Some(LRESULT(1)),
            WM_LBUTTONDOWN => {
                if let Some((_, act, r)) = a.hit_settings(n, l) {
                    a.act(n, act, r, l);
                }
                Some(LRESULT(0))
            }
            WM_MOUSEMOVE => {
                if let Some(Act::Opacity(layer)) = a.settings[n].sliding {
                    let (items, _) = a.page(n);
                    if let Some(c) = items.iter().find(|c| c.act == Act::Opacity(layer)) {
                        let r = c.r;
                        a.set_opacity(n, layer, r, l);
                    }
                    return Some(LRESULT(0));
                }
                let hover = a.hit_settings(n, l).map(|h| h.0);
                if hover != a.settings[n].hover {
                    a.settings[n].hover = hover;
                    let mut t = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    unsafe {
                        let _ = TrackMouseEvent(&mut t);
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                }
                Some(LRESULT(0))
            }
            WM_SETCURSOR if (l.0 & 0xffff) as u32 == HTCLIENT => {
                let mut p = windows::Win32::Foundation::POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut p);
                    let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut p);
                }
                let lp = LPARAM(((p.y as u16 as isize) << 16) | (p.x as u16 as isize));
                let id = if a.hit_settings(n, lp).is_some() { IDC_HAND } else { IDC_ARROW };
                unsafe { SetCursor(LoadCursorW(None, id).ok()) };
                Some(LRESULT(1))
            }
            WM_MOUSELEAVE => {
                a.settings[n].hover = None;
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                Some(LRESULT(0))
            }
            WM_LBUTTONUP => {
                if a.settings[n].sliding.take().is_some() {
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                }
                Some(LRESULT(0))
            }
            WM_MOUSEWHEEL => {
                let (_, h) = a.client_size(n);
                let (_, content) = a.page(n);
                let s = &mut a.settings[n];
                let next = (s.scroll - wheel_delta(w) as f32 / 120.0 * 48.0)
                    .clamp(0.0, (content - h).max(0.0));
                if next != s.scroll {
                    s.scroll = next;
                    a.place_title_edit(n);
                    unsafe {
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                }
                Some(LRESULT(0))
            }
            WM_COMMAND if (w.0 & 0xffff) == TITLE_EDIT_ID && ((w.0 >> 16) as u32) == EN_CHANGE => {
                let edit = a.settings[n].title_edit?;
                let text = unsafe {
                    let len = GetWindowTextLengthW(edit) as usize;
                    let mut buf = vec![0u16; len + 1];
                    let got = GetWindowTextW(edit, &mut buf) as usize;
                    String::from_utf16_lossy(&buf[..got])
                };
                if let Some(i) = a.settings_panel(n)
                    && a.config_of(i).is_some_and(|p| p.title != text)
                {
                    a.update_panel(i, |p| p.title = text);
                }
                Some(LRESULT(0))
            }
            WM_CTLCOLOREDIT => {
                let s = &a.settings[n];
                let hdc = HDC(w.0 as *mut _);
                let text = if s.theme.dark { 0x00f4eeec } else { 0x00231c1a };
                unsafe {
                    SetTextColor(hdc, COLORREF(text));
                    SetBkColor(hdc, COLORREF(s.theme.field_solid));
                }
                Some(LRESULT(s.field_brush.0 as isize))
            }
            WM_DPICHANGED => {
                let s = &mut a.settings[n];
                s.scale = unsafe { GetDpiForWindow(hwnd) } as f32 / 96.0;
                unsafe {
                    let _ = DeleteObject(HGDIOBJ(s.font.0));
                }
                s.font = make_font(s.scale);
                if let Some(e) = s.title_edit {
                    unsafe {
                        SendMessageW(
                            e,
                            WM_SETFONT,
                            Some(WPARAM(s.font.0 as usize)),
                            Some(LPARAM(1)),
                        )
                    };
                }
                let r = unsafe { &*(l.0 as *const RECT) };
                unsafe {
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                a.place_title_edit(n);
                Some(LRESULT(0))
            }
            WM_CLOSE => {
                let s = a.settings.remove(n);
                unsafe {
                    let _ = DestroyWindow(s.hwnd);
                }
                Some(LRESULT(0))
            }
            _ => None,
        }
    })
    .flatten();
    run_deferred();
    handled.unwrap_or_else(|| unsafe { DefWindowProcW(hwnd, msg, w, l) })
}
