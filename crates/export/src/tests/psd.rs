use photocraft_psd::layer::{ChannelData, LayerRecord};
use photocraft_psd::tagged::TaggedBlock;
use photocraft_psd::{Compression, PsdFile};
use subcraft_core::color::srgb_to_linear;
use subcraft_doc::{Layer, LayerContent, Mask, TextureSet};

use super::*;

const RES: u32 = 128;

/// Bottom paint layer with a few dark base-color texels and roughness, a fill layer, and a
/// folder holding a masked paint layer.
fn set() -> TextureSet {
    let mut ts = TextureSet::new("Body", 0, RES);
    let mut base = Layer::new_paint("Base", &ts);
    if let LayerContent::Paint(p) = &mut base.content {
        let img = p.channel_mut(ChannelKind::BaseColor, RES);
        img.set(3, 4, [0.002, 0.25, 0.9, 1.0]);
        img.set(100, 90, [0.6, 0.01, 0.3, 0.5]);
        p.channel_mut(ChannelKind::Roughness, RES).set(5, 5, [0.37, 1.0, 0.0, 0.0]);
        p.channel_mut(ChannelKind::Height, RES).set(6, 6, [-0.4, 1.0, 0.0, 0.0]);
    }
    let fill = Layer::new_fill("Fill", &ts);
    let mut folder = Layer::new_folder("Folder", &ts);
    let mut inner = Layer::new_paint("Inner", &ts);
    if let LayerContent::Paint(p) = &mut inner.content {
        p.channel_mut(ChannelKind::BaseColor, RES).set(70, 10, [0.1, 0.2, 0.3, 1.0]);
    }
    let mut mask = Mask::new(RES, true);
    mask.base.set(80, 80, [0.25, 0.0, 0.0, 0.0]);
    mask.base.set(10, 100, [0.4, 0.0, 0.0, 0.0]);
    inner.mask = Some(mask);
    if let LayerContent::Group(g) = &mut folder.content {
        g.children.push(Arc::new(inner));
    }
    ts.layers = vec![Arc::new(base), Arc::new(fill), Arc::new(folder)];
    ts
}

fn by_name<'a>(file: &'a PsdFile, name: &str) -> (usize, &'a LayerRecord) {
    file.layers().iter().enumerate().find(|(_, r)| r.name() == name).unwrap_or_else(|| panic!("no PSD layer {name}"))
}

fn by_layer_name<'a>(layers: &'a [Arc<Layer>], name: &str) -> &'a Layer {
    fn rec<'a>(list: &'a [Arc<Layer>], name: &str) -> Option<&'a Layer> {
        list.iter().find_map(|l| if l.name == name { Some(&**l) } else { rec(l.children(), name) })
    }
    rec(layers, name).unwrap_or_else(|| panic!("no layer {name}"))
}

fn image(layer: &Layer, kind: ChannelKind) -> &TiledImage {
    match &layer.content { LayerContent::Paint(p) => &p.channels[&kind], _ => panic!("not paint") }
}

/// Replaces one 16-bit plane of a record (full layer rectangle) with `f(x, y, old)`.
fn edit_plane(file: &mut PsdFile, index: usize, id: i16, f: impl Fn(i64, i64, u16) -> u16) {
    let (depth, version) = (file.header.depth, file.header.version);
    let rec = &mut file.layers_mut()[index];
    let rect = rec.channel_rect(id);
    let (w, h) = rect.size().unwrap();
    let old: Vec<u16> = rec.decode_channel(id, depth, version).unwrap().as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
    let (l, t) = (i64::from(rect.left), i64::from(rect.top));
    let new: Vec<u8> = old.iter().enumerate().flat_map(|(i, &v)| f(l + (i % w) as i64, t + (i / w) as i64, v).to_be_bytes()).collect();
    let slot = rec.channels.iter_mut().find(|c| c.id == id).unwrap();
    *slot = ChannelData::encode(id, Compression::Raw, &new, w, h, depth, version).unwrap();
}

#[test]
fn only_folders_and_paint_layers_are_written_with_their_hierarchy() {
    let ts = set();
    let out = write_channel(&ts, ChannelKind::BaseColor).unwrap();
    let file = PsdFile::from_bytes(&out.bytes).unwrap();
    assert_eq!((file.header.width, file.header.height, file.header.depth), (RES, RES, 16));
    let tree = file.layer_tree();
    let names: Vec<String> = tree.iter().map(|n| file.layers()[n.index()].name()).collect();
    assert_eq!(names, ["Base", "Folder"], "the fill layer is not pixels and stays out");
    assert_eq!(file.layers()[tree[1].children()[0].index()].name(), "Inner");
    assert!(file.layer(by_name(&file, "Inner").0).unwrap().user_mask().is_some());
    assert_eq!(out.baseline.layers.len(), 3);
    // Each written record's lyid maps back to its SubCraft layer.
    for (id, b) in &out.baseline.layers {
        let rec = file.layers().iter().find(|r| r.layer_id() == Some(*id)).unwrap();
        assert_eq!(rec.name(), b.name);
        assert!(ts.find(b.layer).is_some());
    }
}

