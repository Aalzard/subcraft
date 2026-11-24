//! Fill Area: finds the areas a painted outline closes off, so they can be filled.
//! Works on the whole texture at once (a flood fill cannot be done per tile).

/// The areas to fill: per texel 0 (leave alone) or the 1-based id of its area.
#[derive(Clone, Debug, PartialEq)]
pub struct Areas {
    pub labels: Vec<u32>,
    pub count: u32,
}

const NONE: u32 = u32::MAX;

/// 4-connected components of the texels `open` accepts: per texel its component (or `NONE`),
/// and per component its size and whether it touches a texel `outside` accepts or the texture
/// border.
fn components(res: usize, open: impl Fn(usize) -> bool, outside: impl Fn(usize) -> bool) -> (Vec<u32>, Vec<(usize, bool)>) {
    let mut label = vec![NONE; res * res];
    let mut info = Vec::new();
    let mut stack = Vec::new();
    for start in 0..res * res {
        if label[start] != NONE || !open(start) {
            continue;
        }
        let id = info.len() as u32;
        let (mut size, mut leaks) = (0usize, false);
        label[start] = id;
        stack.push(start);
        while let Some(i) = stack.pop() {
            size += 1;
            let (x, y) = (i % res, i / res);
            if x == 0 || y == 0 || x == res - 1 || y == res - 1 {
                leaks = true;
            }
            for (ok, j) in [(x > 0, i.wrapping_sub(1)), (x + 1 < res, i + 1), (y > 0, i.wrapping_sub(res)), (y + 1 < res, i + res)] {
                if !ok {
                    continue;
                }
                if outside(j) {
                    leaks = true;
                } else if label[j] == NONE && open(j) {
                    label[j] = id;
                    stack.push(j);
                }
            }
        }
        info.push((size, leaks));
    }
    (label, info)
}

