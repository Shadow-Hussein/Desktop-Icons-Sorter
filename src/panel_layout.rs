//! Where everything in a panel window goes, in DIPs: the title bar and its
//! buttons, the icon grid and the apps a collapsed dock or tile shows. The
//! renderer draws from this and mouse input is hit tested against it, so both
//! always agree. Mirrors the old stylesheet's sizes.

use crate::model::{
    CollapseDirection, HeaderShape, PanelConfig, ResizeEdge, TITLE_BAR_HEIGHT, TitleBarStyle,
    icon_pixels,
};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn inset(&self, d: f32) -> Self {
        Self::new(self.x + d, self.y + d, (self.w - 2.0 * d).max(0.0), (self.h - 2.0 * d).max(0.0))
    }

    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

/// Grid padding and gap, as the old `.grid` had them.
pub const GRID_PAD: f32 = 8.0;
pub const GRID_GAP: f32 = 4.0;
/// Title bar buttons (chevron, gear).
pub const BUTTON: f32 = 22.0;
/// Icons a collapsed dock or tile shows.
pub const PEEK_APP: f32 = 36.0;
pub const PEEK_ICON: f32 = 32.0;
const PEEK_GAP: f32 = 4.0;
/// How far a pill bar is inset from the panel's edges.
const PILL_INSET: f32 = 3.0;
/// Resize handles: strips along each edge and squares at the corners.
const EDGE_GRIP: f32 = 5.0;
const CORNER_GRIP: f32 = 10.0;
/// A dock row shows enough to fill a wide panel; a tile shows a 2 x 2 grid.
pub const DOCK_APPS: usize = 16;
pub const TILE_APPS: usize = 4;

/// The icon grid's measurements for one panel width and icon size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    /// The scrollable area the grid sits in.
    pub area: Rect,
    pub cols: usize,
    /// Each column's width (the columns share the width between them).
    pub cell_w: f32,
    /// Rows have a fixed height so the empty ones the user leaves keep their space.
    pub row_h: f32,
    /// An icon's tile: narrower than the column, centered in it.
    pub tile_w: f32,
    pub icon: f32,
}

impl Grid {
    pub fn new(area: Rect, icon: f32) -> Self {
        let tile_w = icon + 44.0;
        let inner = (area.w - 2.0 * GRID_PAD).max(0.0);
        let cols = (((inner + GRID_GAP) / (tile_w + GRID_GAP)).floor() as usize).max(1);
        let cell_w = (inner - (cols - 1) as f32 * GRID_GAP) / cols as f32;
        Self { area, cols, cell_w, row_h: icon + 46.0, tile_w, icon }
    }

    /// A cell's tile, `scroll` DIPs down the grid.
    pub fn tile(&self, cell: usize, scroll: f32) -> Rect {
        let (col, row) = ((cell % self.cols) as f32, (cell / self.cols) as f32);
        let x = self.area.x + GRID_PAD + col * (self.cell_w + GRID_GAP);
        let y = self.area.y + GRID_PAD + row * (self.row_h + GRID_GAP) - scroll;
        Rect::new(x + (self.cell_w - self.tile_w) / 2.0, y, self.tile_w, self.row_h)
    }

    /// The cell under a point, anywhere in the grid, empty or not.
    pub fn cell_at(&self, x: f32, y: f32, scroll: f32) -> usize {
        let x = x - self.area.x - GRID_PAD;
        let y = y - self.area.y - GRID_PAD + scroll;
        let col = ((x / (self.cell_w + GRID_GAP)).floor().max(0.0) as usize).min(self.cols - 1);
        let row = (y / (self.row_h + GRID_GAP)).floor().max(0.0) as usize;
        row * self.cols + col
    }

    /// How tall the grid's content is with `cells` cells in use.
    pub fn content_height(&self, cells: usize) -> f32 {
        let rows = cells.div_ceil(self.cols) as f32;
        2.0 * GRID_PAD + rows * self.row_h + (rows - 1.0).max(0.0) * GRID_GAP
    }

