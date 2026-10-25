//! One panel window: drawing it, and turning mouse and keyboard input into
//! actions on the app.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use sorter::folder::PanelItem;
use sorter::geometry::{
    SNAP_DISTANCE, SnapLines, expanded_from_collapsed, resize_bounds, snap_move, snap_resize,
    visual_bounds,
};
use sorter::grid::layout_grid;
use sorter::model::{
    Bounds, COLLAPSE_ANIMATION_MS, CollapseDirection, HeaderShape, PanelConfig, ResizeEdge, SortBy,
    TitleBarStyle, collapsed_length, icon_pixels,
};
use sorter::panel_layout::{Grid, Hit, Layout, PEEK_ICON, Rect, hit_test, layout};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use crate::win::icons::{IconCache, Lookup};
use crate::win::render::{Canvas, Font, Rgba};
use crate::win::watch::DirWatch;

/// How far the mouse must move with the button down before a press becomes a drag.
pub const DRAG_THRESHOLD: i32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerKind {
    Move,
    Resize(ResizeEdge),
}

/// A move or resize in progress.
pub struct Pointer {
    pub kind: PointerKind,
    pub start_cursor: POINT,
    /// The window's bounds (DIPs) at the start.
    pub start_bounds: Bounds,
    /// The bounds last asked for, saved at the end.
    pub last: Bounds,
    /// What the panel covers at the start, which is what snaps.
    pub start_visual: Bounds,
    /// Other panels' edges, or None when snapping is off.
    pub lines: Option<SnapLines>,
    pub scale: f32,
}

/// The left button is down on something; what it becomes depends on what happens next.
#[derive(Clone, Debug, PartialEq)]
pub enum Press {
    /// The title bar: a drag past the threshold moves the panel.
    Title(POINT),
    /// A title bar button, clicked when released over it.
    Button(Hit),
    /// An item in the grid: a drag past the threshold drags it out.
    Item(POINT, String),
    /// One of a collapsed dock's or tile's apps: opens on release, or drags out.
    Peek(POINT, String),
}

/// Files being dragged over a panel.
pub struct DropState {
    pub paths: Vec<PathBuf>,
    /// The grid cell a drop would land in, when it can be placed.
    pub slot: Option<usize>,
    /// This panel's own item being arranged (its file name).
    pub own: Option<String>,
}

/// A window sliding or resizing over the collapse animation's length.
pub struct Anim {
    pub from: Bounds,
    pub to: Bounds,
    pub start: u64,
    /// Draw the expanded look while it runs (the panel itself collapsing or expanding).
    pub expanded_look: bool,
}

impl Anim {
    fn progress(&self, now: u64) -> f32 {
        ((now.saturating_sub(self.start)) as f32 / COLLAPSE_ANIMATION_MS as f32).min(1.0)
    }

    pub fn done(&self, now: u64) -> bool {
        self.progress(now) >= 1.0
    }

    /// Where the window is at `now`, eased out like the old CSS transition.
    pub fn at(&self, now: u64) -> Bounds {
        let k = 1.0 - (1.0 - self.progress(now)).powi(4);
        let lerp = |a: i32, b: i32| (a as f32 + (b - a) as f32 * k).round() as i32;
        Bounds {
            x: lerp(self.from.x, self.to.x),
            y: lerp(self.from.y, self.to.y),
            width: lerp(self.from.width, self.to.width),
            height: lerp(self.from.height, self.to.height),
        }
    }
}

