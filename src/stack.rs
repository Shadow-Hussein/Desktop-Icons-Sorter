//! Which panels move when a stacked panel collapses or expands.

use crate::geometry::{SNAP_DISTANCE, visual_bounds};
use crate::model::{Bounds, CollapseDirection, PanelConfig};

/// How far apart (or overlapping) two panels can be and still count as stacked.
pub const STACK_GAP: i32 = SNAP_DISTANCE;
/// How much two stacked panels must overlap across the stack, so corners touching don't count.
const MIN_OVERLAP: i32 = 16;

fn is_vertical(side: CollapseDirection) -> bool {
    !side.is_sideways()
}

/// A panel's place in a stack that runs toward `side`: what it covers along
/// the stack, but its expanded extent across it, so a collapsed tile still
/// holds up the panels under its whole width.
fn stack_rect(p: &PanelConfig, side: CollapseDirection) -> Bounds {
    let v = visual_bounds(p);
    if is_vertical(side) {
        Bounds { x: p.bounds.x, width: p.bounds.width, ..v }
    } else {
        Bounds { y: p.bounds.y, height: p.bounds.height, ..v }
    }
}

fn edge(r: Bounds, side: CollapseDirection) -> i32 {
    match side {
        CollapseDirection::Up => r.y,
        CollapseDirection::Down => r.bottom(),
        CollapseDirection::Left => r.x,
        CollapseDirection::Right => r.right(),
    }
}