    pub fn max_scroll(&self, cells: usize) -> f32 {
        (self.content_height(cells) - self.area.h).max(0.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// The whole window: the panel's background and border.
    pub panel: Rect,
    pub side: CollapseDirection,
    /// The title bar's fill (inset for a pill).
    pub bar: Rect,
    pub chevron: Rect,
    pub gear: Rect,
    /// Where the title text is centered; it's clipped to this.
    pub title: Rect,
    /// Expanded: the icon grid. Collapsed: None.
    pub grid: Option<Grid>,
    /// The apps a collapsed dock or tile shows.
    pub peek: Vec<Rect>,
    /// The area the peek apps sit in, for "Drag icons here" when empty.
    pub peek_area: Rect,
}

impl Layout {
    pub fn vertical_bar(&self) -> bool {
        self.side.is_sideways()
    }
}

/// Lays out a panel window `w` x `h` DIPs, as it looks now (expanded or
/// collapsed). `items` is how many apps a collapsed panel has to show.
pub fn layout(p: &PanelConfig, w: f32, h: f32, items: usize) -> Layout {
    let panel = Rect::new(0.0, 0.0, w, h);
    let side = p.collapse_direction;
    let bar_depth = TITLE_BAR_HEIGHT as f32;
    let full_bar = match side {
        CollapseDirection::Up => Rect::new(0.0, 0.0, w, bar_depth),
        CollapseDirection::Down => Rect::new(0.0, h - bar_depth, w, bar_depth),
        CollapseDirection::Left => Rect::new(0.0, 0.0, bar_depth, h),
        CollapseDirection::Right => Rect::new(w - bar_depth, 0.0, bar_depth, h),
    };
    let pill = p.title_bar_style == TitleBarStyle::Pill;
    let bar = if pill { full_bar.inset(PILL_INSET) } else { full_bar };
    // Buttons sit 4 px in from the bar's ends (3 px inside a pill), centered across it.
    let pad = if pill { PILL_INSET + 3.0 } else { 4.0 };
    let (chevron, gear, title) = if side.is_sideways() {
        let x = full_bar.x + (bar_depth - BUTTON) / 2.0;
        let chevron = Rect::new(x, pad, BUTTON, BUTTON);
        let gear = Rect::new(x, h - pad - BUTTON, BUTTON, BUTTON);
        let top = chevron.bottom() + 4.0;
        (chevron, gear, Rect::new(full_bar.x, top, bar_depth, (gear.y - 4.0 - top).max(0.0)))
    } else {
        let y = full_bar.y + (bar_depth - BUTTON) / 2.0;
        let chevron = Rect::new(pad, y, BUTTON, BUTTON);
        let gear = Rect::new(w - pad - BUTTON, y, BUTTON, BUTTON);
        let left = chevron.right() + 4.0;
        (chevron, gear, Rect::new(left, full_bar.y, (gear.x - 4.0 - left).max(0.0), bar_depth))
    };
    let content = match side {
        CollapseDirection::Up => Rect::new(0.0, bar_depth, w, (h - bar_depth).max(0.0)),
        CollapseDirection::Down => Rect::new(0.0, 0.0, w, (h - bar_depth).max(0.0)),
        CollapseDirection::Left => Rect::new(bar_depth, 0.0, (w - bar_depth).max(0.0), h),
        CollapseDirection::Right => Rect::new(0.0, 0.0, (w - bar_depth).max(0.0), h),
    };
    let mut out = Layout {
        panel,
        side,
        bar,
        chevron,
        gear,
        title,
        grid: None,
        peek: Vec::new(),
        peek_area: content,
    };
    if !p.collapsed {
        out.grid = Some(Grid::new(content, icon_pixels(p.icon_size) as f32));
        return out;
    }
    match p.header_shape {
        HeaderShape::Default => {}
        HeaderShape::Phone => {
            // A row of apps along the bar (a column for side bars); the rest
            // wrap out of sight.
            let (along, across) = if side.is_sideways() {
                (content.h - 16.0, content.w)
            } else {
                (content.w - 16.0, content.h)
            };
            let fit = ((along + PEEK_GAP) / (PEEK_APP + PEEK_GAP)).floor().max(0.0) as usize;
            let off = (across - PEEK_APP) / 2.0;
            for i in 0..items.min(DOCK_APPS).min(fit) {
                let at = 8.0 + i as f32 * (PEEK_APP + PEEK_GAP);
                out.peek.push(if side.is_sideways() {
                    Rect::new(content.x + off, content.y + at, PEEK_APP, PEEK_APP)
                } else {
                    Rect::new(content.x + at, content.y + off, PEEK_APP, PEEK_APP)
                });
            }
        }
        HeaderShape::Folder => {
            // A 2 x 2 grid, centered.
            let span = 2.0 * PEEK_APP + PEEK_GAP;
            let (x0, y0) =
                (content.x + (content.w - span) / 2.0, content.y + (content.h - span) / 2.0);
            for i in 0..items.min(TILE_APPS) {
                let (c, r) = ((i % 2) as f32, (i / 2) as f32);
                let step = PEEK_APP + PEEK_GAP;
                out.peek.push(Rect::new(x0 + c * step, y0 + r * step, PEEK_APP, PEEK_APP));
            }
        }
    }
    out
}

/// What's under the mouse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Resize(ResizeEdge),
    Chevron,
    Gear,
    TitleText,
    /// The bar outside its buttons and text: drag to move, double-click to collapse.
    TitleBar,
    /// A grid cell (an item or a gap), in reading order.
    Cell(usize),
    /// One of a collapsed dock's or tile's apps.
    Peek(usize),
    /// The rest of the panel.
    Body,
}

/// Which edges can resize the panel now. A collapsed panel only resizes along
/// its bar, and a collapsed tile has a fixed size.
pub fn resize_edges(p: &PanelConfig, locked: bool) -> &'static [ResizeEdge] {
    const ALL: [ResizeEdge; 8] = [
        ResizeEdge::N,
        ResizeEdge::S,
        ResizeEdge::E,
        ResizeEdge::W,
        ResizeEdge::NE,
        ResizeEdge::NW,
        ResizeEdge::SE,
        ResizeEdge::SW,
    ];
    if locked {
        &[]
    } else if !p.collapsed {
        &ALL
    } else if p.header_shape == HeaderShape::Folder {
        &[]
    } else if p.collapse_direction.is_sideways() {
        &[ResizeEdge::N, ResizeEdge::S]
    } else {
        &[ResizeEdge::E, ResizeEdge::W]
    }
}