pub struct Panel {
    pub id: String,
    pub hwnd: HWND,
    pub items: Vec<PanelItem>,
    pub scroll: f32,
    /// Selected items' file names, lower-cased.
    pub selected: HashSet<String>,
    /// The item the keyboard is on (lower-cased name).
    pub focus: Option<String>,
    /// Where a Shift+click or Shift+arrow selection starts (lower-cased name).
    pub anchor: Option<String>,
    pub hover: Option<Hit>,
    /// The mouse is over the panel (shows the gear and a minimal bar's chevron).
    pub inside: bool,
    pub pointer: Option<Pointer>,
    pub press: Option<Press>,
    pub watch: Option<DirWatch>,
    /// Where the window is now, in physical pixels, and the scale it was drawn at.
    pub rect: RECT,
    pub scale: f32,
    /// How long the title is, for hit testing the title text.
    pub title_len: f32,
    /// A message to load missing icons is on its way.
    pub icons_queued: bool,
    pub drop: Option<DropState>,
    pub anim: Option<Anim>,
}

impl Panel {
    pub fn new(id: String, hwnd: HWND) -> Self {
        Self {
            id,
            hwnd,
            items: Vec::new(),
            scroll: 0.0,
            selected: HashSet::new(),
            focus: None,
            anchor: None,
            hover: None,
            inside: false,
            pointer: None,
            press: None,
            watch: None,
            rect: RECT::default(),
            scale: 1.0,
            title_len: 0.0,
            icons_queued: false,
            drop: None,
            anim: None,
        }
    }

    pub fn is_selected(&self, item: &PanelItem) -> bool {
        self.selected.contains(&item.file_name.to_lowercase())
    }
}

/// Window bounds in DIPs: what the panel covers, collapsed or not.
pub fn window_bounds(p: &PanelConfig) -> Bounds {
    visual_bounds(p)
}

/// Each item's grid cell, by file name.
pub fn cells_of(p: &PanelConfig, items: &[PanelItem], cols: usize) -> HashMap<String, usize> {
    let names: Vec<String> = items.iter().map(|i| i.file_name.clone()).collect();
    let saved = if p.sort_by == SortBy::Custom { p.cells.as_slice() } else { &[] };
    layout_grid(&names, saved, cols)
}

pub fn item_at_cell<'a>(
    p: &PanelConfig,
    items: &'a [PanelItem],
    cols: usize,
    cell: usize,
) -> Option<&'a PanelItem> {
    let cells = cells_of(p, items, cols);
    items.iter().find(|i| cells.get(&i.file_name) == Some(&cell))
}

/// The layout for the panel window as it is now.
pub fn current_layout(p: &PanelConfig, panel: &Panel) -> Layout {
    let (w, h) = (
        (panel.rect.right - panel.rect.left) as f32 / panel.scale,
        (panel.rect.bottom - panel.rect.top) as f32 / panel.scale,
    );
    layout(p, w, h, panel.items.len())
}

pub fn hit(p: &PanelConfig, panel: &Panel, locked: bool, x: f32, y: f32) -> Hit {
    let l = current_layout(p, panel);
    match hit_test(&l, p, locked, x, y, panel.title_len) {
        // The grid scrolls; hit testing works in unscrolled cells.
        Hit::Cell(_) => Hit::Cell(l.grid.expect("cells need a grid").cell_at(x, y, panel.scroll)),
        h => h,
    }
}

// ----------------------------------------------------------------- drawing

fn chevron_angle(p: &PanelConfig) -> f32 {
    let base = match p.collapse_direction {
        CollapseDirection::Up => 0.0,
        CollapseDirection::Down => 180.0,
        CollapseDirection::Left => -90.0,
        CollapseDirection::Right => 90.0,
    };
    // Points at the content when expanded, and a quarter turn back when collapsed.
    if p.collapsed { base - 90.0 } else { base }
}

fn draw_chevron(c: &mut Canvas, r: Rect, angle: f32, color: Rgba) {
    let (cx, cy) = r.center();
    // The 16-unit "M3 6l5 5 5-5" path at 12 px.
    let k = 12.0 / 16.0;
    let pt = |x: f32, y: f32| (cx + (x - 8.0) * k, cy + (y - 8.0) * k);
    c.rotate(angle, (cx, cy));
    c.line(pt(3.0, 6.0), pt(8.0, 11.0), color, 2.0 * k);
    c.line(pt(8.0, 11.0), pt(13.0, 6.0), color, 2.0 * k);
    c.reset();
}

