use std::{collections::BTreeSet,sync::Arc};
use glam::Vec2;
use subcraft_assets::{starter::{self,Kind},preview::{Address,Settings,ImageJob,example}};
use subcraft_image::{Bitmap,BitmapData};
fn finish(b: Bitmap,k: Kind,s: Settings,side: u32,budget: usize)->Bitmap {let mut j=ImageJob::new(Arc::new(b),k,s,side).unwrap();j.step(0);assert_eq!(j.progress(),0.0);while !j.is_done(){j.step(budget);}j.finish().unwrap()}

#[test]
fn catalog_has_sixty_unique_small_thumbnails_and_lossless_full_size_pngs() {
    assert_eq!(starter::ENTRIES.len(),60);let mut ids=BTreeSet::new();let mut normals=0;let mut bytes=0;let mut thumbs=0;
    for (i,e) in starter::ENTRIES.iter().enumerate() {
        assert!(ids.insert(e.id()));let full=starter::encoded(i).unwrap();assert_eq!(e.bytes,full.len());bytes+=full.len();thumbs+=e.thumb.len();
        let b=subcraft_image::io::decode_bitmap(full,Some(e.file)).unwrap();assert_eq!((b.width,b.height),(1024,1024));
        let t=subcraft_image::io::decode_bitmap(e.thumb,Some("thumb.png")).unwrap();assert_eq!((t.width,t.height),(96,96));
        if e.kind==Kind::Normal {normals+=1;assert_eq!(b.channels,3);assert!(e.tileable());}else{assert_eq!(b.channels,1);assert!(!e.tileable());}
    }
    assert_eq!(normals,30);assert_eq!(bytes,12_928_272);assert!(thumbs<350_000);assert!(starter::encoded(60).is_none());
}
#[test]
fn neutral_normal_and_zero_strength_are_linear_unit_vectors() {
    let s=Settings {strength:0.0,..Default::default()};let b=Bitmap::new_u8(1,1,3,vec![210,80,230]);
    let n=s.sample(&b,Kind::Normal,Vec2::splat(0.5));assert_eq!(n,[0.5,0.5,1.0,1.0]);
    let n=Settings::default().sample(&b,Kind::Normal,Vec2::splat(0.5));let len=((n[0]*2.0-1.0).powi(2)+(n[1]*2.0-1.0).powi(2)+(n[2]*2.0-1.0).powi(2)).sqrt();assert!((len-1.0).abs()<1e-6);assert!(n[0]>0.7);
}
#[test]
fn green_flip_inversion_and_rotation_change_tangents_without_inverting_z() {
    let b=Bitmap::new_u8(1,1,3,vec![190,210,230]);let uv=Vec2::splat(0.5);
    let n=Settings::default().sample(&b,Kind::Normal,uv);let g=Settings {flip_green:true,..Default::default()}.sample(&b,Kind::Normal,uv);
    assert!((g[0]-n[0]).abs()<1e-6);assert!((g[1]+n[1]-1.0).abs()<1e-6);assert_eq!(g[2],n[2]);
    let i=Settings {invert:true,..Default::default()}.sample(&b,Kind::Normal,uv);assert!((i[0]+n[0]-1.0).abs()<1e-6);assert_eq!(i[2],n[2]);
    let r=Settings {rotation:90.0,..Default::default()}.sample(&b,Kind::Normal,uv);assert!((r[0]-(1.0-n[1])).abs()<1e-6);assert!((r[1]-n[0]).abs()<1e-6);
}
#[test]
fn alpha_adjustments_keep_coverage_and_do_not_gamma_transform() {
    let b=Bitmap::new_u8(1,1,2,vec![128,128]);let a=Settings::default().sample(&b,Kind::Alpha,Vec2::splat(0.5));assert!((a[0]-(128.0/255.0f32).powi(2)).abs()<1e-6);
    let s=Settings {brightness:0.2,contrast:2.0,invert:true,..Default::default()};let a=s.sample(&b,Kind::Alpha,Vec2::splat(0.5));assert!((a[0]-(1.0-((128.0/255.0-0.5)*2.0+0.7))*128.0/255.0).abs()<1e-6);
}
#[test]
fn addressing_modes_and_mirrored_normal_derivatives_differ() {
    let b=Bitmap::new_u8(2,1,1,vec![0,255]);let uv=Vec2::new(1.25,0.5);
    let sample=|wrap|Settings{wrap,..Default::default()}.sample(&b,Kind::Alpha,uv)[0];
    assert_eq!(sample(Address::Repeat),0.0);assert_eq!(sample(Address::Mirror),1.0);assert_eq!(sample(Address::Clamp),1.0);
    let n=Bitmap::new_u8(1,1,3,vec![220,128,230]);let s=Settings {wrap:Address::Mirror,..Default::default()};assert!(s.sample(&n,Kind::Normal,uv)[0]<0.5);
}
#[test]
fn raster_work_is_budget_independent_and_seamless_edges_match() {
    let b=Bitmap::new_u8(2,2,1,vec![0,255,100,200]);let settings=Settings {seamless:0.2,scale:1.4,rotation:24.0,..Default::default()};
    let a=finish(b.clone(),Kind::Alpha,settings,32,1);let c=finish(b,Kind::Alpha,settings,32,71);assert_eq!(a,c);
    for i in 0..32 {assert_eq!(a.texel(0,i,false),a.texel(31,i,false));assert_eq!(a.texel(i,0,false),a.texel(i,31,false));}
    assert!(Settings {strength:f32::NAN,..Default::default()}.validate().is_err());
    let BitmapData::U8(data)=a.data else{panic!()};assert!(data.iter().any(|&v|v>30));
}
#[test]
fn sample_stroke_and_lit_normal_preview_contain_image_detail() {
    for kind in [Kind::Alpha,Kind::Normal] {let b=Bitmap::new_u8(1,1,3,if kind==Kind::Normal{vec![128,128,255]}else{vec![255;3]});let p=example(&b,kind);let BitmapData::U8(v)=p.data else {panic!()};assert!(v.iter().max().unwrap()-v.iter().min().unwrap()>40);}
}
