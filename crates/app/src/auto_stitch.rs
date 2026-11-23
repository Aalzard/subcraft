//! Auto Stitcher (Painter's tool of that name): stitches placed without drawing, along the UV
//! seams of the model, along its open borders, or along the edge of a layer's mask. Each line
//! it finds becomes an ordinary pen path layer of the stitch (or stitch tile) chosen for the Pen
//! tool, so every row can be moved, restyled or deleted afterwards like a hand-drawn one.

use std::collections::HashMap;
use std::hash::Hash;

use glam::Vec2;
use subcraft_doc::PenAnchor;
use subcraft_mesh::Mesh;
use subcraft_mesh::uv_raster::{raster_triangle, wrap_to_tile};

use crate::pen::PenEditor;
use crate::{Command, CommandError, Session};

/// Where the Auto Stitcher puts its stitches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StitchAlong {
    /// Edges where two UV islands meet on the model.
    #[default]
    Seams,
    /// Open edges of the mesh (hems).
    Borders,
    SeamsAndBorders,
    /// The edge of the selected layer's mask.
    MaskEdge,
}

impl StitchAlong {
    pub const ALL: [Self; 4] = [Self::Seams, Self::Borders, Self::SeamsAndBorders, Self::MaskEdge];

    pub fn label(self) -> &'static str {
        match self {
            Self::Seams => "UV seams",
            Self::Borders => "Open borders",
            Self::SeamsAndBorders => "UV seams and open borders",
            Self::MaskEdge => "Edge of the selected layer's mask",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutoStitch {
    pub along: StitchAlong,
    /// How far from the edge the row runs, in stitch widths. At 0 a UV seam gets one row right
    /// on it; above 0 it gets a row on each side.
    pub inset: f32,
}

impl AutoStitch {
    pub const INSET: std::ops::RangeInclusive<f32> = 0.0..=10.0;
}

impl Default for AutoStitch {
    fn default() -> Self {
        Self { along: StitchAlong::Seams, inset: 1.0 }
    }
}

/// What [`PenEditor::auto_stitch`] made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoStitched {
    /// Path layers added.
    pub paths: usize,
    /// Lines left out because there were more than [`MAX_LINES`] (the shortest ones).
    pub skipped: usize,
}

/// Most path layers one run adds: the longest lines are kept.
pub const MAX_LINES: usize = 64;
/// Longest line of one path (a path holds at most [`subcraft_doc::PenPath::MAX_ANCHORS`]).
const MAX_POINTS: usize = 200;
/// The mask is traced on a grid of at most this many texels a side.
const MASK_GRID: u32 = 1024;

/// One line of stitches, in the UV square of the texture set.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub points: Vec<Vec2>,
    pub closed: bool,
    /// Size of the stitches of this line against the chosen size: more where the UV island is
    /// larger for its surface, so the stitches are the same size all over the model.
    pub scale: f32,
}

impl Line {
    pub fn length(&self) -> f32 {
        let open: f32 = self.points.windows(2).map(|w| w[0].distance(w[1])).sum();
        match (self.closed, self.points.first(), self.points.last()) {
            (true, Some(a), Some(b)) => open + a.distance(*b),
            _ => open,
        }
    }
}

/// Segments joined end to end into lines: for each line its segments in order (the flag says
/// the segment runs backwards) and whether it closes on itself. A line ends where it is not
/// exactly two segments that meet.
fn chains<K: Hash + Eq + Copy>(ends: &[(K, K)]) -> Vec<(Vec<(usize, bool)>, bool)> {
    let mut at: HashMap<K, Vec<usize>> = HashMap::new();
    for (i, &(a, b)) in ends.iter().enumerate() {
        at.entry(a).or_default().push(i);
        at.entry(b).or_default().push(i);
    }
    let through = |k: &K| at[k].len() == 2;
    let mut used = vec![false; ends.len()];
    let mut out = Vec::new();
    // Lines with ends first, from an end; what is left are closed loops.
    for loops in [false, true] {
        for i in 0..ends.len() {
            if used[i] {
                continue;
            }
            let (a, b) = ends[i];
            let reversed = if loops || !through(&a) {
                false
            } else if !through(&b) {
                true
            } else {
                continue;
            };
            let start = if reversed { b } else { a };
            let mut end = if reversed { a } else { b };
            let mut line = vec![(i, reversed)];
            used[i] = true;
            while end != start && through(&end) {
                let Some(&next) = at[&end].iter().find(|&&s| !used[s]) else { break };
                used[next] = true;
                let back = ends[next].1 == end;
                line.push((next, back));
                end = if back { ends[next].0 } else { ends[next].1 };
            }
            out.push((line, end == start));
        }
    }
    out
}

