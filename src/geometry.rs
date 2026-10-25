//! Panel geometry: resizing, collapsing, snapping and keeping panels on screen.

use crate::model::{
    Bounds, CollapseAlign, CollapseDirection, MIN_PANEL_HEIGHT, MIN_PANEL_WIDTH, PanelConfig,
    ResizeEdge, collapsed_bar_size, collapsed_length,
};

pub const SNAP_DISTANCE: i32 = 12;

/// New bounds after dragging `edge` by (dx, dy), respecting the minimum size.
pub fn resize_bounds(start: Bounds, edge: ResizeEdge, dx: i32, dy: i32) -> Bounds {
    let Bounds { mut x, mut y, mut width, mut height } = start;
    if edge.e {
        width = MIN_PANEL_WIDTH.max(start.width + dx);
    }
    if edge.s {
        height = MIN_PANEL_HEIGHT.max(start.height + dy);
    }
    if edge.w {
        width = MIN_PANEL_WIDTH.max(start.width - dx);
        x = start.x + start.width - width;
    }
    if edge.n {
        height = MIN_PANEL_HEIGHT.max(start.height - dy);
        y = start.y + start.height - height;
    }
    Bounds { x, y, width, height }
}

/// JS `Math.round` of `n / 2`: halves round up.
fn half(n: i32) -> i32 {
    (n as f64 / 2.0 + 0.5).floor() as i32
}

/// How far a part of length `part` sits from the start of `full` when aligned to `align`.
fn align_offset(full: i32, part: i32, align: CollapseAlign) -> i32 {
    match align {
        CollapseAlign::Start => 0,
        CollapseAlign::Center => half(full - part),
        CollapseAlign::End => full - part,
    }
}

/// The window of a collapsed panel: the strip along the bar's edge, `bar_size`
/// deep. `length` shortens it along the bar, as for a square tile, and `align`
/// places it at the start, middle or end; None keeps the full length.
pub fn collapsed_bounds(
    expanded: Bounds,
    direction: CollapseDirection,
    bar_size: i32,
    length: Option<i32>,
    align: CollapseAlign,
) -> Bounds {
    let Bounds { x, y, width, height } = expanded;
    let w = length.unwrap_or(width);
    let h = length.unwrap_or(height);
    let dx = align_offset(width, w, align);
    let dy = align_offset(height, h, align);
    match direction {
        CollapseDirection::Up => Bounds::new(x + dx, y, w, bar_size),
        CollapseDirection::Down => Bounds::new(x + dx, y + height - bar_size, w, bar_size),
        CollapseDirection::Left => Bounds::new(x, y + dy, bar_size, h),
        CollapseDirection::Right => Bounds::new(x + width - bar_size, y + dy, bar_size, h),
    }
}

/// Expanded bounds for a collapsed panel whose window was moved or resized to
/// `win`. Uses the window's edge on the bar's side. With a fixed `length` (a
/// square tile) the window's length along the bar says nothing about the
/// panel's, and `align` says where along the panel the tile sat.
pub fn expanded_from_collapsed(
    win: Bounds,
    expanded: Bounds,
    direction: CollapseDirection,
    length: Option<i32>,
    align: CollapseAlign,
) -> Bounds {
    let (width, height, x, y) = match length {
        None => (win.width, win.height, win.x, win.y),
        Some(len) => (
            expanded.width,
            expanded.height,
            win.x - align_offset(expanded.width, len, align),
            win.y - align_offset(expanded.height, len, align),
        ),
    };
    match direction {
        CollapseDirection::Up => Bounds { x, y: win.y, width, ..expanded },
        CollapseDirection::Down => {
            Bounds { x, y: win.y + win.height - expanded.height, width, ..expanded }
        }
        CollapseDirection::Left => Bounds { x: win.x, y, height, ..expanded },
        CollapseDirection::Right => {
            Bounds { x: win.x + win.width - expanded.width, y, height, ..expanded }
        }
    }
}

