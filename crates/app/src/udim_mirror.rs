//! Painter's UDIM workflow, part 2 (issue #581 follow-up): with "paint across UDIM tiles" on, a
//! material dropped on the model lands on every tile of the group as matching layers. Edits of
//! such a layer (fill channels, projection, mask and content effects) are repeated on the matching
//! layer of every other tile, so one material controls the whole object instead of one copy per
//! tile having to be adjusted by hand.
//!
//! [`partner_command`] translates one command on the source tile's layer into the same command
//! on a partner tile's layer: the layer id is swapped and every effect id is mapped by its
//! position in the stack (each tile's copy carries its own ids). Painted rasters never cross
//! tiles: they belong to each tile's own UVs.

use subcraft_core::{EffectId, LayerId};
use subcraft_doc::{ContentEffectKind, Layer, MaskEffectKind};

use crate::command::Command;
use crate::session::LayerTarget;

/// The layer a command edits, when it is one that is repeated across UDIM tiles.
pub(crate) fn mirrored_layer(cmd: &Command) -> Option<LayerId> {
    Some(match cmd {
        Command::SetFillChannel { layer, .. }
        | Command::ToggleFillChannel { layer, .. }
        | Command::AddFillChannel { layer, .. }
        | Command::SetFillProjection { layer, .. }
        | Command::SetBlendAllChannels { layer, .. }
        | Command::SetMaskEnabled { layer, .. }
        | Command::AddMaskEffect { layer, .. }
        | Command::RemoveMaskEffect { layer, .. }
        | Command::SetMaskEffect { layer, .. }
        | Command::SetIdColors { layer, .. }
        | Command::MoveMaskEffect { layer, .. }
        | Command::DuplicateMaskEffect { layer, .. }
        | Command::ApplyMaskEffects { layer, .. }
        | Command::InsertMaskEffects { layer, .. }
        | Command::ReplaceMaskEffects { layer, .. }
        | Command::AddContentEffect { layer, .. }
        | Command::InsertContentEffect { layer, .. }
        | Command::RemoveContentEffect { layer, .. }
        | Command::SetContentEffect { layer, .. }
        | Command::MoveContentEffect { layer, .. }
        | Command::DuplicateContentEffect { layer, .. }
        | Command::RemoveEffects { layer, .. }
        | Command::DuplicateEffects { layer, .. }
        | Command::NudgeEffects { layer, .. }
        | Command::RenameEffect { layer, .. } => *layer,
        Command::InvertMask(layer) => *layer,
        _ => return None,
    })
}

/// A stack's effects as (id, kind tag), bottom to top: the tag keeps a mapping from pairing two
/// different kinds of effect.
fn stack(layer: &Layer, target: LayerTarget) -> Vec<(EffectId, u8)> {
    match target {
        LayerTarget::Mask => layer.mask.as_ref().map_or_else(Vec::new, |m| m.effects.iter().map(|e| (e.id, mask_tag(&e.kind))).collect()),
        LayerTarget::Content => layer.content_effects.iter().map(|e| (e.id, content_tag(&e.kind))).collect(),
    }
}

fn mask_tag(k: &MaskEffectKind) -> u8 {
    match k {
        MaskEffectKind::Paint { .. } => 0,
        MaskEffectKind::Fill { .. } => 1,
        MaskEffectKind::Generator(_) => 2,
        MaskEffectKind::Levels(_) => 3,
        MaskEffectKind::Filter(_) => 4,
        MaskEffectKind::Invert => 5,
        MaskEffectKind::Image { .. } => 6,
        MaskEffectKind::IdColor(_) => 7,
        MaskEffectKind::Compare(_) => 8,
        MaskEffectKind::Anchor => 9,
        MaskEffectKind::AnchorRef { .. } => 10,
    }
}

fn content_tag(k: &ContentEffectKind) -> u8 {
    match k {
        ContentEffectKind::Paint { .. } => 0,
        ContentEffectKind::Fill(_) => 1,
        ContentEffectKind::Levels { .. } => 2,
        ContentEffectKind::Filter { .. } => 3,
        ContentEffectKind::Anchor => 4,
        ContentEffectKind::Generator(_) => 5,
        ContentEffectKind::Material(_) => 6,
    }
}