#[test]
fn colors_are_srgb_and_data_channels_raw_with_height_remapped() {
    let ts = set();
    let pixel = |kind, name: &str, x: i64, y: i64, c: i16| {
        let file = PsdFile::from_bytes(&write_channel(&ts, kind).unwrap().bytes).unwrap();
        let rec = by_name(&file, name).1;
        let w = rec.rect.size().unwrap().0 as i64;
        let i = ((y - i64::from(rec.rect.top)) * w + (x - i64::from(rec.rect.left))) as usize;
        let b = rec.decode_channel(c, 16, file.header.version).unwrap();
        u16::from_be_bytes([b[2 * i], b[2 * i + 1]])
    };
    // Expected values start from what the f16 tiles actually hold.
    let base = by_layer_name(&ts.layers, "Base");
    let stored = |kind, x, y| image(base, kind).get(x, y)[0];
    assert_eq!(pixel(ChannelKind::BaseColor, "Base", 3, 4, 1), (linear_to_srgb(0.25) * 65535.0).round() as u16);
    assert_eq!(pixel(ChannelKind::Roughness, "Base", 5, 5, 0), (stored(ChannelKind::Roughness, 5, 5) * 65535.0).round() as u16, "data channels are not color-transformed");
    assert_eq!(pixel(ChannelKind::Height, "Base", 6, 6, 0), ((stored(ChannelKind::Height, 6, 6) * 0.5 + 0.5) * 65535.0).round() as u16, "height -0.4 → 0.3");
    assert_eq!(pixel(ChannelKind::BaseColor, "Base", 100, 90, -1), (0.5f32 * 65535.0).round() as u16);
}

#[test]
fn an_unedited_file_changes_nothing_and_keeps_texels_bit_exact() {
    let ts = set();
    for kind in [ChannelKind::BaseColor, ChannelKind::Roughness, ChannelKind::Height] {
        let out = write_channel(&ts, kind).unwrap();
        let edit = apply_channel(&ts, kind, &out.bytes, &out.baseline).unwrap();
        assert!(edit.report.is_empty(), "{kind:?}: {:?}", edit.report);
        assert_eq!(edit.report.missing, 0);
        assert_eq!(edit.layers, ts.layers, "{kind:?}: dark linear values do not survive u16, so nothing may be rewritten");
        assert_eq!(edit.baseline, out.baseline);
    }
}

#[test]
fn an_edited_texel_rewrites_only_its_tile() {
    let ts = set();
    let out = write_channel(&ts, ChannelKind::BaseColor).unwrap();
    let mut file = PsdFile::from_bytes(&out.bytes).unwrap();
    let index = by_name(&file, "Base").0;
    let target = (linear_to_srgb(0.5) * 65535.0).round() as u16;
    edit_plane(&mut file, index, 0, |x, y, v| if (x, y) == (100, 90) { target } else { v });
    let edit = apply_channel(&ts, ChannelKind::BaseColor, &file.to_bytes().unwrap(), &out.baseline).unwrap();
    assert_eq!(edit.report.changed_tiles, 1);
    let before = image(by_layer_name(&ts.layers, "Base"), ChannelKind::BaseColor);
    let after = image(by_layer_name(&edit.layers, "Base"), ChannelKind::BaseColor);
    let p = after.get(100, 90);
    assert!((p[0] - srgb_to_linear(target as f32 / 65535.0)).abs() < 1e-3, "{p:?}");
    assert!((p[3] - 0.5).abs() < 1e-3);
    // The dark texel lives in another tile and keeps its exact f16 value.
    assert_eq!(after.get(3, 4), before.get(3, 4));
    assert_eq!(after.changed_tiles(before), vec![before.tile_index(100 / TILE_SIZE, 90 / TILE_SIZE)]);
    // Applying the same file again is a no-op against the updated baseline.
    let mut next = ts.clone();
    next.layers = edit.layers.clone();
    let again = apply_channel(&next, ChannelKind::BaseColor, &file.to_bytes().unwrap(), &edit.baseline).unwrap();
    assert!(again.report.is_empty(), "{:?}", again.report);
}

#[test]
fn transparent_color_rewritten_by_the_editor_is_not_a_change() {
    let ts = set();
    let out = write_channel(&ts, ChannelKind::BaseColor).unwrap();
    let mut file = PsdFile::from_bytes(&out.bytes).unwrap();
    let (index, rec) = by_name(&file, "Base");
    let (w, h) = rec.rect.size().unwrap();
    let alpha: Vec<u16> = rec.decode_channel(-1, 16, file.header.version).unwrap().as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
    assert_eq!(alpha.len(), w * h);
    let (l, t) = (i64::from(rec.rect.left), i64::from(rec.rect.top));
    edit_plane(&mut file, index, 2, |x, y, v| if alpha[((y - t) as usize) * w + (x - l) as usize] == 0 { 0 } else { v });
    let edit = apply_channel(&ts, ChannelKind::BaseColor, &file.to_bytes().unwrap(), &out.baseline).unwrap();
    assert!(edit.report.is_empty(), "{:?}", edit.report);
}