/// What a panel covers on screen: its expanded bounds, or the bar it collapsed into.
pub fn visual_bounds(p: &PanelConfig) -> Bounds {
    if !p.collapsed {
        return p.bounds;
    }
    collapsed_bounds(
        p.bounds,
        p.collapse_direction,
        collapsed_bar_size(p.header_shape),
        collapsed_length(p.header_shape),
        p.collapse_align,
    )
}

/// Vertical (x) and horizontal (y) lines a moving or resizing panel snaps to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SnapLines {
    pub x: Vec<i32>,
    pub y: Vec<i32>,
}

impl SnapLines {
    /// The edges of every rect, e.g. other panels and the work area.
    pub fn of(rects: impl IntoIterator<Item = Bounds>) -> Self {
        let mut lines = Self::default();
        lines.add(rects);
        lines
    }

    pub fn add(&mut self, rects: impl IntoIterator<Item = Bounds>) {
        for r in rects {
            self.x.extend([r.x, r.right()]);
            self.y.extend([r.y, r.bottom()]);
        }
    }
}

/// The smallest shift that puts one of `values` on one of `lines`, or 0 if none is close.
fn snap_shift(values: &[i32], lines: &[i32], distance: i32) -> i32 {
    let mut best = 0;
    let mut best_abs = distance + 1;
    for &v in values {
        for &line in lines {
            let d = line - v;
            if d.abs() < best_abs {
                best = d;
                best_abs = d.abs();
            }
        }
    }
    if best_abs <= distance { best } else { 0 }
}

/// Snaps a rect being moved so its nearest edge on each axis lands on a line
/// within `distance`. The rect comes from the cursor each time, so it sticks
/// to a line until the cursor has moved `distance` past it.
pub fn snap_move(b: Bounds, lines: &SnapLines, distance: i32) -> Bounds {
    Bounds {
        x: b.x + snap_shift(&[b.x, b.right()], &lines.x, distance),
        y: b.y + snap_shift(&[b.y, b.bottom()], &lines.y, distance),
        ..b
    }
}

/// Snaps the edges named by `edge` to lines, keeping the minimum size.
pub fn snap_resize(b: Bounds, edge: ResizeEdge, lines: &SnapLines, distance: i32) -> Bounds {
    let Bounds { mut x, mut y, mut width, mut height } = b;
    let (right, bottom) = (b.right(), b.bottom());
    if edge.e {
        let r = right + snap_shift(&[right], &lines.x, distance);
        if r - x >= MIN_PANEL_WIDTH {
            width = r - x;
        }
    } else if edge.w {
        let l = x + snap_shift(&[x], &lines.x, distance);
        if right - l >= MIN_PANEL_WIDTH {
            (x, width) = (l, right - l);
        }
    }
    if edge.s {
        let btm = bottom + snap_shift(&[bottom], &lines.y, distance);
        if btm - y >= MIN_PANEL_HEIGHT {
            height = btm - y;
        }
    } else if edge.n {
        let t = y + snap_shift(&[y], &lines.y, distance);
        if bottom - t >= MIN_PANEL_HEIGHT {
            (y, height) = (t, bottom - t);
        }
    }
    Bounds { x, y, width, height }
}

fn intersection_area(a: Bounds, b: Bounds) -> i64 {
    let w = a.right().min(b.right()) - a.x.max(b.x);
    let h = a.bottom().min(b.bottom()) - a.y.max(b.y);
    if w > 0 && h > 0 { w as i64 * h as i64 } else { 0 }
}