fn draw_gear(c: &mut Canvas, r: Rect, color: Rgba) {
    let (cx, cy) = r.center();
    let k = 13.0 / 16.0;
    let pt = |x: f32, y: f32| (cx + (x - 8.0) * k, cy + (y - 8.0) * k);
    c.stroke_ellipse((cx, cy), 2.3 * k + 0.8 * k, 2.3 * k + 0.8 * k, color, 1.6 * k);
    for (a, b) in [
        ((8.0, 1.5), (8.0, 3.5)),
        ((8.0, 12.5), (8.0, 14.5)),
        ((1.5, 8.0), (3.5, 8.0)),
        ((12.5, 8.0), (14.5, 8.0)),
        ((3.4, 3.4), (4.8, 4.8)),
        ((11.2, 11.2), (12.6, 12.6)),
        ((3.4, 12.6), (4.8, 11.2)),
        ((11.2, 4.8), (12.6, 3.4)),
    ] {
        c.line(pt(a.0, a.1), pt(b.0, b.1), color, 1.6 * k);
    }
}

/// Draws text along the bar: across a horizontal bar, or rotated on a side bar
/// (a left bar reads bottom to top, like a book spine).
fn draw_bar_text(
    c: &mut Canvas,
    l: &Layout,
    text: &str,
    font: Font,
    r: Rect,
    color: Rgba,
    shadow: f32,
) {
    if !l.vertical_bar() {
        c.text_shadowed(text, font, r, color, shadow);
        return;
    }
    let (cx, cy) = r.center();
    let turned = Rect::new(cx - r.h / 2.0, cy - r.w / 2.0, r.h, r.w);
    let angle = if l.side == CollapseDirection::Left { -90.0 } else { 90.0 };
    c.rotate(angle, (cx, cy));
    c.text_shadowed(text, font, turned, color, shadow);
    c.reset();
}

/// The bar's fill with its edges fading out (the soft style): a little along
/// its length and at its outer edge, and into the panel past its inner edge.
fn draw_soft_bar(c: &mut Canvas, l: &Layout, fill: Rgba) {
    let b = l.bar;
    const BLEED: f32 = 6.0;
    // The fill reaches 6 px into the panel and fades out there.
    let r = match l.side {
        CollapseDirection::Up => Rect::new(b.x, b.y, b.w, b.h + BLEED),
        CollapseDirection::Down => Rect::new(b.x, b.y - BLEED, b.w, b.h + BLEED),
        CollapseDirection::Left => Rect::new(b.x, b.y, b.w + BLEED, b.h),
        CollapseDirection::Right => Rect::new(b.x - BLEED, b.y, b.w + BLEED, b.h),
    };
    let (outer, inner) = match l.side {
        CollapseDirection::Up => ((r.x, r.y), (r.x, r.bottom())),
        CollapseDirection::Down => ((r.x, r.bottom()), (r.x, r.y)),
        CollapseDirection::Left => ((r.x, r.y), (r.right(), r.y)),
        CollapseDirection::Right => ((r.right(), r.y), (r.x, r.y)),
    };
    let depth = if l.vertical_bar() { r.w } else { r.h };
    let length = if l.vertical_bar() { r.h } else { r.w };
    let clear = fill.with_alpha(0.0);
    let across = [(0.0, clear), (6.0 / depth, fill), (1.0 - 12.0 / depth, fill), (1.0, clear)];
    let (start, end) = if l.vertical_bar() {
        ((r.x, r.y), (r.x, r.bottom()))
    } else {
        ((r.x, r.y), (r.right(), r.y))
    };
    let along = [(0.0, 0.0), (14.0 / length, 1.0), (1.0 - 14.0 / length, 1.0), (1.0, 0.0)];
    c.fill_masked_gradient(r, outer, inner, &across, start, end, &along);
}