/// The enclosed areas of a `res` x `res` texture. `wall` marks painted texels (the outline),
/// `inside` the texels on a UV island (everything else is the outside). An area is a
/// 4-connected group of unpainted island texels, so an outline one texel thick holds even on
/// a diagonal.
///
/// With open borders an area is filled when it reaches neither the edge of its UV island nor
/// the texture border. With `closed_borders` those edges act as outline too: of the areas of
/// one island the largest is taken as the outside, and every other one is filled (so a
/// stroke from island edge to island edge cuts off a fillable corner).
pub fn enclosed(res: usize, wall: &[bool], inside: &[bool], closed_borders: bool) -> Areas {
    assert!(wall.len() == res * res && inside.len() == res * res);
    let (label, info) = components(res, |i| inside[i] && !wall[i], |i| !inside[i]);
    let mut fill = vec![false; info.len()];
    if closed_borders {
        // The island of each area, then the largest area per island stays empty.
        let (island, islands) = components(res, |i| inside[i], |_| false);
        let mut largest: Vec<Option<(usize, u32)>> = vec![None; islands.len()];
        let mut area_island = vec![NONE; info.len()];
        for i in 0..res * res {
            if label[i] != NONE {
                area_island[label[i] as usize] = island[i];
            }
        }
        for (area, &(size, _)) in info.iter().enumerate() {
            let slot = &mut largest[area_island[area] as usize];
            if slot.is_none_or(|(best, _)| size > best) {
                *slot = Some((size, area as u32));
            }
        }
        for (area, f) in fill.iter_mut().enumerate() {
            *f = largest[area_island[area] as usize].is_some_and(|(_, keep)| keep != area as u32);
        }
    } else {
        for (f, &(_, leaks)) in fill.iter_mut().zip(&info) {
            *f = !leaks;
        }
    }
    // Compact ids, 1-based, in scan order.
    let mut ids = vec![0u32; info.len()];
    let mut count = 0;
    let labels = label
        .into_iter()
        .map(|l| {
            if l == NONE || !fill[l as usize] {
                return 0;
            }
            if ids[l as usize] == 0 {
                count += 1;
                ids[l as usize] = count;
            }
            ids[l as usize]
        })
        .collect();
    Areas { labels, count }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A map from rows of text: `#` painted, `.` open island, ` ` outside any island.
    fn map(rows: &[&str]) -> (usize, Vec<bool>, Vec<bool>) {
        let res = rows.len();
        assert!(rows.iter().all(|r| r.len() == res));
        let cells: Vec<u8> = rows.iter().flat_map(|r| r.bytes()).collect();
        (res, cells.iter().map(|c| *c == b'#').collect(), cells.iter().map(|c| *c != b' ').collect())
    }

    fn show(a: &Areas, res: usize) -> Vec<String> {
        a.labels.chunks(res).map(|row| row.iter().map(|l| char::from_digit(*l, 36).unwrap()).collect()).collect()
    }

    #[test]
    fn a_closed_outline_is_filled_and_an_open_one_is_not() {
        let (res, wall, inside) = map(&[
            "          ",
            " ........ ",
            " .####... ",
            " .#..#... ",
            " .#..#.## ",
            " .####.#. ",
            " ......#. ",
            " ......## ",
            " ........ ",
            "          ",
        ]);
        let areas = enclosed(res, &wall, &inside, false);
        // The ring's inside is filled; the corner cut off by the L reaches the island edge.
        assert_eq!(areas.count, 1);
        assert_eq!(show(&areas, res)[3], "0001100000");
        assert_eq!(show(&areas, res)[4], "0001100000");
        assert_eq!(areas.labels.iter().filter(|l| **l == 1).count(), 4);
        // Closed borders: the island edge closes the corner too (2 texels), numbered in scan order.
        let closed = enclosed(res, &wall, &inside, true);
        assert_eq!(closed.count, 2);
        assert_eq!(show(&closed, res)[3], "0001100000");
        assert_eq!(show(&closed, res)[5], "0000000020");
        assert_eq!(show(&closed, res)[6], "0000000020");
    }

    #[test]
    fn a_gap_lets_the_fill_leak_and_a_diagonal_line_holds() {
        // One texel missing from the ring: not enclosed.
        let (res, wall, inside) = map(&["........", ".####...", ".#..#...", ".#......", ".####...", "........", "........", "........"]);
        assert_eq!(enclosed(res, &wall, &inside, false).count, 0);
        // A diamond drawn with diagonal steps only (corners touch): still closed.
        let (res, wall, inside) = map(&[".......", "...#...", "..#.#..", ".#...#.", "..#.#..", "...#...", "......."]);
        let areas = enclosed(res, &wall, &inside, false);
        assert_eq!(areas.count, 1);
        assert_eq!(show(&areas, res)[3], "0011100");
    }

    #[test]
    fn the_texture_border_and_island_gaps_count_as_outside() {
        // No islands given (all inside): only the texture border is the outside.
        let (res, wall, inside) = map(&["#####", "#...#", "#...#", "#...#", "#####"]);
        assert_eq!(enclosed(res, &wall, &inside, false).count, 1);
        let (res, wall, inside) = map(&["#.###", "#...#", "#...#", "#...#", "#####"]);
        assert_eq!(enclosed(res, &wall, &inside, false).count, 0, "open towards the texture border");
        // Two islands, each with its own ring: both filled; nothing is filled between islands.
        let (res, wall, inside) = map(&[
            "         ",
            " ###  ...",
            " #.#  ###",
            " ###  #.#",
            "      ###",
            "         ",
            " ....    ",
            " ....    ",
            "         ",
        ]);
        let areas = enclosed(res, &wall, &inside, false);
        assert_eq!(areas.count, 2);
        // Closed borders on an island with no paint at all: nothing to fill.
        let closed = enclosed(res, &wall, &inside, true);
        assert_eq!(closed.labels[6 * res + 2], 0);
        // On the first island the only open area is the ring's inside: with closed borders it
        // is the largest (the outside), so it stays empty; open borders fill it.
        assert_eq!(closed.labels[2 * res + 2], 0);
        assert_eq!(areas.labels[2 * res + 2], 1);
    }
}