/// Makes sure a panel is reachable: if less than a title-bar-sized strip is on
/// any work area (a monitor was unplugged, say), moves it onto the work area
/// it overlaps most, or the nearest one, and shrinks it to fit if needed. The
/// strip runs along the edge the title bar sits on.
pub fn clamp_to_work_areas(
    b: Bounds,
    areas: &[Bounds],
    title_side: CollapseDirection,
    visible_strip: i32,
) -> Bounds {
    let center_distance = |a: &Bounds| {
        let dx = (a.x as f64 + a.width as f64 / 2.0) - (b.x as f64 + b.width as f64 / 2.0);
        let dy = (a.y as f64 + a.height as f64 / 2.0) - (b.y as f64 + b.height as f64 / 2.0);
        dx.hypot(dy)
    };
    let Some(best) = areas.iter().min_by(|a1, a2| {
        intersection_area(b, **a2)
            .cmp(&intersection_area(b, **a1))
            .then(center_distance(a1).total_cmp(&center_distance(a2)))
    }) else {
        return b;
    };
    let depth = if title_side.is_sideways() { b.width } else { b.height };
    let strip = collapsed_bounds(b, title_side, depth.min(30), None, CollapseAlign::Start);
    let reachable = areas.iter().any(|a| intersection_area(strip, *a) >= visible_strip as i64 * 20);
    if reachable {
        return b;
    }
    let width = b.width.min(best.width);
    let height = b.height.min(best.height);
    let x = b.x.max(best.x).min(best.right() - width);
    let y = b.y.max(best.y).min(best.bottom() - height);
    Bounds { x, y, width, height }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::create_panel;
    use crate::model::{FOLDER_CARD_SIZE, HeaderShape};
    use CollapseAlign::*;
    use CollapseDirection::*;

    const START: Bounds = Bounds::new(100, 100, 300, 200);

    fn snap(b: Bounds, lines: &SnapLines) -> Bounds {
        snap_move(b, lines, SNAP_DISTANCE)
    }

    fn lines(x: &[i32], y: &[i32]) -> SnapLines {
        SnapLines { x: x.to_vec(), y: y.to_vec() }
    }

    #[test]
    fn resize_grows_from_the_south_east_corner() {
        assert_eq!(resize_bounds(START, ResizeEdge::SE, 50, 20), Bounds::new(100, 100, 350, 220));
    }

    #[test]
    fn resize_keeps_the_opposite_edge_when_dragging_north_west() {
        assert_eq!(resize_bounds(START, ResizeEdge::NW, 40, 30), Bounds::new(140, 130, 260, 170));
    }

    #[test]
    fn resize_enforces_the_minimum_size() {
        let b = resize_bounds(START, ResizeEdge::W, 1000, 0);
        assert_eq!(b.width, MIN_PANEL_WIDTH);
        assert_eq!(b.right(), START.right());
        assert_eq!(resize_bounds(START, ResizeEdge::S, 0, -1000).height, MIN_PANEL_HEIGHT);
    }

    #[test]
    fn snap_move_snaps_near_screen_edges() {
        let area = SnapLines::of([Bounds::new(0, 0, 1920, 1040)]);
        let b = snap(Bounds { x: 8, y: 5, ..START }, &area);
        assert_eq!((b.x, b.y), (0, 0));
        assert_eq!(snap(Bounds { x: 1615, ..START }, &area).x, 1620);
        assert_eq!(snap(START, &area), START);
    }

    #[test]
    fn snap_move_lines_up_with_another_panel_and_sits_against_it() {
        let other = SnapLines::of([Bounds::new(500, 100, 300, 200)]);
        let b = snap(Bounds { x: 507, y: 600, ..START }, &other);
        assert_eq!((b.x, b.y), (500, 600));
        let b = snap(Bounds { x: 190, y: 290, ..START }, &other);
        assert_eq!((b.x, b.y), (200, 300));
    }

    #[test]
    fn snap_move_picks_the_closest_line() {
        assert_eq!(snap(START, &lines(&[95, 410], &[])).x, 95);
    }

    #[test]
    fn snap_move_lets_go_past_the_snap_distance() {
        let l = lines(&[500], &[]);
        assert_eq!(snap(Bounds { x: 512, ..START }, &l).x, 500);
        assert_eq!(snap(Bounds { x: 513, ..START }, &l).x, 513);
    }

    #[test]
    fn snap_resize_snaps_only_the_dragged_edges() {
        let l = lines(&[405, 95], &[310]);
        let r = |e| snap_resize(START, e, &l, SNAP_DISTANCE);
        assert_eq!(r(ResizeEdge::E), Bounds { width: 305, ..START });
        assert_eq!(r(ResizeEdge::W), Bounds { x: 95, width: 305, ..START });
        assert_eq!(r(ResizeEdge::SE), Bounds { width: 305, height: 210, ..START });
        assert_eq!(r(ResizeEdge::N), START);
    }

    #[test]
    fn snap_resize_keeps_the_minimum_size() {
        let small = Bounds::new(0, 0, MIN_PANEL_WIDTH + 5, 200);
        let l = lines(&[MIN_PANEL_WIDTH - 5], &[]);
        assert_eq!(snap_resize(small, ResizeEdge::E, &l, 12), small);
    }

    #[test]
    fn visual_bounds_is_the_bar_once_collapsed() {
        let mut p = create_panel("A", START, "/a");
        assert_eq!(visual_bounds(&p), START);
        p.collapsed = true;
        assert_eq!(visual_bounds(&p), Bounds { height: 30, ..START });
        p.collapse_direction = Down;
        assert_eq!(visual_bounds(&p), Bounds { y: 270, height: 30, ..START });
    }

    const AREAS: [Bounds; 2] = [Bounds::new(0, 0, 1920, 1040), Bounds::new(1920, 0, 1280, 1000)];

    fn clamp(b: Bounds, side: CollapseDirection) -> Bounds {
        clamp_to_work_areas(b, &AREAS, side, 40)
    }

    #[test]
    fn clamp_leaves_a_reachable_panel_alone() {
        let b = Bounds::new(2000, 50, 300, 200);
        assert_eq!(clamp(b, Up), b);
    }

    #[test]
    fn clamp_pulls_a_panel_back_from_an_unplugged_monitor() {
        assert_eq!(clamp(Bounds::new(4000, 50, 300, 200), Up), Bounds::new(2900, 50, 300, 200));
    }

    #[test]
    fn clamp_moves_a_panel_whose_title_bar_is_above_the_screen() {
        assert_eq!(clamp(Bounds::new(100, -500, 300, 200), Up).y, 0);
    }

    #[test]
    fn clamp_checks_the_strip_on_the_title_bar_side() {
        let b = Bounds::new(100, 900, 300, 400);
        assert_eq!(clamp(b, Up), b);
        assert_eq!(clamp(b, Down).y, 640);
        let b = Bounds::new(3000, 50, 400, 200);
        assert_eq!(clamp(b, Up), b);
        assert_eq!(clamp(b, Right).x, 2800);
    }

    const EXPANDED: Bounds = Bounds::new(10, 100, 300, 200);

    fn strip(dir: CollapseDirection, size: i32) -> Bounds {
        collapsed_bounds(EXPANDED, dir, size, None, Start)
    }

    #[test]
    fn collapsed_bounds_keeps_the_strip_on_the_bar_edge() {
        assert_eq!(strip(Up, 30), Bounds::new(10, 100, 300, 30));
        assert_eq!(strip(Down, 30), Bounds::new(10, 270, 300, 30));
        assert_eq!(strip(Left, 30), Bounds::new(10, 100, 30, 200));
        assert_eq!(strip(Right, 30), Bounds::new(280, 100, 30, 200));
    }

    #[test]
    fn collapsed_bounds_keeps_the_taller_phone_strip_on_the_bar_edge() {
        let size = collapsed_bar_size(HeaderShape::Phone);
        assert!(size > collapsed_bar_size(HeaderShape::Default));
        assert_eq!(strip(Down, size), Bounds::new(10, 300 - size, 300, size));
        assert_eq!(strip(Right, size), Bounds::new(310 - size, 100, size, 200));
    }

    #[test]
    fn a_tile_is_a_square_in_the_corner_at_the_start() {
        let size = collapsed_bar_size(HeaderShape::Folder);
        let len = collapsed_length(HeaderShape::Folder);
        assert_eq!(len, Some(size));
        let tile = |dir| collapsed_bounds(EXPANDED, dir, size, len, Start);
        assert_eq!(tile(Up), Bounds::new(10, 100, size, size));
        assert_eq!(tile(Down), Bounds::new(10, 300 - size, size, size));
        assert_eq!(tile(Left), Bounds::new(10, 100, size, size));
        assert_eq!(tile(Right), Bounds::new(310 - size, 100, size, size));
    }

    #[test]
    fn a_tile_can_sit_in_the_middle_or_at_the_end() {
        let size = FOLDER_CARD_SIZE;
        let tile = |dir, align| collapsed_bounds(EXPANDED, dir, size, Some(size), align);
        // 300 x 200 panel, 122 px square: 89 px in from the left when centered.
        let b = tile(Up, Center);
        assert_eq!((b.x, b.y), (10 + 89, 100));
        let b = tile(Down, End);
        assert_eq!((b.x, b.y), (310 - size, 300 - size));
        let b = tile(Left, Center);
        assert_eq!((b.x, b.y), (10, 100 + 39));
        let b = tile(Right, End);
        assert_eq!((b.x, b.y), (310 - size, 300 - size));
    }

    #[test]
    fn a_full_length_strip_ignores_the_position() {
        assert_eq!(collapsed_bounds(EXPANDED, Up, 30, None, End), strip(Up, 30));
    }

    #[test]
    fn expanded_from_collapsed_round_trips_an_unmoved_panel() {
        for dir in CollapseDirection::ALL {
            for size in [30, collapsed_bar_size(HeaderShape::Phone)] {
                let win = strip(dir, size);
                assert_eq!(expanded_from_collapsed(win, EXPANDED, dir, None, Start), EXPANDED);
            }
        }
    }

    #[test]
    fn expanded_from_collapsed_round_trips_an_unmoved_tile() {
        let size = FOLDER_CARD_SIZE;
        for dir in CollapseDirection::ALL {
            for align in [Start, Center, End] {
                let tile = collapsed_bounds(EXPANDED, dir, size, Some(size), align);
                assert_eq!(
                    expanded_from_collapsed(tile, EXPANDED, dir, Some(size), align),
                    EXPANDED
                );
            }
        }
    }

    #[test]
    fn a_moved_tile_keeps_the_expanded_size() {
        let win = Bounds::new(600, 400, 122, 122);
        let len = Some(FOLDER_CARD_SIZE);
        assert_eq!(
            expanded_from_collapsed(win, EXPANDED, Up, len, Start),
            Bounds::new(600, 400, 300, 200)
        );
        assert_eq!(
            expanded_from_collapsed(win, EXPANDED, Right, len, Start),
            Bounds::new(422, 400, 300, 200)
        );
    }

    #[test]
    fn the_expanded_panel_moves_with_its_collapsed_bar() {
        let win = Bounds::new(50, 500, 260, 30);
        assert_eq!(
            expanded_from_collapsed(win, EXPANDED, Up, None, Start),
            Bounds::new(50, 500, 260, 200)
        );
        assert_eq!(
            expanded_from_collapsed(win, EXPANDED, Down, None, Start),
            Bounds::new(50, 330, 260, 200)
        );
    }

    #[test]
    fn a_down_panel_is_anchored_by_its_bottom_edge() {
        let win = Bounds::new(10, 262, 300, 38);
        assert_eq!(expanded_from_collapsed(win, EXPANDED, Down, None, Start).y, 100);
    }

    #[test]
    fn a_sideways_panel_moves_and_stretches_with_its_bar() {
        let win = Bounds::new(600, 40, 30, 350);
        assert_eq!(
            expanded_from_collapsed(win, EXPANDED, Left, None, Start),
            Bounds::new(600, 40, 300, 350)
        );
        assert_eq!(
            expanded_from_collapsed(win, EXPANDED, Right, None, Start),
            Bounds::new(330, 40, 300, 350)
        );
    }
}