/// The partner's effect at the same position of the same stack, when it is the same kind.
fn map_effect(source: &Layer, partner: &Layer, target: LayerTarget, id: EffectId) -> Option<EffectId> {
    let (s, p) = (stack(source, target), stack(partner, target));
    let i = s.iter().position(|e| e.0 == id)?;
    p.get(i).filter(|e| e.1 == s[i].1).map(|e| e.0)
}

fn map_effects(source: &Layer, partner: &Layer, target: LayerTarget, ids: &[EffectId]) -> Option<Vec<EffectId>> {
    ids.iter().map(|&id| map_effect(source, partner, target, id)).collect()
}

/// Where an effect added on the source (above `selected`, else on top) goes on the partner, as
/// the `above` of an insert command (`None` = bottom).
fn insert_above(source: &Layer, partner: &Layer, target: LayerTarget, selected: Option<EffectId>) -> Option<Option<EffectId>> {
    match selected.filter(|s| stack(source, target).iter().any(|e| e.0 == *s)) {
        Some(s) => Some(Some(map_effect(source, partner, target, s)?)),
        None => Some(stack(partner, target).last().map(|e| e.0)),
    }
}

/// A raster-holding mask effect gets a blank raster of the partner's resolution.
fn retarget_mask_kind(kind: &MaskEffectKind, resolution: u32) -> MaskEffectKind {
    match kind {
        MaskEffectKind::Paint { image } => {
            MaskEffectKind::Paint { image: subcraft_image::TiledImage::new(resolution, resolution, image.comps(), image.fill()) }
        }
        k => k.clone(),
    }
}

fn retarget_content_kind(kind: &ContentEffectKind, resolution: u32) -> ContentEffectKind {
    match kind {
        ContentEffectKind::Paint { channels } => ContentEffectKind::Paint {
            channels: channels.iter().map(|(&c, img)| (c, subcraft_image::TiledImage::new(resolution, resolution, img.comps(), img.fill()))).collect(),
        },
        k => k.clone(),
    }
}