/// The line moved `distance` to the side its segments' `normals` point to (one normal per
/// segment; a closed line has as many segments as points). The ends of an open line are pulled
/// in by the same distance, so its first and last stitch stay clear of what the line ends on.
fn inset_line(points: &[Vec2], normals: &[Vec2], closed: bool, distance: f32) -> Vec<Vec2> {
    let n = points.len();
    if n < 2 || distance <= 0.0 {
        return points.to_vec();
    }
    let segments = normals.len();
    let mut out: Vec<Vec2> = (0..n)
        .map(|i| {
            let before = if closed { Some((i + segments - 1) % segments) } else { i.checked_sub(1) };
            let after = if closed { Some(i % segments) } else { (i < segments).then_some(i) };
            let (a, b) = match (before, after) {
                (Some(a), Some(b)) => (normals[a], normals[b]),
                (Some(a), None) | (None, Some(a)) => (normals[a], normals[a]),
                (None, None) => (Vec2::ZERO, Vec2::ZERO),
            };
            let m = (a + b).normalize_or(a);
            // A mitre, at most twice the distance at a sharp corner.
            points[i] + m * distance / m.dot(a).max(0.5)
        })
        .collect();
    let length: f32 = out.windows(2).map(|w| w[0].distance(w[1])).sum();
    if !closed && length > distance * 4.0 {
        let first = (out[1] - out[0]).normalize_or_zero();
        let last = (out[n - 1] - out[n - 2]).normalize_or_zero();
        let (head, tail) = (out[0].distance(out[1]), out[n - 1].distance(out[n - 2]));
        out[0] += first * distance.min(head * 0.9);
        out[n - 1] -= last * distance.min(tail * 0.9);
    }
    out
}

/// Douglas-Peucker: the points of an open line that keep it within `tolerance`.
fn simplify_open(points: &[Vec2], tolerance: f32) -> Vec<Vec2> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0, points.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let d = points[b] - points[a];
        let len = d.length();
        let (mut far, mut at) = (tolerance, None);
        for i in a + 1..b {
            let off = if len > 1e-12 { (d.perp_dot(points[i] - points[a]) / len).abs() } else { points[i].distance(points[a]) };
            if off > far {
                (far, at) = (off, Some(i));
            }
        }
        if let Some(i) = at {
            keep[i] = true;
            stack.push((a, i));
            stack.push((i, b));
        }
    }
    points.iter().zip(&keep).filter(|(_, k)| **k).map(|(p, _)| *p).collect()
}

fn simplify(line: Line, tolerance: f32) -> Line {
    if !line.closed || line.points.len() < 4 {
        return Line { points: simplify_open(&line.points, tolerance), closed: line.closed && line.points.len() > 2, scale: line.scale };
    }
    // A loop: two halves between its first point and the one farthest from it.
    let p = &line.points;
    let far = (1..p.len()).max_by(|&a, &b| p[a].distance_squared(p[0]).total_cmp(&p[b].distance_squared(p[0]))).unwrap_or(1);
    let mut points = simplify_open(&p[..=far], tolerance);
    let mut back: Vec<Vec2> = p[far..].to_vec();
    back.push(p[0]);
    let back = simplify_open(&back, tolerance);
    points.extend_from_slice(&back[1..back.len() - 1]);
    // The two points the halves were cut at may lie on a straight run.
    for cut in [0, points.len().min(far)] {
        let n = points.len();
        if n < 4 || cut >= n {
            continue;
        }
        let i = points.iter().position(|&q| q == if cut == 0 { p[0] } else { p[far] });
        let Some(i) = i else { continue };
        let (a, b) = (points[(i + n - 1) % n], points[(i + 1) % n]);
        let d = b - a;
        if d.length() > 1e-12 && (d.perp_dot(points[i] - a) / d.length()).abs() <= tolerance && (points[i] - a).dot(d) > 0.0 && (points[i] - b).dot(d) < 0.0 {
            points.remove(i);
        }
    }
    let closed = points.len() > 2;
    Line { points, closed, scale: line.scale }
}

/// Simplified, the short ones dropped, the long ones cut into pieces a path can hold.
fn finish(lines: Vec<Line>, tolerance: f32, min_length: f32) -> Vec<Line> {
    let mut out = Vec::new();
    for line in lines {
        let scale = line.scale;
        let line = simplify(line, tolerance * scale);
        if line.points.len() < 2 || line.length() < min_length * scale {
            continue;
        }
        if line.points.len() <= MAX_POINTS {
            out.push(line);
            continue;
        }
        let mut points = line.points;
        if line.closed {
            points.push(points[0]);
        }
        let mut from = 0;
        while from + 1 < points.len() {
            let to = (from + MAX_POINTS - 1).min(points.len() - 1);
            out.push(Line { points: points[from..=to].to_vec(), closed: false, scale });
            from = to;
        }
    }
    out
}