fn overlap(a: Bounds, b: Bounds, vertical: bool) -> i32 {
    if vertical {
        a.right().min(b.right()) - a.x.max(b.x)
    } else {
        a.bottom().min(b.bottom()) - a.y.max(b.y)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StackMove {
    pub ids: Vec<String>,
    pub dx: i32,
    pub dy: i32,
}

/// The panels to move when `before` collapses or expands into `after`: those
/// stacked against the edge opposite its bar (the edge that moves), then the
/// panels stacked against those, and so on, all moved as far as that edge did.
pub fn stack_move(before: &PanelConfig, after: &PanelConfig, panels: &[PanelConfig]) -> StackMove {
    let side = before.collapse_direction.opposite();
    let vertical = is_vertical(side);
    let shift = edge(stack_rect(after, side), side) - edge(stack_rect(before, side), side);
    if shift == 0 || !before.stackable {
        return StackMove::default();
    }

    // A panel that doesn't stack stays put and holds up nothing below it.
    let mut rest: Vec<(&str, Bounds)> = panels
        .iter()
        .filter(|p| p.id != before.id && p.stackable)
        .map(|p| (p.id.as_str(), stack_rect(p, side)))
        .collect();
    let mut ids = Vec::new();
    let mut queue = std::collections::VecDeque::from([stack_rect(before, side)]);
    while let Some(r) = queue.pop_front() {
        let far = edge(r, side);
        rest.retain(|&(id, o)| {
            let near = edge(o, side.opposite());
            let stacked = (near - far).abs() <= STACK_GAP && overlap(r, o, vertical) >= MIN_OVERLAP;
            if stacked {
                ids.push(id.to_owned());
                queue.push_back(o);
            }
            !stacked
        });
    }
    if vertical { StackMove { ids, dx: 0, dy: shift } } else { StackMove { ids, dx: shift, dy: 0 } }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::create_panel;
    use crate::model::{FOLDER_CARD_SIZE, HeaderShape, TITLE_BAR_HEIGHT};

    fn panel(id: &str, b: Bounds) -> PanelConfig {
        PanelConfig { id: id.into(), ..create_panel(id, b, &format!("/{id}")) }
    }

    fn collapsed(p: &PanelConfig, c: bool) -> PanelConfig {
        PanelConfig { collapsed: c, ..p.clone() }
    }

    fn moved_y(p: &PanelConfig, dy: i32) -> PanelConfig {
        let mut p = p.clone();
        p.bounds.y += dy;
        p
    }

    fn ids(m: &StackMove) -> Vec<&str> {
        m.ids.iter().map(String::as_str).collect()
    }

    // A column: a on top, b right under it, c under b with a small gap.
    fn column() -> (PanelConfig, PanelConfig, PanelConfig) {
        (
            panel("a", Bounds::new(100, 100, 300, 200)),
            panel("b", Bounds::new(100, 300, 300, 150)),
            panel("c", Bounds::new(120, 456, 200, 100)),
        )
    }
    const FREED: i32 = 200 - TITLE_BAR_HEIGHT;

    #[test]
    fn moves_the_panels_below_up_when_a_panel_collapses_up() {
        let (a, b, c) = column();
        let m = stack_move(&a, &collapsed(&a, true), &[a.clone(), b, c]);
        assert_eq!(m, StackMove { ids: vec!["b".into(), "c".into()], dx: 0, dy: -FREED });
    }

    #[test]
    fn moves_them_back_down_when_it_expands() {
        let (a, b, c) = column();
        let ca = collapsed(&a, true);
        let all = [ca.clone(), moved_y(&b, -FREED), moved_y(&c, -FREED)];
        let m = stack_move(&ca, &collapsed(&ca, false), &all);
        assert_eq!(m, StackMove { ids: vec!["b".into(), "c".into()], dx: 0, dy: FREED });
    }

    #[test]
    fn leaves_panels_above_and_beside_alone() {
        let (a, _, _) = column();
        let above = panel("above", Bounds::new(100, 0, 300, 100));
        let beside = panel("beside", Bounds::new(400, 100, 200, 200));
        let corner = panel("corner", Bounds::new(400, 300, 200, 100));
        let m = stack_move(&a, &collapsed(&a, true), &[a.clone(), above, beside, corner]);
        assert!(m.ids.is_empty());
    }

    #[test]
    fn stops_at_a_gap_wider_than_the_snap_distance() {
        let (a, _, _) = column();
        let far = panel("far", Bounds::new(100, 320, 300, 100));
        assert!(stack_move(&a, &collapsed(&a, true), &[a.clone(), far]).ids.is_empty());
    }

    #[test]
    fn moves_the_panels_above_down_when_a_panel_collapses_down() {
        let (a, b, _) = column();
        let lower = PanelConfig { collapse_direction: CollapseDirection::Down, ..b };
        let m = stack_move(&lower, &collapsed(&lower, true), &[a, lower.clone()]);
        assert_eq!(m, StackMove { ids: vec!["a".into()], dx: 0, dy: 150 - TITLE_BAR_HEIGHT });
    }

    #[test]
    fn works_sideways() {
        let (a, _, _) = column();
        let left =
            PanelConfig { id: "left".into(), collapse_direction: CollapseDirection::Left, ..a };
        let right = panel("right", Bounds::new(400, 120, 200, 200));
        let m = stack_move(&left, &collapsed(&left, true), &[left.clone(), right]);
        assert_eq!(
            m,
            StackMove { ids: vec!["right".into()], dx: -(300 - TITLE_BAR_HEIGHT), dy: 0 }
        );
    }

    #[test]
    fn holds_up_panels_under_the_whole_width_of_a_collapsed_tile() {
        let (a, _, _) = column();
        let tile = PanelConfig { header_shape: HeaderShape::Folder, ..a };
        // Its right part is outside the 122 px tile but under the panel's width.
        let offset = panel("offset", Bounds::new(300, 300, 200, 100));
        let m = stack_move(&tile, &collapsed(&tile, true), &[tile.clone(), offset.clone()]);
        assert_eq!(m, StackMove { ids: vec!["offset".into()], dx: 0, dy: FOLDER_CARD_SIZE - 200 });
        let ct = collapsed(&tile, true);
        let mut raised = offset;
        raised.bounds.y = 100 + FOLDER_CARD_SIZE;
        let m = stack_move(&ct, &collapsed(&ct, false), &[ct.clone(), raised]);
        assert_eq!(ids(&m), ["offset"]);
    }

    #[test]
    fn skips_panels_that_do_not_stack() {
        let (a, b, c) = column();
        let fixed = PanelConfig { stackable: false, ..a.clone() };
        let m =
            stack_move(&fixed, &collapsed(&fixed, true), &[fixed.clone(), b.clone(), c.clone()]);
        assert!(m.ids.is_empty());
        let loose = PanelConfig { stackable: false, ..b };
        assert!(stack_move(&a, &collapsed(&a, true), &[a.clone(), loose, c]).ids.is_empty());
    }

    #[test]
    fn does_nothing_when_the_edge_does_not_move() {
        let (a, _, _) = column();
        let side = PanelConfig { collapse_direction: CollapseDirection::Left, ..a };
        let under = panel("under", Bounds::new(100, 300, 300, 100));
        assert!(stack_move(&side, &collapsed(&side, true), &[side.clone(), under]).ids.is_empty());
    }
}
