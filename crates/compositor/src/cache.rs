//! Incremental compositing: diff the texture set against the last evaluated one and refresh only
//! the tiles that can have changed.
//!
//! * pointer-equal layers are skipped (copy-on-write: untouched subtrees are shared);
//! * layers whose parameters are unchanged ([`Layer::same_parameters`]) contribute the tiles that
//!   changed identity in their paint channels and mask base ([`TiledImage::changed_tiles`]);
//! * anything else (parameter, structure, channel list, resolution change) dirties every tile.
//!
//! The returned dirty list is what the renderer re-uploads.

use std::collections::BTreeSet;
use std::sync::Arc;

use subcraft_doc::{ContentEffectKind, Layer, LayerContent, MaskEffectKind, TextureSet, changed_paint_tiles};

use crate::{Composite, EvalContext};

#[derive(Default)]
pub struct CompositeCache {
    last: Option<TextureSet>,
    composite: Option<Composite>,
    force_all: bool,
}

/// Result of diffing two layer trees.
enum Dirty {
    All,
    Tiles(BTreeSet<usize>),
}

impl CompositeCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn composite(&self) -> Option<&Composite> {
        self.composite.as_ref()
    }

    /// Force a full re-evaluation on the next update (resources, mesh maps or geometry changed).
    pub fn invalidate(&mut self) {
        self.force_all = true;
    }

    /// Bring the composite up to date with `ts`; returns the re-evaluated tile indices (sorted).
    pub fn update(&mut self, ts: &TextureSet, ctx: &EvalContext) -> Vec<usize> {
        let reusable = !self.force_all
            && self.composite.as_ref().is_some_and(|c| c.resolution == ts.resolution)
            && self.last.as_ref().is_some_and(|l| l.id == ts.id && l.channels == ts.channels);
        let dirty = match (&self.last, reusable) {
            (Some(last), true) => diff_lists(&last.layers, &ts.layers),
            _ => Dirty::All,
        };
        let composite = match (&mut self.composite, reusable) {
            (Some(c), true) => c,
            (slot, _) => slot.insert(Composite::new(ts)),
        };
        let tiles: Vec<usize> = match dirty {
            Dirty::All => (0..composite.tile_count()).collect(),
            Dirty::Tiles(set) => set.into_iter().collect(),
        };
        if !tiles.is_empty() {
            composite.evaluate_tiles(ts, ctx, &tiles);
        }
        self.last = Some(ts.clone());
        self.force_all = false;
        tiles
    }
}

fn diff_lists(old: &[Arc<Layer>], new: &[Arc<Layer>]) -> Dirty {
    let mut tiles = BTreeSet::new();
    if collect_list(old, new, &mut tiles) { Dirty::Tiles(tiles) } else { Dirty::All }
}

/// Adds raster-changed tiles to `out`; returns false if the change is not raster-only.
fn collect_list(old: &[Arc<Layer>], new: &[Arc<Layer>], out: &mut BTreeSet<usize>) -> bool {
    if old.len() != new.len() {
        return false;
    }
    for (a, b) in old.iter().zip(new) {
        if Arc::ptr_eq(a, b) {
            continue;
        }
        if !a.same_parameters(b) {
            return false;
        }
        if let (Some(ma), Some(mb)) = (&a.mask, &b.mask) {
            out.extend(ma.base.changed_tiles(&mb.base));
            // Same parameters, so the same effect list: only paint effects differ, in texels.
            for (ea, eb) in ma.effects.iter().zip(&mb.effects).filter(|(x, y)| !Arc::ptr_eq(x, y)) {
                if let (MaskEffectKind::Paint { image: ia }, MaskEffectKind::Paint { image: ib }) = (&ea.kind, &eb.kind) {
                    out.extend(ia.changed_tiles(ib));
                }
            }
        }
        for (ea, eb) in a.content_effects.iter().zip(&b.content_effects).filter(|(x, y)| !Arc::ptr_eq(x, y)) {
            if let (ContentEffectKind::Paint { channels: ca }, ContentEffectKind::Paint { channels: cb }) = (&ea.kind, &eb.kind) {
                changed_paint_tiles(ca, cb, out);
            }
        }
        match (&a.content, &b.content) {
            (LayerContent::Paint(pa), LayerContent::Paint(pb)) => changed_paint_tiles(&pa.channels, &pb.channels, out),
            (LayerContent::Group(ga), LayerContent::Group(gb)) if !collect_list(&ga.children, &gb.children, out) => return false,
            _ => {}
        }
    }
    true
}