/// Per triangle, how large its UV island is for its surface against the mean of material `slot`
/// (1 everywhere on a mesh of even texel density). Every triangle of an island has the same
/// value.
pub fn relative_scales(mesh: &Mesh, slot: u32) -> Vec<f32> {
    let islands = subcraft_mesh::topology::uv_islands(mesh);
    let mut areas: HashMap<u32, (f64, f64)> = HashMap::new();
    let mut total = (0.0f64, 0.0f64);
    for t in (0..mesh.triangle_count()).filter(|&t| mesh.tri_material[t] == slot) {
        let (uv, p) = (mesh.tri_uvs(t), mesh.tri_positions(t));
        let in_uv = ((uv[1] - uv[0]).perp_dot(uv[2] - uv[0]).abs() * 0.5) as f64;
        let on_model = ((p[1] - p[0]).cross(p[2] - p[0]).length() * 0.5) as f64;
        let a = areas.entry(islands[t]).or_default();
        *a = (a.0 + in_uv, a.1 + on_model);
        total = (total.0 + in_uv, total.1 + on_model);
    }
    let density = |a: (f64, f64)| if a.0 > 0.0 && a.1 > 0.0 { (a.0 / a.1).sqrt() } else { 0.0 };
    let mean = density(total);
    (0..mesh.triangle_count())
        .map(|t| {
            let d = areas.get(&islands[t]).map_or(0.0, |&a| density(a));
            if mean > 0.0 && d > 0.0 { ((d / mean) as f32).clamp(0.1, 10.0) } else { 1.0 }
        })
        .collect()
}

/// Lines along the UV seams (`seams`) and open borders (`borders`) of the triangles of material
/// `slot`, `inset` UV units inside their UV island. A seam has a line in the island on each side
/// of it; with `on_seam` it has only one, for a single row right on it (give a tiny `inset`).
/// Lines shorter than `min_length` are left out. The three lengths are multiplied by the
/// [`Line::scale`] of the island the line is in.
#[allow(clippy::too_many_arguments)]
pub fn seam_lines(mesh: &Mesh, slot: u32, seams: bool, borders: bool, on_seam: bool, inset: f32, tolerance: f32, min_length: f32) -> Vec<Line> {
    let scales = relative_scales(mesh, slot);
    let weld = subcraft_mesh::topology::weld_positions(mesh);
    let islands = subcraft_mesh::topology::uv_islands(mesh);
    let bits = |p: Vec2| [p.x.to_bits(), p.y.to_bits()];
    let corners = |t: usize, k: usize| {
        let c = mesh.tri(t);
        (c[k] as usize, c[(k + 1) % 3] as usize)
    };
    // Every use of a welded edge: (triangle, corner).
    let mut uses: HashMap<(u32, u32), Vec<(usize, usize)>> = HashMap::new();
    for t in 0..mesh.triangle_count() {
        for k in 0..3 {
            let (a, b) = corners(t, k);
            let (a, b) = (weld[a], weld[b]);
            if a != b {
                uses.entry((a.min(b), a.max(b))).or_default().push((t, k));
            }
        }
    }
    // The edge as one of its triangles has it in UV, the same whichever way it runs.
    let in_uv = |&(t, k): &(usize, usize)| {
        let (a, b) = corners(t, k);
        let mut ends = [(weld[a], bits(mesh.uvs[a])), (weld[b], bits(mesh.uvs[b]))];
        ends.sort();
        (ends, mesh.tri_material[t])
    };
    let mut sides: Vec<(usize, usize)> = Vec::new();
    for list in uses.values() {
        let mut mine: Vec<(usize, usize)> = list.iter().copied().filter(|&(t, _)| mesh.tri_material[t] == slot).collect();
        if mine.is_empty() {
            continue;
        }
        let border = list.len() == 1;
        let seam = !border && list[1..].iter().any(|u| in_uv(u) != in_uv(&list[0]));
        if !((border && borders) || (seam && seams)) {
            continue;
        }
        mine.sort();
        if seam && on_seam {
            mine.truncate(1);
        }
        sides.extend(mine);
    }
    sides.sort();
    // Each side as a UV segment of its island, with the direction into the island.
    let mut ends = Vec::new();
    let mut segments = Vec::new();
    for &(t, k) in &sides {
        let (uv, _) = wrap_to_tile(mesh.tri_uvs(t));
        let (a, b, c) = (uv[k], uv[(k + 1) % 3], uv[(k + 2) % 3]);
        let n = (b - a).perp().normalize_or_zero();
        if n == Vec2::ZERO {
            continue;
        }
        let inward = if n.dot(c - a) < 0.0 { -n } else { n };
        ends.push(((islands[t], bits(a)), (islands[t], bits(b))));
        segments.push((a, b, inward, scales[t]));
    }
    let mut lines = Vec::new();
    for (line, closed) in chains(&ends) {
        let mut points = Vec::with_capacity(line.len() + 1);
        let mut normals = Vec::with_capacity(line.len());
        let scale = segments[line[0].0].3;
        for (i, &(s, reversed)) in line.iter().enumerate() {
            let (a, b, n, _) = segments[s];
            let (a, b) = if reversed { (b, a) } else { (a, b) };
            if i == 0 {
                points.push(a);
            }
            if !(closed && i + 1 == line.len()) {
                points.push(b);
            }
            normals.push(n);
        }
        lines.push(Line { points: inset_line(&points, &normals, closed, inset * scale), closed, scale });
    }
    finish(lines, tolerance, min_length)
}

