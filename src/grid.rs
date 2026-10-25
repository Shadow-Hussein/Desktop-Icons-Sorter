//! A panel's icon grid in custom order. Cells are numbered in reading order
//! (row * cols + col) for a grid `cols` wide, which depends on the panel's
//! width; saved spots are columns and rows, so they survive resizing.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::model::PlacedItem;

/// Windows file names ignore case.
fn key(name: &str) -> String {
    name.to_lowercase()
}

/// The cell each item shows in, by name. `names` is every item, in the order
/// items without a spot should follow. An item keeps its saved spot unless
/// it's off the right edge (the panel got narrower) or already taken; then it
/// goes to the next free cell. Items without a spot follow the last occupied
/// cell, so gaps the user left stay empty.
pub fn layout_grid(names: &[String], saved: &[PlacedItem], cols: usize) -> HashMap<String, usize> {
    let cols = cols.max(1);
    let saved_by: HashMap<String, &PlacedItem> = saved.iter().map(|p| (key(&p.name), p)).collect();
    let mut taken = HashSet::new();
    let mut out = HashMap::new();
    let next_free = |taken: &HashSet<usize>, from: usize| {
        let mut cell = from;
        while taken.contains(&cell) {
            cell += 1;
        }
        cell
    };

    let mut with_spot: Vec<(&String, &PlacedItem)> =
        names.iter().filter_map(|n| saved_by.get(&key(n)).map(|s| (n, *s))).collect();
    with_spot.sort_by_key(|(_, s)| (s.row, s.col));
    let mut bumped = Vec::new();
    for (name, spot) in with_spot {
        let (row, col) = (spot.row as usize, spot.col as usize);
        let cell = row * cols + col;
        if col < cols && !taken.contains(&cell) {
            taken.insert(cell);
            out.insert(name.clone(), cell);
        } else {
            bumped.push((name, row * cols + col.min(cols - 1)));
        }
    }
    for (name, from) in bumped {
        let cell = next_free(&taken, from);
        taken.insert(cell);
        out.insert(name.clone(), cell);
    }

    let mut last = taken.iter().max().map(|&c| c as isize).unwrap_or(-1);
    for name in names {
        if out.contains_key(name) {
            continue;
        }
        let cell = next_free(&taken, (last + 1) as usize);
        last = cell as isize;
        taken.insert(cell);
        out.insert(name.clone(), cell);
    }
    out
}

/// Puts `moving` (names, in order) into `target` and the cells after it, like
/// a phone's home screen: an item already there shifts one cell along with
/// the ones right after it, up to the first empty cell, so gaps elsewhere
/// stay. A moving item's old cell is freed first. Returns every item's spot.
pub fn place_at(
    layout: &HashMap<String, usize>,
    moving: &[String],
    target: usize,
    cols: usize,
) -> Vec<PlacedItem> {
    let cols = cols.max(1);
    let moving_keys: HashSet<String> = moving.iter().map(|n| key(n)).collect();
    let mut cells: BTreeMap<usize, String> = layout
        .iter()
        .filter(|(name, _)| !moving_keys.contains(&key(name)))
        .map(|(name, &cell)| (cell, name.clone()))
        .collect();
    for (at, name) in (target..).zip(moving) {
        if cells.contains_key(&at) {
            let mut end = at;
            while cells.contains_key(&end) {
                end += 1;
            }
            for i in (at + 1..=end).rev() {
                let shifted = cells.remove(&(i - 1)).expect("the run has no gaps");
                cells.insert(i, shifted);
            }
        }
        cells.insert(at, name.clone());
    }
    cells
        .into_iter()
        .map(|(cell, name)| PlacedItem::new(name, (cell % cols) as u32, (cell / cols) as u32))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    fn map(pairs: &[(&str, usize)]) -> HashMap<String, usize> {
        pairs.iter().map(|&(n, c)| (n.to_string(), c)).collect()
    }

    fn cells_of(out: &[PlacedItem], cols: u32) -> HashMap<String, usize> {
        out.iter().map(|p| (p.name.clone(), (p.row * cols + p.col) as usize)).collect()
    }

    #[test]
    fn packs_items_without_spots_in_the_order_given() {
        assert_eq!(
            layout_grid(&names(&["a", "b", "c"]), &[], 2),
            map(&[("a", 0), ("b", 1), ("c", 2)])
        );
    }

    #[test]
    fn keeps_saved_spots_and_the_gaps_between_them() {
        let saved = [PlacedItem::new("a", 0, 0), PlacedItem::new("b", 3, 1)];
        assert_eq!(layout_grid(&names(&["a", "b"]), &saved, 4), map(&[("a", 0), ("b", 7)]));
    }

    #[test]
    fn puts_items_without_a_spot_after_the_last_occupied_cell() {
        let saved = [PlacedItem::new("a", 2, 0)];
        assert_eq!(
            layout_grid(&names(&["a", "new1", "new2"]), &saved, 4),
            map(&[("a", 2), ("new1", 3), ("new2", 4)])
        );
    }

    #[test]
    fn moves_an_item_off_the_right_edge_and_back_when_wider() {
        let saved = [PlacedItem::new("a", 0, 0), PlacedItem::new("far", 4, 0)];
        assert_eq!(layout_grid(&names(&["a", "far"]), &saved, 3), map(&[("a", 0), ("far", 2)]));
        assert_eq!(layout_grid(&names(&["a", "far"]), &saved, 5), map(&[("a", 0), ("far", 4)]));
    }

    #[test]
    fn resolves_two_items_saved_in_the_same_cell() {
        let saved = [PlacedItem::new("a", 1, 0), PlacedItem::new("b", 1, 0)];
        assert_eq!(layout_grid(&names(&["a", "b"]), &saved, 3), map(&[("a", 1), ("b", 2)]));
    }

    #[test]
    fn matches_saved_names_regardless_of_case() {
        let saved = [PlacedItem::new("game.LNK", 2, 1)];
        assert_eq!(layout_grid(&names(&["Game.lnk"]), &saved, 3), map(&[("Game.lnk", 5)]));
    }

    fn layout() -> HashMap<String, usize> {
        map(&[("a", 0), ("b", 1), ("c", 2), ("d", 5)])
    }

    #[test]
    fn puts_an_item_into_an_empty_cell_and_frees_its_old_one() {
        assert_eq!(
            place_at(&layout(), &names(&["a"]), 7, 4),
            [
                PlacedItem::new("b", 1, 0),
                PlacedItem::new("c", 2, 0),
                PlacedItem::new("d", 1, 1),
                PlacedItem::new("a", 3, 1),
            ]
        );
    }

    #[test]
    fn shifts_the_run_at_the_target_up_to_the_first_gap_only() {
        let out = place_at(&layout(), &names(&["x"]), 1, 4);
        assert_eq!(cells_of(&out, 4), map(&[("a", 0), ("x", 1), ("b", 2), ("c", 3), ("d", 5)]));
    }

    #[test]
    fn lets_a_moved_item_take_a_spot_earlier_in_its_own_run() {
        let out = place_at(&layout(), &names(&["c"]), 0, 4);
        assert_eq!(cells_of(&out, 4), map(&[("c", 0), ("a", 1), ("b", 2), ("d", 5)]));
    }

    #[test]
    fn places_several_items_one_after_another() {
        let out = place_at(&map(&[("a", 0)]), &names(&["x", "y"]), 4, 3);
        assert_eq!(
            out,
            [PlacedItem::new("a", 0, 0), PlacedItem::new("x", 1, 1), PlacedItem::new("y", 2, 1)]
        );
    }
}