/// An item's tile: highlight, icon (or a placeholder while it loads), the
/// shortcut arrow and the name.
#[allow(clippy::too_many_arguments)]
fn draw_item(
    c: &mut Canvas,
    icons: &mut IconCache,
    missing: &mut bool,
    item: &PanelItem,
    tile: Rect,
    icon_dip: f32,
    text: Rgba,
    selected: bool,
    hovered: bool,
) {
    if selected {
        c.fill_round(tile, 6.0, Rgba(46.0 / 255.0, 156.0 / 255.0, 240.0 / 255.0, 0.3));
        c.stroke_round(tile, 6.0, Rgba(120.0 / 255.0, 190.0 / 255.0, 1.0, 0.65), 1.0);
    } else if hovered {
        c.fill_round(tile, 6.0, Rgba::white(0.1));
        c.stroke_round(tile, 6.0, Rgba::white(0.12), 1.0);
    }
    let icon = Rect::new(tile.x + (tile.w - icon_dip) / 2.0, tile.y + 7.0, icon_dip, icon_dip);
    let px = (icon_dip * c.scale).round() as i32;
    let drawn = draw_icon(c, icons, missing, item, icon, px);
    if drawn && item.is_shortcut {
        let gfx = c.gfx();
        // The arrow is drawn at the item's own size, like Explorer's overlay.
        let overlay: Option<ID2D1Bitmap> = icons.overlay(gfx, px).cloned();
        if let Some(o) = overlay {
            c.image(&o, icon);
        }
    }
    let label_h = if selected { 200.0 } else { 31.0 };
    let label = Rect::new(tile.x + 3.0, icon.bottom() + 4.0, tile.w - 6.0, label_h);
    c.text_shadowed(&item.display_name, Font::Label, label, text, 0.85);
}

/// Draws an item's icon into `r`; false (and a placeholder) when it isn't loaded yet.
fn draw_icon(
    c: &mut Canvas,
    icons: &mut IconCache,
    missing: &mut bool,
    item: &PanelItem,
    r: Rect,
    px: i32,
) -> bool {
    let bitmap = match icons.get(c.gfx(), &item.path, item.modified_ms, px) {
        Lookup::Ready(b) => b.cloned(),
        Lookup::Missing => {
            *missing = true;
            None
        }
    };
    match bitmap {
        Some(b) => {
            c.image(&b, r);
            true
        }
        None => {
            let p = Rect::new(r.x + r.w * 0.15, r.y + r.h * 0.15, r.w * 0.7, r.h * 0.7);
            c.fill_round(p, 6.0, Rgba::white(0.15));
            false
        }
    }
}

/// Everything a panel needs drawn, gathered so drawing borrows nothing else.
pub struct Scene<'a> {
    pub config: &'a PanelConfig,
    pub panel: &'a Panel,
    pub layout: &'a Layout,
    /// The drop target cell while something is dragged over the grid.
    pub drop_active: bool,
    /// Cells to draw items in (custom order may differ from list order).
    pub cells: &'a HashMap<String, usize>,
    /// Where a drop would land (the dashed slot), while dragging.
    pub slot: Option<usize>,
    /// Items being dragged out of this panel are left out of the grid.
    pub hidden_item: Option<&'a str>,
}