/// Lines along the edge of a mask: where `mask` (`size` x `size` values, row by row) crosses
/// one half, moved `inset` UV units into the masked side. Only where the texels are `covered`
/// by the mesh, so the border of a UV island is not taken for an edge.
pub fn mask_lines(mask: &[f32], size: usize, covered: &[bool], inset: f32, tolerance: f32, min_length: f32) -> Vec<Line> {
    if size < 2 || mask.len() < size * size || covered.len() < size * size {
        return Vec::new();
    }
    let value = |x: usize, y: usize| mask[y * size + x];
    // A crossing lies on a grid edge: (x, y, 0) runs from texel (x, y) to (x + 1, y), and
    // (x, y, 1) to (x, y + 1).
    type Edge = (u32, u32, u8);
    let mut at: HashMap<Edge, Vec2> = HashMap::new();
    let mut ends: Vec<(Edge, Edge)> = Vec::new();
    for y in 0..size - 1 {
        for x in 0..size - 1 {
            let v = [value(x, y), value(x + 1, y), value(x + 1, y + 1), value(x, y + 1)];
            let case = (0..4).fold(0u8, |c, i| c | (u8::from(v[i] >= 0.5) << i));
            if case == 0 || case == 15 {
                continue;
            }
            if !(covered[y * size + x] && covered[y * size + x + 1] && covered[(y + 1) * size + x] && covered[(y + 1) * size + x + 1]) {
                continue;
            }
            let (top, right, bottom, left): (Edge, Edge, Edge, Edge) = ((x as u32, y as u32, 0), (x as u32 + 1, y as u32, 1), (x as u32, y as u32 + 1, 0), (x as u32, y as u32, 1));
            let joined = v.iter().sum::<f32>() >= 2.0;
            let pairs: &[(Edge, Edge)] = match case {
                1 | 14 => &[(left, top)],
                2 | 13 => &[(top, right)],
                3 | 12 => &[(left, right)],
                4 | 11 => &[(right, bottom)],
                6 | 9 => &[(top, bottom)],
                7 | 8 => &[(left, bottom)],
                // Two corners across: joined through the middle or not, by the cell's mean.
                5 if joined => &[(top, right), (left, bottom)],
                5 => &[(left, top), (right, bottom)],
                10 if joined => &[(left, top), (right, bottom)],
                _ => &[(top, right), (left, bottom)],
            };
            let cross = |a: f32, b: f32| if (b - a).abs() > 1e-9 { ((0.5 - a) / (b - a)).clamp(0.0, 1.0) } else { 0.5 };
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            for &(a, b) in pairs {
                for e in [a, b] {
                    at.entry(e).or_insert_with(|| {
                        let p = if e == top {
                            Vec2::new(fx + cross(v[0], v[1]), fy)
                        } else if e == right {
                            Vec2::new(fx + 1.0, fy + cross(v[1], v[2]))
                        } else if e == bottom {
                            Vec2::new(fx + cross(v[3], v[2]), fy + 1.0)
                        } else {
                            Vec2::new(fx, fy + cross(v[0], v[3]))
                        };
                        p / size as f32
                    });
                }
                ends.push((a, b));
            }
        }
    }
    let nearest = |p: Vec2| {
        let c = (p * size as f32).floor().clamp(Vec2::ZERO, Vec2::splat(size as f32 - 1.0));
        value(c.x as usize, c.y as usize)
    };
    let mut lines = Vec::new();
    for (line, closed) in chains(&ends) {
        let mut points = Vec::with_capacity(line.len() + 1);
        for (i, &(s, reversed)) in line.iter().enumerate() {
            let (a, b) = if reversed { (ends[s].1, ends[s].0) } else { ends[s] };
            if i == 0 {
                points.push(at[&a]);
            }
            if !(closed && i + 1 == line.len()) {
                points.push(at[&b]);
            }
        }
        // The steps of the texel grid smoothed away.
        for _ in 0..2 {
            let n = points.len();
            let before = points.clone();
            for i in 0..n {
                let (a, b) = if closed { (before[(i + n - 1) % n], before[(i + 1) % n]) } else if i == 0 || i + 1 == n { continue } else { (before[i - 1], before[i + 1]) };
                points[i] = (a + before[i] * 2.0 + b) * 0.25;
            }
        }
        if inset > 0.0 && points.len() > 1 {
            // Which side is masked: the higher mask value a texel and a half off the line.
            let n = points.len();
            let segments = if closed { n } else { n - 1 };
            let mut normals: Vec<Vec2> = (0..segments).map(|i| (points[(i + 1) % n] - points[i]).perp().normalize_or_zero()).collect();
            let off = 1.5 / size as f32;
            let vote: f32 = (0..segments).map(|i| {
                let mid = (points[i] + points[(i + 1) % n]) * 0.5;
                nearest(mid + normals[i] * off) - nearest(mid - normals[i] * off)
            }).sum();
            if vote < 0.0 {
                normals.iter_mut().for_each(|n| *n = -*n);
            }
            points = inset_line(&points, &normals, closed, inset);
        }
        lines.push(Line { points, closed, scale: 1.0 });
    }
    finish(lines, tolerance, min_length)
}

