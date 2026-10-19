use subcraft_app::{Session,Command,asset_apply::Destination};
use subcraft_assets::starter::Kind;
use subcraft_core::ChannelKind;
use subcraft_doc::{LayerContent,FillSource};
use subcraft_image::Bitmap;
fn session()->Session {Session::new(subcraft_mesh::primitives::cube(2.0),128)}
fn normal()->Bitmap {Bitmap::new_u8(1,1,3,vec![128,128,255])}
#[test]
fn normal_application_is_one_undoable_fill_with_portable_resource() {
    let mut s=session();let before=s.document().clone();Destination::capture(&s,None).apply(&mut s,"Test",Kind::Normal,normal()).unwrap();
    let LayerContent::Fill(f)=&s.selected_layer().unwrap().content else {panic!()};assert!(f.channels[&ChannelKind::Normal].enabled);assert!(!f.channels[&ChannelKind::BaseColor].enabled);
    let FillSource::Bitmap{resource}=f.channels[&ChannelKind::Normal].source else {panic!()};assert_eq!(s.resources().get(resource).unwrap().bitmap.texel(0,0,false)[2],1.0);
    let after=s.document().clone();s.execute(Command::Undo).unwrap();assert_eq!(s.document(),&before);s.execute(Command::Redo).unwrap();assert_eq!(s.document(),&after);
    let path=std::env::temp_dir().join(format!("asset-{}.subcraft",subcraft_core::ObjectId::new()));s.save(Some(&path)).unwrap();let reopened=Session::open_project(&path).unwrap();assert_eq!(reopened.document(),s.document());assert_eq!(reopened.resources().get(resource).unwrap().bitmap.texel(0,0,false)[2],1.0);std::fs::remove_file(path).unwrap();
}
#[test]
fn delayed_image_does_not_apply_to_a_different_project_or_wrong_channel() {
    let s=session();let d=Destination::capture(&s,None);let mut other=session();let before=other.document().clone();assert!(d.apply(&mut other,"No",Kind::Normal,normal()).is_err());assert_eq!(other.document(),&before);
    let mut s=session();Destination::capture(&s,None).apply(&mut s,"Fill",Kind::Normal,normal()).unwrap();let id=s.selected_layer().unwrap().id;let before=s.document().clone();let d=Destination::capture(&s,Some((id,ChannelKind::BaseColor)));assert!(d.apply(&mut s,"Wrong",Kind::Normal,normal()).is_err());assert_eq!(s.document(),&before);
}
#[test]
fn alpha_application_changes_the_brush_without_editing_project_layers() {
    let mut s=session();let before=s.document().clone();Destination::capture(&s,None).apply(&mut s,"Starter: Soft",Kind::Alpha,Bitmap::new_u8(2,2,1,vec![0,255,255,0])).unwrap();assert_eq!(s.brush.alpha_name,"Starter: Soft");assert_eq!(s.document(),&before);assert!(s.brush.alpha.is_some());
}
#[test]
fn saving_a_brush_preset_persists_only_its_adjusted_starter_tip() {
    let root=std::env::temp_dir().join(format!("starter-preset-{}",subcraft_core::ObjectId::new()));let lib=subcraft_assets::UserLibrary::new(&root);let mut s=session();
    Destination::capture(&s,None).apply(&mut s,"Starter: Test",Kind::Alpha,Bitmap::new_u8(2,2,1,vec![15,200,80,255])).unwrap();
    let (preset,tip)=s.save_current_brush_preset(&lib,"Saved").unwrap();assert!(tip.is_some());
    let (tips,errors)=lib.load_alphas();assert!(errors.is_empty());assert_eq!(tips.len(),1);let loaded=subcraft_app::brush_preset::load_brush_preset(preset.user_file.as_ref().unwrap()).unwrap();let mut reopened=session();assert!(reopened.apply_brush_preset(&loaded,&tips).is_none());assert!((reopened.brush.alpha.as_ref().unwrap().texel(0,0,false)[0]-15.0/255.0).abs()<1e-6);assert_eq!(tips[0].id,tip.unwrap().id);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn identical_adjusted_images_share_one_resource_and_channel_drop_keeps_strength() {
    let mut s=session();Destination::capture(&s,None).apply(&mut s,"One",Kind::Normal,normal()).unwrap();
    let id=s.selected_layer().unwrap().id;let LayerContent::Fill(f)=&s.selected_layer().unwrap().content else {panic!()};let mut fill=f.channels[&ChannelKind::Normal].clone();fill.strength=0.4;s.execute(Command::SetFillChannel {layer:id,channel:ChannelKind::Normal,fill}).unwrap();
    Destination::capture(&s,Some((id,ChannelKind::Normal))).apply(&mut s,"Again",Kind::Normal,normal()).unwrap();assert_eq!(s.resources().iter().count(),1);let LayerContent::Fill(f)=&s.selected_layer().unwrap().content else{panic!()};assert_eq!(f.channels[&ChannelKind::Normal].strength,0.4);
}
#[test]
fn adjusted_imported_maps_keep_their_channel_classification() {
    let mut s=session();Destination::capture(&s,None).apply(&mut s,"Wood_Roughness",Kind::Texture,Bitmap::new_u8(1,1,1,vec![70])).unwrap();let LayerContent::Fill(f)=&s.selected_layer().unwrap().content else{panic!()};assert!(f.channels[&ChannelKind::Roughness].enabled);assert!(!f.channels[&ChannelKind::BaseColor].enabled);
}

#[test]
fn open_pack_material_imports_its_images_once_and_composites_image_detail() {
    let mut s = session();
    let lib = subcraft_assets::Library::builtin();
    let m = lib.materials.iter().find(|m| m.name == "Metal Plates").unwrap();
    let before = s.resources().iter().count();
    s.add_material(m).unwrap();
    assert_eq!(s.history().undo_label(), Some("Material: Metal Plates"));
    assert_eq!(s.resources().iter().count(), before + m.images.len());
    let layer = s.selected_layer().unwrap();
    let LayerContent::Fill(f) = &layer.content else { panic!() };
    for img in &m.images {
        assert!(matches!(f.channels[&img.channel].source, FillSource::Bitmap { .. }), "{:?}", img.channel);
    }
    // Applying it again reuses the same resources (no duplicate image data in the project).
    s.add_material(m).unwrap();
    assert_eq!(s.resources().iter().count(), before + m.images.len());
    s.update_composites();
    let ts = s.selected_texture_set().id;
    let color = s.composite(ts).unwrap().channel(ChannelKind::BaseColor).unwrap().to_dense();
    let (lo, hi) = color.chunks(4).fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p[0]), hi.max(p[0])));
    assert!(hi - lo > 0.02 && hi > lo * 1.5, "image detail reaches the composite ({lo}..{hi})");
}

#[test]
fn textured_smart_material_brings_its_images_and_is_one_undo_step() {
    let mut s = session();
    let lib = subcraft_assets::Library::builtin();
    let smart = lib.smart_materials.iter().find(|m| m.name == "Painted Steel Plates").unwrap();
    let entries = s.history().entries().len();
    s.add_smart_material(smart).unwrap();
    assert_eq!(s.history().entries().len(), entries + 1);
    let folder = s.selected_layer().unwrap();
    let LayerContent::Fill(base) = &folder.children()[0].content else { panic!() };
    let FillSource::Bitmap { resource } = base.channels[&ChannelKind::BaseColor].source else { panic!("image base color") };
    assert!(s.resources().get(resource).is_some());
}

#[test]
fn builtin_brush_presets_find_their_tips_and_set_the_brush() {
    let mut s = session();
    let tips = subcraft_assets::alphas::builtin();
    for p in subcraft_app::brush_preset::builtin() {
        assert_eq!(s.apply_brush_preset(&p, &tips), None, "{}", p.name);
        assert_eq!(s.brush.settings, p.settings);
        assert_eq!(s.brush.alpha_name, p.tip.clone().unwrap_or_else(|| "Round".into()), "{}", p.name);
    }
}
