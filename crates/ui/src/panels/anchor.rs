//! Anchor point controls (ADR-0045): the anchor picker shared by fill channels, Anchor
//! Reference mask effects and generator inputs, reference warnings and an anchor's dependents.
//! Widgets only edit values; every change goes through the caller's app command.

use egui::{RichText, Ui};
use subcraft_core::ChannelKind;
use subcraft_doc::{AnchorDomain, AnchorIndex, AnchorInfo, AnchorInput, RefSite, RefStatus};

use crate::theme::{self, TEXT_DIM, TEXT_FAINT};
use crate::widgets::*;

/// Label of an anchor in a picker: its name and the layer it sits in.
pub fn label(a: &AnchorInfo) -> String {
    let domain = if a.domain == AnchorDomain::Mask { " (mask)" } else { "" };
    format!("{} · {}{domain}", a.name, a.layer_name)
}

/// The anchor a new reference starts with: the nearest of `candidates` below it.
pub fn default_input(candidates: &[&AnchorInfo], channel: ChannelKind) -> Option<AnchorInput> {
    candidates.last().map(|a| AnchorInput::new(a.id, channel))
}

/// Anchor picker for the reference at `site` (`input`: `None` = no anchor, offered only when
/// `none_label` is given). Content anchors also pick a channel from `channels`. Shows the
/// reference's problem, if any. Returns true when `input` changed.
pub fn picker(
    ui: &mut Ui,
    index: &AnchorIndex,
    site: RefSite,
    input: &mut Option<AnchorInput>,
    none_label: Option<&str>,
    channels: &[ChannelKind],
    default_channel: ChannelKind,
) -> bool {
    let mut changed = false;
    let candidates = index.candidates(site);
    let current = input.and_then(|i| index.anchor(i.anchor));
    let selected = match (input.as_ref(), current) {
        (None, _) => none_label.unwrap_or("Choose an anchor").to_string(),
        (Some(_), Some(a)) => label(a),
        (Some(_), None) => "Missing anchor".to_string(),
    };
    prop_row(ui, "Anchor", |ui| {
        egui::ComboBox::from_id_salt(("anchor_pick", format!("{site:?}"))).width(170.0).selected_text(selected).show_ui(ui, |ui| {
            if let Some(none) = none_label
                && ui.selectable_label(input.is_none(), none).clicked()
            {
                *input = None;
                changed = true;
            }
            for a in &candidates {
                let is = input.is_some_and(|i| i.anchor == a.id);
                if ui.selectable_label(is, label(a)).clicked() && !is {
                    let channel = input.map_or(default_channel, |i| i.channel);
                    *input = Some(AnchorInput::new(a.id, channel));
                    changed = true;
                }
            }
            if candidates.is_empty() {
                ui.label(RichText::new("No anchor point below. Add one to a lower layer's content or mask (Add › Anchor Point).").small().color(TEXT_FAINT));
            }
        });
    });
    if let Some(i) = input.as_mut()
        && current.is_some_and(|a| a.domain == AnchorDomain::Content)
    {
        prop_row(ui, "Channel", |ui| {
            egui::ComboBox::from_id_salt(("anchor_channel", format!("{site:?}"))).width(120.0).selected_text(i.channel.display_name()).show_ui(ui, |ui| {
                for k in channels {
                    changed |= ui.selectable_value(&mut i.channel, *k, k.display_name()).changed();
                }
            });
        });
    }
    if input.is_some() {
        match index.reference(site).map(|r| r.status) {
            Some(status) if !status.is_ok() => warning(ui, status),
            _ => {}
        }
    }
    changed
}

/// A reference problem, in the warning color.
pub fn warning(ui: &mut Ui, status: RefStatus) {
    if status.is_ok() {
        return;
    }
    ui.label(RichText::new(format!("{}  {}", crate::icons::WARNING, status.message())).font(theme::font(12.0)).color(theme::WARNING));
}

/// An anchor point's own properties: what it captures and who reads it.
pub fn anchor_info(ui: &mut Ui, index: &AnchorIndex, anchor: subcraft_core::EffectId, mask: bool) {
    hint(
        ui,
        if mask {
            "Captures the mask as it is at this point of the stack. Anchor Reference effects and generator inputs in layers above (or later in this mask) can read it."
        } else {
            "Captures this layer's content (every channel) as it is at this point of the stack, before its mask and blending. Fill channels, Anchor Reference mask effects and generator inputs in layers above can read it."
        },
    );
    let readers = index.dependents(anchor);
    if readers.is_empty() {
        ui.label(RichText::new("Not read by any layer yet.").small().color(TEXT_FAINT));
        return;
    }
    ui.label(RichText::new(format!("Read by {} input(s):", readers.len())).small().color(TEXT_DIM));
    for r in readers {
        let place = match r.site {
            RefSite::Fill { channel, .. } => format!("fill channel {}", channel.display_name()),
            RefSite::Mask { .. } => "mask effect".to_string(),
        };
        let who = index.layer_name(r.site.layer()).unwrap_or("a layer");
        let text = RichText::new(format!("• {who}: {place}")).small();
        ui.label(if r.status.is_ok() { text.color(TEXT_DIM) } else { text.color(theme::WARNING) })
            .on_hover_text(r.status.message());
    }
}