impl PenEditor {
    /// Place the Pen tool's stitch (or stitch tile, or plain line) along the seams, borders or
    /// mask edge `options` names, on the selected texture set: one path layer per line, in a
    /// folder when there are several, as one undo step.
    pub fn auto_stitch(&mut self, session: &mut Session, options: AutoStitch) -> Result<AutoStitched, CommandError> {
        let invalid = |s: &str| CommandError::Invalid(s.into());
        let (template, name) = self.styled_path(session);
        let radius = template.radius;
        let inset = options.inset.clamp(*AutoStitch::INSET.start(), *AutoStitch::INSET.end()) * 2.0 * radius;
        let (slot, resolution) = {
            let ts = session.selected_texture_set();
            (ts.material_slot, ts.resolution)
        };
        let tolerance = (radius * 0.15).max(0.5 / resolution as f32);
        let min_length = radius * 8.0;
        let mesh = session.scene().mesh.clone();
        let mut lines = match options.along {
            StitchAlong::MaskEdge => {
                let layer = session.selection().layer.ok_or_else(|| invalid("select a layer with a mask"))?;
                let mask = session.mask_image(layer)?;
                let size = resolution.min(MASK_GRID).max(2);
                let step = (resolution / size).max(1);
                let size = (resolution / step) as usize;
                let values: Vec<f32> = (0..size * size).map(|i| mask.image.get((i % size) as u32 * step + step / 2, (i / size) as u32 * step + step / 2)[0]).collect();
                let mut covered = vec![false; size * size];
                let clip = subcraft_core::IRect::from_size(size as u32, size as u32);
                for t in (0..mesh.triangle_count()).filter(|&t| mesh.tri_material[t] == slot) {
                    let (uv, _) = wrap_to_tile(mesh.tri_uvs(t));
                    raster_triangle(uv, size as u32, size as u32, clip, 0.0, |h| {
                        if h.inside {
                            covered[h.y as usize * size + h.x as usize] = true;
                        }
                    });
                }
                let mut lines = mask_lines(&values, size, &covered, inset, tolerance.max(0.75 / size as f32), min_length);
                // The size of the island under each line's first point.
                let scales = relative_scales(&mesh, slot);
                for line in &mut lines {
                    let p = line.points[0];
                    let under = (0..mesh.triangle_count()).filter(|&t| mesh.tri_material[t] == slot).find(|&t| {
                        let (uv, _) = wrap_to_tile(mesh.tri_uvs(t));
                        let side = |a: Vec2, b: Vec2| (b - a).perp_dot(p - a);
                        let d = [side(uv[0], uv[1]), side(uv[1], uv[2]), side(uv[2], uv[0])];
                        d.iter().all(|&x| x >= 0.0) || d.iter().all(|&x| x <= 0.0)
                    });
                    line.scale = under.map_or(1.0, |t| scales[t]);
                }
                lines
            }
            along => {
                let (seams, borders) = (along != StitchAlong::Borders, along != StitchAlong::Seams);
                // Never right on the edge: a point there is on no island.
                seam_lines(&mesh, slot, seams, borders, inset <= 0.0, inset.max(radius * 0.05), tolerance, min_length)
            }
        };
        if lines.is_empty() {
            return Err(invalid(match options.along {
                StitchAlong::Seams => "no UV seams long enough for a row of stitches",
                StitchAlong::Borders => "no open borders long enough for a row of stitches",
                StitchAlong::SeamsAndBorders => "no UV seams or open borders long enough for a row of stitches",
                StitchAlong::MaskEdge => "the mask has no edge long enough for a row of stitches",
            }));
        }
        lines.sort_by(|a, b| b.length().total_cmp(&a.length()));
        let skipped = lines.len().saturating_sub(MAX_LINES);
        lines.truncate(MAX_LINES);
        let mut layers = Vec::with_capacity(lines.len());
        let mut steps = 0;
        let mut add = |editor: &mut Self, session: &mut Session| -> Result<(), CommandError> {
            for line in &lines {
                let mut path = template.clone();
                path.anchors = line.points.iter().map(|&p| PenAnchor::new(p)).collect();
                path.closed = line.closed;
                path.radius = (path.radius * line.scale).clamp(0.00001, 0.5);
                let layer = editor.drawn_layer(session, path, name)?;
                layers.push(layer.id);
                session.execute(Command::InsertLayer { layer: Box::new(layer), label: "Auto Stitcher".into() })?;
                steps += 1;
            }
            if layers.len() > 1 {
                session.execute(Command::GroupLayers(layers.clone()))?;
                steps += 1;
                if let Some(folder) = session.selection().layer {
                    session.execute(Command::Rename { layer: folder, name: "Auto stitches".into() })?;
                    steps += 1;
                }
            }
            Ok(())
        };
        let result = add(self, session);
        if steps > 1 {
            session.squash_history(steps);
        }
        self.finish_path();
        result.map(|()| AutoStitched { paths: lines.len(), skipped })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_core::ChannelKind;
    use subcraft_doc::LayerContent;

    #[test]
    fn segments_are_joined_into_open_lines_and_loops() {
        // An open line 1-2-3-4 given out of order and partly backwards, and a loop 10-11-12.
        let ends = [(2, 3), (2, 1), (4, 3), (10, 11), (12, 10), (11, 12)];
        let mut found = chains(&ends);
        found.sort_by_key(|(line, _)| line.len());
        assert_eq!(found.len(), 2);
        let (open, closed) = (&found[0], &found[1]);
        assert!(!open.1 && closed.1);
        let walk = |line: &[(usize, bool)]| -> Vec<i32> {
            let mut out = Vec::new();
            for (i, &(s, back)) in line.iter().enumerate() {
                let (a, b) = if back { (ends[s].1, ends[s].0) } else { ends[s] };
                if i == 0 {
                    out.push(a);
                }
                out.push(b);
            }
            out
        };
        let w = walk(&open.0);
        assert!(w == [1, 2, 3, 4] || w == [4, 3, 2, 1], "{w:?}");
        let w = walk(&closed.0);
        assert!(w.len() == 4 && w[0] == w[3] && [10, 11, 12].iter().all(|k| w.contains(k)), "{w:?}");
        // Three lines meeting in one point end there.
        assert_eq!(chains(&[(0, 1), (0, 2), (0, 3)]).len(), 3);
    }

    #[test]
    fn a_line_is_moved_to_its_inner_side_and_simplified() {
        // A square loop, counter-clockwise with the inside on its normals' side.
        let square = [Vec2::new(0.2, 0.2), Vec2::new(0.8, 0.2), Vec2::new(0.8, 0.8), Vec2::new(0.2, 0.8)];
        let normals = [Vec2::Y, -Vec2::X, -Vec2::Y, Vec2::X];
        let inner = inset_line(&square, &normals, true, 0.1);
        for (p, want) in inner.iter().zip([(0.3, 0.3), (0.7, 0.3), (0.7, 0.7), (0.3, 0.7)]) {
            assert!(p.distance(Vec2::new(want.0, want.1)) < 1e-5, "{p:?} for {want:?}");
        }
        // An open line also loses the distance at both ends.
        let open = inset_line(&[Vec2::new(0.1, 0.5), Vec2::new(0.9, 0.5)], &[Vec2::Y], false, 0.05);
        assert!(open[0].distance(Vec2::new(0.15, 0.55)) < 1e-5 && open[1].distance(Vec2::new(0.85, 0.55)) < 1e-5, "{open:?}");
        // Points on a straight run go, the corner stays.
        let bent: Vec<Vec2> = (0..=10).map(|i| Vec2::new(i as f32 * 0.05, 0.0)).chain((1..=10).map(|i| Vec2::new(0.5, i as f32 * 0.05))).collect();
        let s = simplify(Line { points: bent, closed: false, scale: 1.0 }, 0.001);
        assert_eq!(s.points, [Vec2::ZERO, Vec2::new(0.5, 0.0), Vec2::new(0.5, 0.5)]);
        // A long line is cut into pieces a path can hold, end to end.
        let wavy: Vec<Vec2> = (0..500).map(|i| Vec2::new(i as f32 * 0.001, if i % 2 == 0 { 0.0 } else { 0.1 })).collect();
        let pieces = finish(vec![Line { points: wavy, closed: false, scale: 1.0 }], 0.001, 0.01);
        assert_eq!(pieces.len(), 3);
        assert!(pieces.iter().all(|l| l.points.len() <= MAX_POINTS && !l.closed));
        assert_eq!(pieces[0].points.last(), pieces[1].points.first());
    }

    /// A cube: every face is its own UV island, so all 12 edges are seams and there is no
    /// open border. Beside the seams every face gets one closed row; on the seams each of the
    /// 12 edges gets one row.
    #[test]
    fn a_cube_gets_a_row_inside_every_face_or_one_on_every_edge() {
        let mesh = subcraft_mesh::primitives::cube(2.0);
        assert!(relative_scales(&mesh, 0).iter().all(|s| (s - 1.0).abs() < 1e-3), "every face has the same share of the texture");
        let beside = seam_lines(&mesh, 0, true, false, false, 0.02, 0.001, 0.05);
        assert_eq!(beside.len(), 6, "one loop per face");
        let islands = subcraft_mesh::topology::uv_islands(&mesh);
        for line in &beside {
            assert!(line.closed && line.points.len() == 4, "a square: {line:?}");
            // Every corner lies inside a triangle of one face, 0.02 from two of its sides.
            let inside: Vec<u32> = line.points.iter().map(|&p| {
                let t = (0..mesh.triangle_count()).find(|&t| {
                    let uv = mesh.tri_uvs(t);
                    let s = |a: Vec2, b: Vec2| (b - a).perp_dot(p - a);
                    let (d0, d1, d2) = (s(uv[0], uv[1]), s(uv[1], uv[2]), s(uv[2], uv[0]));
                    (d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0) || (d0 <= 0.0 && d1 <= 0.0 && d2 <= 0.0)
                });
                islands[t.unwrap_or_else(|| panic!("{p:?} is on no face"))]
            }).collect();
            assert!(inside.iter().all(|&i| i == inside[0]), "one island per loop: {inside:?}");
        }
        // The sides of the faces in UV (the layout is not square), back out to the seam.
        let sides: Vec<f32> = beside.iter().flat_map(|l| (0..4).map(move |i| l.points[i].distance(l.points[(i + 1) % 4]) + 0.04)).collect();
        let (short, long) = (sides.iter().copied().fold(f32::MAX, f32::min), sides.iter().copied().fold(0.0, f32::max));
        let on = seam_lines(&mesh, 0, true, false, true, 0.0005, 0.001, 0.05);
        // Rows that meet in a corner of one face are one line, so there can be fewer than 12.
        assert!(on.len() <= 12 && !on.is_empty());
        let total: f32 = on.iter().map(Line::length).sum();
        assert!(total > 12.0 * short - 0.05 && total < 12.0 * long + 0.05, "together as long as the 12 edges: {total}");
        // No open borders on a closed cube; a plane is one island with a border and no seam.
        assert!(seam_lines(&mesh, 0, false, true, false, 0.02, 0.001, 0.05).is_empty());
        let plane = subcraft_mesh::primitives::plane(2.0, 4);
        assert!(seam_lines(&plane, 0, true, false, false, 0.02, 0.001, 0.05).is_empty());
        let hem = seam_lines(&plane, 0, false, true, false, 0.05, 0.001, 0.05);
        assert_eq!(hem.len(), 1);
        assert!(hem[0].closed && hem[0].points.len() == 4);
        assert!(hem[0].points.iter().all(|p| (p.x - 0.5).abs().max((p.y - 0.5).abs()) - 0.45 < 1e-4), "{:?}", hem[0].points);
        // Another material slot has none of it.
        assert!(seam_lines(&plane, 3, true, true, false, 0.05, 0.001, 0.05).is_empty());
    }

    #[test]
    fn the_edge_of_a_mask_is_traced_as_one_smooth_loop() {
        // A disc of radius 0.3 in a 128 grid, soft over two texels.
        let size = 128usize;
        let mask: Vec<f32> = (0..size * size).map(|i| {
            let p = Vec2::new((i % size) as f32 + 0.5, (i / size) as f32 + 0.5) / size as f32;
            ((0.3 - p.distance(Vec2::splat(0.5))) * size as f32 * 0.5 + 0.5).clamp(0.0, 1.0)
        }).collect();
        let all = vec![true; size * size];
        let lines = mask_lines(&mask, size, &all, 0.0, 0.002, 0.1);
        assert_eq!(lines.len(), 1);
        let circle = &lines[0];
        assert!(circle.closed && circle.points.len() > 12 && circle.points.len() < MAX_POINTS, "{} points", circle.points.len());
        for p in &circle.points {
            assert!((p.distance(Vec2::splat(0.5)) - 0.3).abs() < 0.006, "{p:?} is off the edge");
        }
        assert!((circle.length() - std::f32::consts::TAU * 0.3).abs() < 0.05, "length {}", circle.length());
        // Moved in: on the masked side, whichever way the loop runs.
        let inner = &mask_lines(&mask, size, &all, 0.05, 0.002, 0.1)[0];
        assert!(inner.points.iter().all(|p| (p.distance(Vec2::splat(0.5)) - 0.25).abs() < 0.008));
        let inverted: Vec<f32> = mask.iter().map(|v| 1.0 - v).collect();
        let outer = &mask_lines(&inverted, size, &all, 0.05, 0.002, 0.1)[0];
        assert!(outer.points.iter().all(|p| (p.distance(Vec2::splat(0.5)) - 0.35).abs() < 0.008));
        // Where the mesh does not cover the texture there is no edge: the left half only.
        let left: Vec<bool> = (0..size * size).map(|i| i % size < size / 2).collect();
        let half = mask_lines(&mask, size, &left, 0.0, 0.002, 0.1);
        assert_eq!(half.len(), 1);
        assert!(!half[0].closed && half[0].points.iter().all(|p| p.x < 0.5));
    }

    fn cube_session() -> Session {
        let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 256);
        let ts = s.selected_texture_set().id;
        if !s.channels().contains(&ChannelKind::Height) {
            s.execute(Command::AddChannel { texture_set: ts, kind: ChannelKind::Height }).unwrap();
        }
        s
    }