/// Draws the whole panel. Sets `missing` when an icon still has to be loaded.
pub fn draw(c: &mut Canvas, s: &Scene, icons: &mut IconCache, missing: &mut bool) {
    let p = s.config;
    let l = s.layout;
    let style = p.title_bar_style;
    let shape = p.header_shape;
    let rounded = if p.collapsed && shape != HeaderShape::Default { 14.0 } else { 8.0 };
    // A collapsed pill bar is only its capsule; the panel behind it stays clear.
    let clear = p.collapsed && style == TitleBarStyle::Pill && shape == HeaderShape::Default;

    // Background. A little alpha even at 0% opacity, or clicks would fall
    // through the transparent parts of the window to the desktop.
    if !clear {
        let bg = Rgba::hex(&p.bg_color, (p.bg_opacity as f32).max(2.0 / 255.0));
        c.fill_round(l.panel, rounded, bg);
        c.stroke_round(l.panel, rounded, Rgba::white(0.14), 1.0);
    }
    if s.drop_active {
        c.stroke_round(l.panel, rounded, Rgba::ACCENT, 2.0);
    }

    // Title bar.
    let fill = Rgba::hex(&p.title_bar_color, p.title_bar_opacity as f32);
    match style {
        TitleBarStyle::Standard => {
            c.clip(l.bar);
            c.fill_round(l.panel, rounded, fill);
            c.unclip();
            if !p.collapsed {
                let sep = Rgba::white(0.08);
                let b = l.bar;
                match l.side {
                    CollapseDirection::Up => {
                        c.line((b.x, b.bottom() - 0.5), (b.right(), b.bottom() - 0.5), sep, 1.0)
                    }
                    CollapseDirection::Down => {
                        c.line((b.x, b.y + 0.5), (b.right(), b.y + 0.5), sep, 1.0)
                    }
                    CollapseDirection::Left => {
                        c.line((b.right() - 0.5, b.y), (b.right() - 0.5, b.bottom()), sep, 1.0)
                    }
                    CollapseDirection::Right => {
                        c.line((b.x + 0.5, b.y), (b.x + 0.5, b.bottom()), sep, 1.0)
                    }
                }
            }
        }
        TitleBarStyle::Pill => {
            let radius = l.bar.w.min(l.bar.h) / 2.0;
            c.fill_round(l.bar, radius, fill);
            c.stroke_round(l.bar, radius, Rgba::white(0.14), 1.0);
        }
        TitleBarStyle::Minimal => {}
        TitleBarStyle::Soft => draw_soft_bar(c, l, fill),
    }

    let text = Rgba::hex(&p.title_text_color, 1.0);
    let pill = style == TitleBarStyle::Pill;
    let button = |c: &mut Canvas, r: Rect, hit: Hit| {
        if s.panel.hover == Some(hit) {
            c.fill_round(r, if pill { r.h / 2.0 } else { 6.0 }, Rgba::white(0.16));
        }
    };
    // A minimal bar's chevron shows only while pointing at the panel (or collapsed).
    let chevron_shown = style != TitleBarStyle::Minimal || p.collapsed || s.panel.inside;
    if chevron_shown {
        button(c, l.chevron, Hit::Chevron);
        let a = if s.panel.hover == Some(Hit::Chevron) { 1.0 } else { 0.75 };
        draw_chevron(c, l.chevron, chevron_angle(p), text.with_alpha(a));
    }
    if s.panel.inside {
        button(c, l.gear, Hit::Gear);
        let a = if s.panel.hover == Some(Hit::Gear) { 1.0 } else { 0.75 };
        draw_gear(c, l.gear, text.with_alpha(a));
    }
    let title = if p.title.is_empty() { "Untitled" } else { &p.title };
    draw_bar_text(c, l, title, Font::Title, l.title, text, 0.45);

    // Content.
    if let Some(g) = &l.grid {
        draw_grid(c, s, g, icons, missing, text);
    } else if shape != HeaderShape::Default {
        draw_peek(c, s, icons, missing, text);
    }
}