/// `cmd` (on `source`, a layer of the source tile) as the same edit of `partner`, the matching
/// layer of another tile at `resolution`. `selected` is the source's selected effect per stack
/// (where an added effect goes). `None` when the partner's layer does not line up (an effect
/// missing or of another kind, no mask): that tile is left as it is.
pub(crate) fn partner_command(cmd: &Command, source: &Layer, partner: &Layer, resolution: u32, selected_mask: Option<EffectId>, selected_content: Option<EffectId>) -> Option<Command> {
    let l = partner.id;
    let has_mask = partner.mask.is_some();
    let m = |id| map_effect(source, partner, LayerTarget::Mask, id);
    let c = |id| map_effect(source, partner, LayerTarget::Content, id);
    Some(match cmd {
        Command::SetFillChannel { channel, fill, .. } => Command::SetFillChannel { layer: l, channel: *channel, fill: fill.clone() },
        Command::ToggleFillChannel { effect, channel, solo, .. } => {
            let effect = match effect { Some(e) => Some(c(*e)?), None => None };
            Command::ToggleFillChannel { layer: l, effect, channel: *channel, solo: *solo }
        }
        Command::AddFillChannel { channel, .. } => Command::AddFillChannel { layer: l, channel: *channel },
        Command::SetFillProjection { projection, .. } => Command::SetFillProjection { layer: l, projection: projection.clone() },
        Command::SetBlendAllChannels { from, mode, opacity, .. } => Command::SetBlendAllChannels { layer: l, from: *from, mode: *mode, opacity: *opacity },
        Command::SetMaskEnabled { enabled, .. } if has_mask => Command::SetMaskEnabled { layer: l, enabled: *enabled },
        Command::InvertMask(_) if has_mask => Command::InvertMask(l),
        Command::AddMaskEffect { kind, .. } if has_mask => Command::InsertMaskEffects {
            layer: l,
            effects: vec![retarget_mask_kind(kind, resolution)],
            above: insert_above(source, partner, LayerTarget::Mask, selected_mask)?,
            label: "Add Mask Effect".into(),
        },
        Command::InsertMaskEffects { effects, above, label, .. } if has_mask => Command::InsertMaskEffects {
            layer: l,
            effects: effects.iter().map(|k| retarget_mask_kind(k, resolution)).collect(),
            above: match above { Some(a) => Some(m(*a)?), None => None },
            label: label.clone(),
        },
        Command::ApplyMaskEffects { effects, label, .. } => {
            Command::ApplyMaskEffects { layer: l, effects: effects.iter().map(|k| retarget_mask_kind(k, resolution)).collect(), label: label.clone() }
        }
        Command::ReplaceMaskEffects { effects, label, .. } => {
            Command::ReplaceMaskEffects { layer: l, effects: effects.iter().map(|k| retarget_mask_kind(k, resolution)).collect(), label: label.clone() }
        }
        Command::RemoveMaskEffect { effect, .. } => Command::RemoveMaskEffect { layer: l, effect: m(*effect)? },
        Command::SetMaskEffect { effect, .. } => {
            let id = m(effect.id)?;
            let mut e = effect.clone();
            e.id = id;
            // The partner keeps what was painted into its own Paint effect.
            if let MaskEffectKind::Paint { .. } = e.kind {
                let own = partner.mask.as_ref()?.effects.iter().find(|x| x.id == id)?;
                e.kind = own.kind.clone();
            }
            Command::SetMaskEffect { layer: l, effect: e }
        }
        Command::SetIdColors { effect, colors, .. } => Command::SetIdColors { layer: l, effect: m(*effect)?, colors: colors.clone() },
        Command::MoveMaskEffect { effect, index, .. } => Command::MoveMaskEffect { layer: l, effect: m(*effect)?, index: *index },
        Command::DuplicateMaskEffect { effect, .. } => Command::DuplicateMaskEffect { layer: l, effect: m(*effect)? },
        Command::AddContentEffect { kind, .. } => Command::InsertContentEffect {
            layer: l,
            kind: retarget_content_kind(kind, resolution),
            above: insert_above(source, partner, LayerTarget::Content, selected_content)?,
        },
        Command::InsertContentEffect { kind, above, .. } => Command::InsertContentEffect {
            layer: l,
            kind: retarget_content_kind(kind, resolution),
            above: match above { Some(a) => Some(c(*a)?), None => None },
        },
        Command::RemoveContentEffect { effect, .. } => Command::RemoveContentEffect { layer: l, effect: c(*effect)? },
        Command::SetContentEffect { effect, .. } => {
            let id = c(effect.id)?;
            let mut e = effect.clone();
            e.id = id;
            if let ContentEffectKind::Paint { .. } = e.kind {
                let own = partner.content_effects.iter().find(|x| x.id == id)?;
                e.kind = own.kind.clone();
            }
            Command::SetContentEffect { layer: l, effect: e }
        }
        Command::MoveContentEffect { effect, index, .. } => Command::MoveContentEffect { layer: l, effect: c(*effect)?, index: *index },
        Command::DuplicateContentEffect { effect, .. } => Command::DuplicateContentEffect { layer: l, effect: c(*effect)? },
        Command::RemoveEffects { target, effects, .. } => Command::RemoveEffects { layer: l, target: *target, effects: map_effects(source, partner, *target, effects)? },
        Command::DuplicateEffects { target, effects, .. } => Command::DuplicateEffects { layer: l, target: *target, effects: map_effects(source, partner, *target, effects)? },
        Command::NudgeEffects { target, effects, up, .. } => Command::NudgeEffects { layer: l, target: *target, effects: map_effects(source, partner, *target, effects)?, up: *up },
        Command::RenameEffect { target, effect, name, .. } => Command::RenameEffect { layer: l, target: *target, effect: map_effect(source, partner, *target, *effect)?, name: name.clone() },
        _ => return None,
    })
}