fn grip(l: &Layout, x: f32, y: f32, inset: f32) -> Option<ResizeEdge> {
    let p = l.panel;
    let (n, s) = (y < p.y + EDGE_GRIP, y >= p.bottom() - EDGE_GRIP);
    let (w, e) = (x < p.x + EDGE_GRIP, x >= p.right() - EDGE_GRIP);
    let (cn, cs) = (y < p.y + CORNER_GRIP, y >= p.bottom() - CORNER_GRIP);
    let (cw, ce) = (x < p.x + CORNER_GRIP, x >= p.right() - CORNER_GRIP);
    if (cn && cw) || (n && cw) || (w && cn) {
        return Some(ResizeEdge::NW);
    }
    if (cn && ce) || (n && ce) || (e && cn) {
        return Some(ResizeEdge::NE);
    }
    if (cs && cw) || (s && cw) || (w && cs) {
        return Some(ResizeEdge::SW);
    }
    if (cs && ce) || (s && ce) || (e && cs) {
        return Some(ResizeEdge::SE);
    }
    // Edge strips stop short of the corners (`inset`), as the old handles did.
    let along_x = x >= p.x + inset && x < p.right() - inset;
    let along_y = y >= p.y + inset && y < p.bottom() - inset;
    match () {
        _ if n && along_x => Some(ResizeEdge::N),
        _ if s && along_x => Some(ResizeEdge::S),
        _ if w && along_y => Some(ResizeEdge::W),
        _ if e && along_y => Some(ResizeEdge::E),
        _ => None,
    }
}