#[test]
fn names_visibility_and_masks_come_back_and_untouched_properties_do_not() {
    let ts = set();
    let out = write_channel(&ts, ChannelKind::BaseColor).unwrap();
    let mut file = PsdFile::from_bytes(&out.bytes).unwrap();
    let index = by_name(&file, "Inner").0;
    {
        let rec = &mut file.layers_mut()[index];
        rec.flags.set_hidden(true);
        rec.blocks.retain(|b| &b.key != b"luni");
        rec.blocks.push(TaggedBlock::unicode_name("Inner renamed"));
        rec.name = b"Inner renamed".to_vec();
    }
    edit_plane(&mut file, index, -2, |x, y, v| if (x, y) == (70, 70) { 0 } else { v });
    let edit = apply_channel(&ts, ChannelKind::BaseColor, &file.to_bytes().unwrap(), &out.baseline).unwrap();
    assert_eq!((edit.report.renamed, edit.report.visibility, edit.report.changed_tiles), (1, 1, 1));
    let inner = by_layer_name(&edit.layers, "Inner renamed");
    assert!(!inner.visible);
    let mask = &inner.mask.as_ref().unwrap().base;
    assert_eq!(mask.get(70, 70)[0], 0.0);
    assert_eq!(mask.get(10, 100)[0], ts.find(inner.id).unwrap().mask.as_ref().unwrap().base.get(10, 100)[0], "other mask tiles stay exact");
    // A rename made in SubCraft after the export is not undone by the unchanged file name.
    let mut renamed = ts.clone();
    renamed.find_mut(by_layer_name(&ts.layers, "Base").id).unwrap().name = "Renamed in SubCraft".into();
    let edit = apply_channel(&renamed, ChannelKind::BaseColor, &out.bytes, &out.baseline).unwrap();
    assert!(edit.report.is_empty());
    by_layer_name(&edit.layers, "Renamed in SubCraft");
}

#[test]
fn a_layer_added_in_the_editor_becomes_a_paint_layer_above_its_neighbor() {
    let ts = set();
    let out = write_channel(&ts, ChannelKind::BaseColor).unwrap();
    let mut file = PsdFile::from_bytes(&out.bytes).unwrap();
    let (index, rec) = by_name(&file, "Inner");
    let mut copy = rec.clone();
    copy.blocks.retain(|b| &b.key != b"lyid" && &b.key != b"luni");
    copy.blocks.push(TaggedBlock::layer_id(999));
    copy.blocks.push(TaggedBlock::unicode_name("Scratches"));
    file.layers_mut().insert(index + 1, copy);
    edit_plane(&mut file, index + 1, 0, |_, _, _| 40000);
    edit_plane(&mut file, index + 1, -1, |x, _, _| if x < 96 { 65535 } else { 0 });
    let edit = apply_channel(&ts, ChannelKind::BaseColor, &file.to_bytes().unwrap(), &out.baseline).unwrap();
    assert_eq!(edit.report.added.len(), 1);
    let folder = by_layer_name(&edit.layers, "Folder");
    let names: Vec<&str> = folder.children().iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Inner", "Scratches"]);
    let added = by_layer_name(&edit.layers, "Scratches");
    let LayerContent::Paint(p) = &added.content else { panic!() };
    assert_eq!(p.channels.keys().copied().collect::<Vec<_>>(), [ChannelKind::BaseColor], "paints only the edited channel");
    let r = &p.channels[&ChannelKind::BaseColor];
    let v = r.get(70, 10);
    assert!((v[0] - srgb_to_linear(40000.0 / 65535.0)).abs() < 1e-3 && v[3] == 1.0, "{v:?}");
    assert_eq!(r.get(100, 10)[3], 0.0);
    assert_eq!(edit.baseline.layers[&999].layer, added.id);
    // Saving again does not add it twice.
    let mut next = ts.clone();
    next.layers = edit.layers.clone();
    let again = apply_channel(&next, ChannelKind::BaseColor, &file.to_bytes().unwrap(), &edit.baseline).unwrap();
    assert!(again.report.is_empty(), "{:?}", again.report);
}

#[test]
fn deleted_layers_are_reported_not_removed_and_a_resized_canvas_is_refused() {
    let ts = set();
    let out = write_channel(&ts, ChannelKind::BaseColor).unwrap();
    let mut file = PsdFile::from_bytes(&out.bytes).unwrap();
    let index = by_name(&file, "Base").0;
    file.layers_mut().remove(index);
    let edit = apply_channel(&ts, ChannelKind::BaseColor, &file.to_bytes().unwrap(), &out.baseline).unwrap();
    assert_eq!(edit.report.missing, 1);
    assert_eq!(edit.layers, ts.layers);
    let small = TextureSet::new("Body", 0, RES / 2);
    assert!(apply_channel(&small, ChannelKind::BaseColor, &out.bytes, &out.baseline).is_err());
    assert!(apply_channel(&ts, ChannelKind::Roughness, &out.bytes, &out.baseline).is_err(), "RGB file for a gray channel");
}
