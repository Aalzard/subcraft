use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::{ChannelKind as C, ChannelValue};
use subcraft_doc::{ContentEffectKind, FillChannel, FillSource, Filter, MaskEffectKind};

fn color(s: &mut Session) -> [f32; 4] {
    s.update_composites(); s.composite(s.selection().texture_set).unwrap().channel(C::BaseColor).unwrap().get(32,32)
}

#[test]
fn filter_edits_reorder_duplicate_undo_and_reopen_without_rewriting_layer_data() {
    let mut s = Session::new(subcraft_mesh::primitives::plane(2.0,1),128);
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap(); let layer=s.selection().layer.unwrap();
    s.execute(Command::SetFillChannel {layer,channel:C::BaseColor,fill:FillChannel::new(FillSource::uniform(ChannelValue::rgb(1.0,0.0,0.0)))}).unwrap();
    s.execute(Command::Seal).unwrap(); let before=s.document().clone();
    s.execute(Command::AddContentEffect {layer,kind:ContentEffectKind::Filter {channel:C::BaseColor,
        filter:Filter::Hsl {hue:120.0,saturation:1.0,lightness:0.0}}}).unwrap();
    assert!(color(&mut s)[1]>0.99);
    s.execute(Command::Undo).unwrap(); assert_eq!(s.document(),&before);
    s.execute(Command::Redo).unwrap();
    let effect=s.selected_layer().unwrap().content_effects[0].id;
    s.execute(Command::DuplicateContentEffect {layer,effect}).unwrap();
    assert!(color(&mut s)[2]>0.99, "two 120-degree rotations give blue");
    s.execute(Command::RemoveContentEffect {layer,effect}).unwrap();
    s.execute(Command::AddMask {layer,white:false}).unwrap();
    s.execute(Command::AddMaskEffect {layer,kind:MaskEffectKind::Fill {value:0.4,procedural:None,projection:Default::default()}}).unwrap();
    s.execute(Command::AddMaskEffect {layer,kind:MaskEffectKind::Filter(Filter::Threshold {cutoff:0.3})}).unwrap();
    assert!(color(&mut s)[1]>0.99);
    let mask_effect=s.selected_effect().unwrap();
    s.execute(Command::MoveMaskEffect {layer,effect:mask_effect,index:0}).unwrap();
    let mixed=color(&mut s); assert!((mixed[1]-0.7).abs()<0.002);
    s.execute(Command::MoveMaskEffect {layer,effect:mask_effect,index:1}).unwrap();
    s.execute(Command::Seal).unwrap(); let original=s.document().clone();
    let mut changed=s.selected_layer().unwrap().mask.as_ref().unwrap().effects[1].as_ref().clone();
    for cutoff in [0.1,0.5,0.8] {
        changed.kind=MaskEffectKind::Filter(Filter::Threshold {cutoff});
        s.execute(Command::SetMaskEffect {layer,effect:changed.clone()}).unwrap();
        s.update_composites_within(Some(std::time::Duration::from_millis(2)));
    }
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(10);
    while s.composites_pending() {
        assert!(std::time::Instant::now()<deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
        s.update_composites_within(Some(std::time::Duration::from_millis(2)));
    }
    let latest=s.composite(s.selection().texture_set).unwrap().channel(C::BaseColor).unwrap().get(32,32);
    assert_eq!(latest,[0.5,0.5,0.5,1.0], "latest threshold wins in the worker");
    s.execute(Command::Undo).unwrap(); assert_eq!(s.document(),&original, "a parameter gesture is one undo step");
    // Export must drain the newest filter edit even if its worker preview is not published yet.
    let mut content=s.selected_layer().unwrap().content_effects[0].as_ref().clone();
    content.kind=ContentEffectKind::Filter {channel:C::BaseColor,filter:Filter::Hsl {hue:-120.0,saturation:1.0,lightness:0.0}};
    s.execute(Command::SetContentEffect {layer,effect:content}).unwrap();
    s.update_composites_within(Some(std::time::Duration::from_millis(2)));
    let archive=s.export_archive(&subcraft_export::ExportSettings::default()).unwrap();
    let mut zip=zip::ZipArchive::new(std::io::Cursor::new(archive.bytes)).unwrap();
    let name=zip.file_names().find_map(|n|n.ok().filter(|n|n.contains("BaseColor")).map(|n|n.into_owned())).unwrap();
    let mut bytes=Vec::new();std::io::Read::read_to_end(&mut zip.by_name(&name).unwrap(),&mut bytes).unwrap();
    let pixel=subcraft_image::io::decode_bitmap(&bytes,Some(&name)).unwrap().texel(32,32,false);
    assert!(pixel[2]>0.99 && pixel[0]<0.01 && pixel[1]<0.01,"export must contain the newest blue filter: {pixel:?}");
    let expected=color(&mut s); let path=std::env::temp_dir().join(format!("subcraft-filters-{}.subcraft",layer.0));
    s.save(Some(&path)).unwrap(); let mut reopened=Session::open_project(&path).unwrap();
    assert_eq!(color(&mut reopened),expected); assert_eq!(reopened.document(),s.document());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn invalid_filters_and_wrong_domains_leave_document_and_history_unchanged() {
    let mut s=Session::new(subcraft_mesh::primitives::cube(2.0),64); let layer=s.selection().layer.unwrap();
    s.execute(Command::AddMask {layer,white:false}).unwrap(); s.execute(Command::Seal).unwrap();
    let before=s.document().clone(); let entries=s.history().entries().len();
    for (channel,filter) in [(C::Normal,Filter::Threshold {cutoff:0.5}),
        (C::Roughness,Filter::Hsl {hue:90.0,saturation:1.0,lightness:0.0}),
        (C::BaseColor,Filter::Clamp {low:0.8,high:0.2}), (C::BaseColor,Filter::Blur {radius:33.0}), (C::Height,Filter::Sharpen {radius:2.0,amount:-1.0})] {
        assert!(s.execute(Command::AddContentEffect {layer,kind:ContentEffectKind::Filter {channel,filter}}).is_err());
    }
    assert!(s.execute(Command::AddMaskEffect {layer,kind:MaskEffectKind::Filter(Filter::ALL[1])}).is_err());
    assert_eq!(s.document(),&before); assert_eq!(s.history().entries().len(),entries);
}

#[test]
fn neighborhood_mask_edits_latest_worker_export_undo_and_save_are_exact() {
    let mut s=Session::new(subcraft_mesh::primitives::plane(2.0,1),128);
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();let layer=s.selection().layer.unwrap();
    s.execute(Command::SetFillChannel {layer,channel:C::BaseColor,fill:FillChannel::new(FillSource::uniform(ChannelValue::rgb(1.0,1.0,1.0)))}).unwrap();
    s.execute(Command::AddMask {layer,white:false}).unwrap();
    let mut image=subcraft_image::TiledImage::new(128,128,2,[0.0;4]);
    for y in 0..128 {for x in 0..64 {image.set(x,y,[1.0,1.0,0.0,0.0]);}}
    s.execute(Command::AddMaskEffect {layer,kind:MaskEffectKind::Paint {image}}).unwrap();
    s.execute(Command::AddMaskEffect {layer,kind:MaskEffectKind::Filter(Filter::Blur {radius:2.0})}).unwrap();s.execute(Command::Seal).unwrap();
    let original=s.document().clone();let mut effect=s.selected_layer().unwrap().mask.as_ref().unwrap().effects[1].as_ref().clone();
    for radius in [4.0,16.0,0.0] {
        effect.kind=MaskEffectKind::Filter(Filter::Blur {radius});s.execute(Command::SetMaskEffect {layer,effect:effect.clone()}).unwrap();
        s.update_composites_within(Some(std::time::Duration::from_millis(1)));
    }
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(10);
    while s.composites_pending() {assert!(std::time::Instant::now()<deadline);std::thread::sleep(std::time::Duration::from_millis(1));s.update_composites_within(Some(std::time::Duration::from_millis(1)));}
    assert_eq!(s.composite(s.selection().texture_set).unwrap().channel(C::BaseColor).unwrap().get(64,63),[0.5,0.5,0.5,1.0]);
    s.execute(Command::Undo).unwrap();assert_eq!(s.document(),&original);
    effect.kind=MaskEffectKind::Filter(Filter::Blur {radius:8.0});s.execute(Command::SetMaskEffect {layer,effect}).unwrap();s.update_composites_within(Some(std::time::Duration::from_millis(1)));
    let archive=s.export_archive(&subcraft_export::ExportSettings::default()).unwrap();
    let expected=s.composite(s.selection().texture_set).unwrap().channel(C::BaseColor).unwrap().get(64,63);
    assert!(expected[0]>0.6 && expected[0]<0.8);
    let mut zip=zip::ZipArchive::new(std::io::Cursor::new(archive.bytes)).unwrap();
    let name=zip.file_names().find_map(|n|n.ok().filter(|n|n.contains("BaseColor")).map(|n|n.into_owned())).unwrap();
    let mut bytes=Vec::new();std::io::Read::read_to_end(&mut zip.by_name(&name).unwrap(),&mut bytes).unwrap();
    let pixel=subcraft_image::io::decode_bitmap(&bytes,Some(&name)).unwrap().texel(64,63,true);
    assert!((pixel[0]-expected[0]).abs()<0.01,"latest blur must export {pixel:?} vs {expected:?}");
    let path=std::env::temp_dir().join(format!("subcraft-spatial-{}.subcraft",layer.0));s.save(Some(&path)).unwrap();
    let mut reopened=Session::open_project(&path).unwrap();reopened.update_composites();
    assert_eq!(reopened.document(),s.document());assert_eq!(reopened.composite(reopened.selection().texture_set),s.composite(s.selection().texture_set));std::fs::remove_file(path).unwrap();
}