fn draw_grid(
    c: &mut Canvas,
    s: &Scene,
    g: &Grid,
    icons: &mut IconCache,
    missing: &mut bool,
    text: Rgba,
) {
    let p = s.config;
    let panel = s.panel;
    c.clip(g.area);
    if panel.items.is_empty() && s.slot.is_none() {
        let r = Rect::new(g.area.x + 8.0, g.area.y + 8.0 + 24.0, g.area.w - 16.0, 40.0);
        c.text_shadowed("Drag icons here", Font::Hint, r, text.with_alpha(0.6), 0.6);
    }
    if let Some(slot) = s.slot {
        let t = g.tile(slot, panel.scroll);
        let icon = Rect::new(t.x + (t.w - g.icon) / 2.0, t.y + 7.0, g.icon, g.icon);
        c.fill_round(icon, 12.0, Rgba(46.0 / 255.0, 156.0 / 255.0, 240.0 / 255.0, 0.2));
        c.stroke_round(icon, 12.0, Rgba::white(0.6), 2.0);
    }
    let icon_dip = icon_pixels(p.icon_size) as f32;
    let hovered_cell = match panel.hover {
        Some(Hit::Cell(cell)) => Some(cell),
        _ => None,
    };
    // Selected items go last: their full names may reach into the row below.
    let mut order: Vec<&PanelItem> = panel.items.iter().collect();
    order.sort_by_key(|i| panel.is_selected(i));
    for item in order {
        if s.hidden_item.is_some_and(|h| h.eq_ignore_ascii_case(&item.file_name)) {
            continue;
        }
        let Some(&cell) = s.cells.get(&item.file_name) else { continue };
        let tile = g.tile(cell, panel.scroll);
        if tile.bottom() < g.area.y || tile.y > g.area.bottom() {
            continue;
        }
        let selected = panel.is_selected(item);
        let hovered = hovered_cell == Some(cell) && panel.pointer.is_none();
        draw_item(c, icons, missing, item, tile, icon_dip, text, selected, hovered);
    }
    // A thin scrollbar when the grid is taller than the panel.
    let used = s.cells.values().max().map_or(0, |m| m + 1);
    let max = g.max_scroll(used);
    if max > 0.0 {
        let total = g.content_height(used);
        let h = (g.area.h * g.area.h / total).max(24.0);
        let y = g.area.y + (g.area.h - h) * (panel.scroll / max);
        c.fill_round(
            Rect::new(g.area.right() - 7.0, y + 2.0, 5.0, h - 4.0),
            2.5,
            Rgba::white(0.25),
        );
    }
    c.unclip();
}

fn draw_peek(c: &mut Canvas, s: &Scene, icons: &mut IconCache, missing: &mut bool, text: Rgba) {
    let l = s.layout;
    let panel = s.panel;
    if panel.items.is_empty() {
        let a = l.peek_area;
        if s.config.header_shape == HeaderShape::Folder || !l.vertical_bar() {
            let r = Rect::new(a.x + 4.0, a.y, a.w - 8.0, a.h);
            let (_, th) = c.gfx().measure("Drag icons here", Font::Hint, r.w);
            let r = Rect::new(r.x, a.y + (a.h - th) / 2.0, r.w, th + 2.0);
            c.text_shadowed("Drag icons here", Font::Hint, r, text.with_alpha(0.6), 0.6);
        } else {
            draw_bar_text(c, l, "Drag icons here", Font::Hint, a, text.with_alpha(0.6), 0.6);
        }
        return;
    }
    // Made at the size they show, so they're as sharp as the grid's.
    let px = (PEEK_ICON * c.scale).round() as i32;
    for (i, r) in l.peek.iter().enumerate() {
        let Some(item) = panel.items.get(i) else { break };
        if panel.hover == Some(Hit::Peek(i)) {
            c.fill_round(*r, 8.0, Rgba::white(0.14));
            c.stroke_round(*r, 8.0, Rgba::white(0.14), 1.0);
        }
        let inner = Rect::new(
            r.x + (r.w - PEEK_ICON) / 2.0,
            r.y + (r.h - PEEK_ICON) / 2.0,
            PEEK_ICON,
            PEEK_ICON,
        );
        draw_icon(c, icons, missing, item, inner, px);
    }
}