/// What's at (x, y). `title_len` is how long the title text is along the bar,
/// so only the text itself counts as the title (the rest of the bar moves
/// and collapses the panel).
pub fn hit_test(l: &Layout, p: &PanelConfig, locked: bool, x: f32, y: f32, title_len: f32) -> Hit {
    let edges = resize_edges(p, locked);
    if !edges.is_empty() {
        let inset = if p.collapsed { 4.0 } else { 8.0 };
        if let Some(e) = grip(l, x, y, inset).filter(|e| edges.contains(e)) {
            return Hit::Resize(e);
        }
    }
    if l.chevron.contains(x, y) {
        return Hit::Chevron;
    }
    if l.gear.contains(x, y) {
        return Hit::Gear;
    }
    let full_bar = match l.side {
        CollapseDirection::Up => Rect::new(0.0, 0.0, l.panel.w, TITLE_BAR_HEIGHT as f32),
        CollapseDirection::Down => {
            Rect::new(0.0, l.panel.h - TITLE_BAR_HEIGHT as f32, l.panel.w, TITLE_BAR_HEIGHT as f32)
        }
        CollapseDirection::Left => Rect::new(0.0, 0.0, TITLE_BAR_HEIGHT as f32, l.panel.h),
        CollapseDirection::Right => {
            Rect::new(l.panel.w - TITLE_BAR_HEIGHT as f32, 0.0, TITLE_BAR_HEIGHT as f32, l.panel.h)
        }
    };
    if full_bar.contains(x, y) {
        let (c, at) = if l.vertical_bar() {
            (l.title.y + l.title.h / 2.0, y)
        } else {
            (l.title.x + l.title.w / 2.0, x)
        };
        let half = title_len.min(if l.vertical_bar() { l.title.h } else { l.title.w }) / 2.0;
        return if (at - c).abs() <= half { Hit::TitleText } else { Hit::TitleBar };
    }
    if let Some(i) = l.peek.iter().position(|r| r.contains(x, y)) {
        return Hit::Peek(i);
    }
    if let Some(g) = &l.grid
        && g.area.contains(x, y)
    {
        return Hit::Cell(g.cell_at(x, y, 0.0));
    }
    Hit::Body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::create_panel;
    use crate::model::{Bounds, IconSize};

    fn panel() -> PanelConfig {
        create_panel("A", Bounds::new(0, 0, 360, 240), "/a")
    }

    #[test]
    fn a_top_bar_has_the_chevron_left_and_the_gear_right() {
        let l = layout(&panel(), 360.0, 240.0, 0);
        assert_eq!(l.bar, Rect::new(0.0, 0.0, 360.0, 30.0));
        assert_eq!(l.chevron, Rect::new(4.0, 4.0, 22.0, 22.0));
        assert_eq!(l.gear, Rect::new(334.0, 4.0, 22.0, 22.0));
        assert_eq!(l.grid.unwrap().area, Rect::new(0.0, 30.0, 360.0, 210.0));
    }

    #[test]
    fn the_bar_sits_on_the_collapse_edge() {
        let mut p = panel();
        p.collapse_direction = CollapseDirection::Down;
        let l = layout(&p, 360.0, 240.0, 0);
        assert_eq!(l.bar, Rect::new(0.0, 210.0, 360.0, 30.0));
        assert_eq!(l.grid.unwrap().area, Rect::new(0.0, 0.0, 360.0, 210.0));
        p.collapse_direction = CollapseDirection::Right;
        let l = layout(&p, 360.0, 240.0, 0);
        assert_eq!(l.bar, Rect::new(330.0, 0.0, 30.0, 240.0));
        assert_eq!(l.chevron, Rect::new(334.0, 4.0, 22.0, 22.0));
        assert_eq!(l.gear, Rect::new(334.0, 214.0, 22.0, 22.0));
        assert_eq!(l.grid.unwrap().area, Rect::new(0.0, 0.0, 330.0, 240.0));
    }

    #[test]
    fn a_pill_bar_is_inset() {
        let mut p = panel();
        p.title_bar_style = TitleBarStyle::Pill;
        let l = layout(&p, 360.0, 240.0, 0);
        assert_eq!(l.bar, Rect::new(3.0, 3.0, 354.0, 24.0));
    }

    #[test]
    fn the_grid_fits_as_many_columns_as_it_can() {
        // Medium icons: 92 px tiles; 360 - 16 = 344 → 3 columns of 112.
        let g = Grid::new(Rect::new(0.0, 30.0, 360.0, 210.0), 48.0);
        assert_eq!(g.cols, 3);
        assert_eq!(g.cell_w, (344.0 - 8.0) / 3.0);
        assert_eq!(g.row_h, 94.0);
        let t = g.tile(4, 0.0);
        assert_eq!(t.y, 30.0 + 8.0 + 98.0);
        assert_eq!(g.cell_at(t.x + 1.0, t.y + 1.0, 0.0), 4);
        assert_eq!(g.cell_at(-50.0, -50.0, 0.0), 0);
        assert_eq!(Grid::new(Rect::new(0.0, 0.0, 50.0, 50.0), 64.0).cols, 1);
    }

    #[test]
    fn the_grid_scrolls_only_past_its_area() {
        let g = Grid::new(Rect::new(0.0, 30.0, 360.0, 210.0), 48.0);
        assert_eq!(g.max_scroll(6), 0.0);
        assert_eq!(g.content_height(7), 16.0 + 3.0 * 94.0 + 8.0);
        assert_eq!(g.max_scroll(7), 16.0 + 3.0 * 94.0 + 8.0 - 210.0);
    }

    #[test]
    fn a_collapsed_dock_shows_a_row_of_apps() {
        let mut p = panel();
        p.collapsed = true;
        p.header_shape = HeaderShape::Phone;
        p.icon_size = IconSize::Large;
        let l = layout(&p, 360.0, 78.0, 20);
        assert!(l.grid.is_none());
        // (360 - 16 + 4) / 40 = 8 fit.
        assert_eq!(l.peek.len(), 8);
        assert_eq!(l.peek[0], Rect::new(8.0, 36.0, 36.0, 36.0));
        assert_eq!(layout(&p, 360.0, 78.0, 3).peek.len(), 3);
    }

    #[test]
    fn a_collapsed_tile_shows_four_apps_in_a_square() {
        let mut p = panel();
        p.collapsed = true;
        p.header_shape = HeaderShape::Folder;
        let l = layout(&p, 122.0, 122.0, 9);
        assert_eq!(l.peek.len(), 4);
        // 92 px under the bar, 76 px of apps: 8 px in from each side.
        assert_eq!(l.peek[0], Rect::new(23.0, 30.0 + 8.0, 36.0, 36.0));
        assert_eq!(l.peek[3].x, 23.0 + 40.0);
    }

    #[test]
    fn hit_test_finds_buttons_text_bar_cells_and_edges() {
        let p = panel();
        let l = layout(&p, 360.0, 240.0, 0);
        let hit = |x, y| hit_test(&l, &p, false, x, y, 40.0);
        assert_eq!(hit(10.0, 10.0), Hit::Chevron);
        assert_eq!(hit(340.0, 10.0), Hit::Gear);
        assert_eq!(hit(180.0, 15.0), Hit::TitleText);
        assert_eq!(hit(250.0, 15.0), Hit::TitleBar);
        assert_eq!(hit(20.0, 60.0), Hit::Cell(0));
        assert_eq!(hit(180.0, 1.0), Hit::Resize(ResizeEdge::N));
        assert_eq!(hit(1.0, 1.0), Hit::Resize(ResizeEdge::NW));
        assert_eq!(hit(359.0, 239.0), Hit::Resize(ResizeEdge::SE));
        assert_eq!(hit(1.0, 120.0), Hit::Resize(ResizeEdge::W));
        assert_eq!(hit_test(&l, &p, true, 180.0, 1.0, 40.0), Hit::TitleText);
    }

    #[test]
    fn a_collapsed_panel_resizes_only_along_its_bar() {
        let mut p = panel();
        p.collapsed = true;
        assert_eq!(resize_edges(&p, false), [ResizeEdge::E, ResizeEdge::W]);
        p.collapse_direction = CollapseDirection::Left;
        assert_eq!(resize_edges(&p, false), [ResizeEdge::N, ResizeEdge::S]);
        p.header_shape = HeaderShape::Folder;
        assert!(resize_edges(&p, false).is_empty());
        let l = layout(&p, 122.0, 122.0, 0);
        assert_eq!(hit_test(&l, &p, false, 1.0, 60.0, 0.0), Hit::TitleBar);
    }
}