    fn path_layers(s: &Session) -> usize {
        let mut n = 0;
        s.selected_texture_set().walk(|l, _| n += usize::from(matches!(&l.content, LayerContent::Paint(p) if p.path.is_some())));
        n
    }

    #[test]
    fn stitches_along_the_seams_of_a_cube_are_path_layers_in_one_folder_and_one_undo_step() {
        let mut s = cube_session();
        let tips = subcraft_assets::alphas::builtin();
        let mut pen = PenEditor::default();
        pen.style = Some(crate::stitch::style(&mut s, crate::stitch::find("Stitch: Straight").unwrap(), &tips));
        let before = s.history().cursor();
        let made = pen.auto_stitch(&mut s, AutoStitch::default()).unwrap();
        assert_eq!(made, AutoStitched { paths: 6, skipped: 0 });
        assert_eq!(path_layers(&s), 6);
        let folder = s.selected_layer().unwrap();
        assert_eq!(folder.name, "Auto stitches");
        assert_eq!(s.history().cursor(), before + 1, "one undo step");
        // Stitches were painted: thread colour and relief on the texture.
        s.update_composites();
        let composite = s.composite(s.selected_texture_set().id).unwrap();
        let height = composite.channel(ChannelKind::Height).unwrap();
        let raised = (0..256 * 256).filter(|i| height.get(i % 256, i / 256)[0] > 0.05).count();
        assert!(raised > 500, "{raised} raised texels");
        s.execute(Command::Undo).unwrap();
        assert_eq!(path_layers(&s), 0, "undone in one step");
        // One row on each seam.
        let made = pen.auto_stitch(&mut s, AutoStitch { along: StitchAlong::Seams, inset: 0.0 }).unwrap();
        assert!((3..=12).contains(&made.paths), "{} rows for 12 edges (rows that meet on one face are one path)", made.paths);
        // A closed cube has no open border.
        assert!(pen.auto_stitch(&mut s, AutoStitch { along: StitchAlong::Borders, inset: 1.0 }).is_err());
    }
    /// A disc painted into a layer's mask on a plane: one closed row of stitches around it.
    #[test]
    fn stitches_follow_the_edge_of_the_selected_layers_mask() {
        use subcraft_doc::MaskEffectKind;
        use subcraft_image::TiledImage;
        let mut s = Session::new(subcraft_mesh::primitives::plane(2.0, 4), 256);
        let ts = s.selected_texture_set().id;
        if !s.channels().contains(&ChannelKind::Height) {
            s.execute(Command::AddChannel { texture_set: ts, kind: ChannelKind::Height }).unwrap();
        }
        let mut pen = PenEditor::default();
        let tips = subcraft_assets::alphas::builtin();
        pen.style = Some(crate::stitch::style(&mut s, crate::stitch::find("Stitch: Straight").unwrap(), &tips));
        s.execute(Command::AddLayer(crate::NewLayer::Fill)).unwrap();
        let layer = s.selection().layer.unwrap();
        // Without a mask there is no edge to follow.
        assert!(pen.auto_stitch(&mut s, AutoStitch { along: StitchAlong::MaskEdge, inset: 0.0 }).is_err());
        s.execute(Command::AddMask { layer, white: false }).unwrap();
        let disc: Vec<f32> = (0..256 * 256).flat_map(|i| {
            let p = Vec2::new((i % 256) as f32 + 0.5, (i / 256) as f32 + 0.5) / 256.0;
            [1.0, f32::from(p.distance(Vec2::splat(0.5)) < 0.3)]
        }).collect();
        s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Paint { image: TiledImage::from_dense(256, 256, 2, [0.0; 4], &disc) } }).unwrap();
        s.execute(Command::SelectLayer(Some(layer))).unwrap();
        let made = pen.auto_stitch(&mut s, AutoStitch { along: StitchAlong::MaskEdge, inset: 0.0 }).unwrap();
        assert_eq!(made, AutoStitched { paths: 1, skipped: 0 });
        let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!("the new path layer is selected") };
        let path = p.path.as_ref().unwrap();
        assert!(path.closed && path.anchors.len() > 12);
        assert!(path.anchors.iter().all(|a| (a.position.distance(Vec2::splat(0.5)) - 0.3).abs() < 0.01), "the points lie on the disc's edge");
        // The stitches are painted along the edge and nowhere else.
        let height = &p.channels[&ChannelKind::Height];
        let (mut on, mut off) = (0, 0);
        for i in 0..256 * 256u32 {
            if height.get(i % 256, i / 256)[1] > 0.5 {
                let d = Vec2::new((i % 256) as f32 + 0.5, (i / 256) as f32 + 0.5) / 256.0;
                if (d.distance(Vec2::splat(0.5)) - 0.3).abs() < 0.03 { on += 1 } else { off += 1 }
            }
        }
        assert!(on > 150 && off == 0, "{on} texels on the edge, {off} away from it");
    }
}