// --------------------------------------------------------- move and resize

/// Starts moving or resizing; None when the panel is locked or can't resize now.
pub fn start_pointer(
    p: &PanelConfig,
    others: &[PanelConfig],
    kind: PointerKind,
    snapping: bool,
    scale: f32,
    from: Option<POINT>,
) -> Option<Pointer> {
    // A collapsed tile has a fixed size.
    if matches!(kind, PointerKind::Resize(_))
        && p.collapsed
        && collapsed_length(p.header_shape).is_some()
    {
        return None;
    }
    // Measured from where the button went down, so the first few pixels
    // (before the press counted as a drag) aren't lost.
    let cursor = match from {
        Some(p) => p,
        None => {
            let mut c = POINT::default();
            unsafe { GetCursorPos(&mut c) }.ok()?;
            c
        }
    };
    let start = window_bounds(p);
    let lines =
        snapping.then(|| SnapLines::of(others.iter().filter(|o| o.id != p.id).map(visual_bounds)));
    Some(Pointer {
        kind,
        start_cursor: cursor,
        start_bounds: start,
        last: start,
        start_visual: visual_bounds(p),
        lines,
        scale,
    })
}

/// Where the window goes for the cursor at `cursor`. `work_area` is the work
/// area (DIPs) of the monitor under the cursor, whose edges snap too.
pub fn pointer_bounds(s: &Pointer, p: &PanelConfig, cursor: POINT, work_area: Bounds) -> Bounds {
    let dx = ((cursor.x - s.start_cursor.x) as f32 / s.scale).round() as i32;
    let dy = ((cursor.y - s.start_cursor.y) as f32 / s.scale).round() as i32;
    let lines = s.lines.as_ref().map(|l| {
        let mut l = l.clone();
        l.add([work_area]);
        l
    });
    match s.kind {
        PointerKind::Move => {
            // Snap what the panel covers, then move the window by as much.
            let v = s.start_visual;
            let moved = Bounds { x: v.x + dx, y: v.y + dy, ..v };
            let snapped = lines.as_ref().map_or(moved, |l| snap_move(moved, l, SNAP_DISTANCE));
            Bounds {
                x: s.start_bounds.x + dx + snapped.x - moved.x,
                y: s.start_bounds.y + dy + snapped.y - moved.y,
                ..s.start_bounds
            }
        }
        PointerKind::Resize(edge) => {
            // A collapsed panel only resizes along its title bar.
            let sideways = p.collapsed && p.collapse_direction.is_sideways();
            let flat = p.collapsed && !sideways;
            let mut next = resize_bounds(
                s.start_bounds,
                edge,
                if sideways { 0 } else { dx },
                if flat { 0 } else { dy },
            );
            if flat {
                next = Bounds { y: s.start_bounds.y, height: s.start_bounds.height, ..next };
            }
            if sideways {
                next = Bounds { x: s.start_bounds.x, width: s.start_bounds.width, ..next };
            }
            let edge = if flat {
                edge.horizontal_only()
            } else if sideways {
                edge.vertical_only()
            } else {
                edge
            };
            match &lines {
                Some(l) => snap_resize(next, edge, l, SNAP_DISTANCE),
                None => next,
            }
        }
    }
}

/// The panel's new expanded bounds once a move or resize ends at `b`.
pub fn end_bounds(s: &Pointer, p: &PanelConfig) -> Bounds {
    let b = s.last;
    if s.kind == PointerKind::Move {
        let (dx, dy) = (b.x - s.start_bounds.x, b.y - s.start_bounds.y);
        return Bounds { x: p.bounds.x + dx, y: p.bounds.y + dy, ..p.bounds };
    }
    if p.collapsed {
        expanded_from_collapsed(
            b,
            p.bounds,
            p.collapse_direction,
            collapsed_length(p.header_shape),
            p.collapse_align,
        )
    } else {
        b
    }
}
