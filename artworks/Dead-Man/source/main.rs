use glam::{Mat4, Vec2, Vec3};
use rayon::prelude::*;
use std::{path::Path, sync::Arc, time::Instant};
use subcraft_app::{Command, Session};
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress};
use subcraft_core::{ChannelKind as C, ChannelValue as V, MeshMapKind as M};
use subcraft_doc::*;
use subcraft_image::{FileFormat, TiledImage, load_bitmap, save_image};
use subcraft_mesh::{Bvh, Mesh};
use subcraft_render::{headless, ViewParams, ViewportRenderer, SHADED_CHANNELS};

#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;
const MASKS: usize = 15;
fn sm(a:f32,b:f32,x:f32)->f32 { let t=((x-a)/(b-a)).clamp(0.,1.);t*t*(3.-2.*t) }
fn band(a:f32,b:f32,x:f32,soft:f32)->f32 {sm(a-soft,a,x)*(1.-sm(b,b+soft,x))}
fn hash(x:i32,y:i32,z:i32,seed:u32)->f32 {
    let mut h=(x as u32).wrapping_mul(374761393)^(y as u32).wrapping_mul(668265263)^(z as u32).wrapping_mul(2246822519)^seed;
    h=(h^(h>>13)).wrapping_mul(1274126177);((h^(h>>16))>>8) as f32/16777215.
}
// Non-periodic object-space noise. Identical positions have identical detail across UV seams.
fn noise(p:Vec3,seed:u32)->f32 {
    let c=p.floor();let f=p-c;let u=f*f*f*(f*(f*6.-Vec3::splat(15.))+Vec3::splat(10.));let mut v=0.;
    for z in 0..2 {for y in 0..2 {for x in 0..2 {let w=Vec3::new(if x==0 {1.-u.x}else{u.x},if y==0 {1.-u.y}else{u.y},if z==0 {1.-u.z}else{u.z});v+=w.x*w.y*w.z*hash(c.x as i32+x,c.y as i32+y,c.z as i32+z,seed);}}}v
}
fn fbm(p:Vec3,seed:u32)->f32 {noise(p,seed)*0.57+noise(p*2.07+Vec3::splat(3.7),seed+1)*0.28+noise(p*4.13,seed+2)*0.15}
fn ell(p:Vec3,c:[f32;3],r:[f32;3])->f32 {1.-sm(0.55,1.20,((p-Vec3::from(c))/Vec3::from(r)).length())}
fn rgb(a:[f32;3])->V { V::rgb(subcraft_core::color::srgb_to_linear(a[0]),subcraft_core::color::srgb_to_linear(a[1]),subcraft_core::color::srgb_to_linear(a[2])) }
#[derive(Clone,Copy)]
struct Lesion { p:Vec3,r:f32 }
fn lesions(mesh:&Mesh,bvh:&Bvh)->Vec<Lesion> {
    let mut out=Vec::new();
    // Asymmetric shoulder clusters and satellite sores, continued over the back and legs.
    for (x,y,r,back) in [
        (-36.,-42.,2.0,false),(-43.,-44.,2.5,false),(-49.,-47.,1.8,false),(-39.,-48.,1.5,false),
        (-46.,-52.,2.1,false),(-35.,-54.,2.4,false),(-52.,-54.,1.3,false),(-42.,-57.,1.4,false),
        (-31.,-46.,1.2,false),(-55.,-59.,1.6,false),(-48.,-61.,1.3,false),(-38.,-39.,1.2,false),
        (35.,-47.,2.6,false),(42.,-43.,1.6,false),(46.,-50.,1.3,false),(31.,-55.,1.6,false),
        (26.,-68.,2.0,false),(30.,-83.,1.7,false),(-20.,-102.,1.4,false),(57.,-83.,1.7,false),
        (64.,-111.,1.3,false),(-77.,-125.,1.6,false),(-25.,-213.,2.0,false),(33.,-239.,1.5,false),
        (-43.,-300.,1.7,false),(47.,-282.,1.2,false),(-23.,-59.,2.2,true),(-31.,-67.,1.5,true),
        (27.,-53.,2.1,true),(36.,-59.,1.5,true),(14.,-91.,1.6,true),(-13.,-125.,1.7,true),
        (-29.,-226.,1.8,true),(35.,-314.,1.4,true),(-17.,5.,0.9,false)
    ] {let sign=if back {-1.}else{1.};if let Some(hit)=bvh.intersect(Vec3::new(x,y,sign*200.),Vec3::new(0.,0.,-sign),0.,500.) {out.push(Lesion{p:mesh.interpolate(hit.tri as usize,hit.u,hit.v).position,r});}}out
}
#[derive(Clone,Copy)]
struct Fields { masks:[f32;MASKS], height:f32 }
fn fields(p:Vec3,n:Vec3,sites:&[Lesion])->Fields {
    let coarse=fbm(p*0.095,31);let fine=noise(p*0.8,87);let patch=fbm(p*0.28,64);
    let front=sm(-8.,2.,p.z);let back=1.-sm(-32.,-22.,p.z);let torso=band(-122.,-63.,p.y,8.);
    let muscle=(ell(p,[-20.,-91.,3.],[20.,32.,20.])*0.78+ell(p,[25.,-205.,-4.],[18.,35.,24.])*0.88+ell(p,[-28.,-226.,-9.],[16.,25.,25.])*0.6+ell(p,[44.,-70.,0.],[12.,20.,19.])*0.32).min(1.)*front;
    let rear_muscle=(ell(p,[-27.,-91.,-44.],[17.,26.,19.])*0.6+ell(p,[30.,-225.,-39.],[14.,26.,15.])*0.5)*back;
    let exposed=(muscle+rear_muscle).min(1.)*(0.70+0.30*sm(0.30,0.71,coarse));
    let bruise=(ell(p,[-16.,-14.,7.],[11.,13.,19.])+ell(p,[17.,-79.,-1.],[18.,33.,20.])*0.50+ell(p,[41.,-260.,-21.],[16.,16.,20.])*0.65).min(1.)*(0.45+0.45*coarse);
    let warmth=(torso*0.42+muscle*0.7+rear_muscle*0.45).min(0.85)*(0.4+0.6*patch);
    let mouth_d=Vec2::new((p.x-5.)/10.5,(p.y+19.)/14.).length();
    let mouth=(1.-sm(0.76,0.96,mouth_d))*front*band(-37.,-5.,p.y,1.);
    let lip=(1.-sm(0.12,0.24,(mouth_d-0.91).abs()))*front*band(-38.,-4.,p.y,1.);
    let neck=ell(p,[0.,-45.,-2.],[12.,15.,18.])*front;
    let seam_x=1.2+(p.y*0.17).sin()*1.15;
    let skull=(1.-sm(0.5,1.6,(p.x-seam_x).abs()))*band(-5.,27.,p.y,2.)*sm(-4.,4.,p.z);
    let belly=(1.-sm(0.45,1.5,(p.x-2.-(p.y*0.095).sin()*1.5).abs()))*band(-174.,-119.,p.y,3.)*front;
    let sternum=(1.-sm(0.3,1.1,p.x.abs()))*band(-116.,-66.,p.y,4.)*front*0.7;
    let spine=(1.-sm(0.6,2.1,(p.x+(p.y*0.12).sin()).abs()))*band(-149.,-50.,p.y,4.)*back*0.70;
    let scar=(skull+belly+sternum+spine).min(1.);let necrotic=(mouth+neck*0.8+scar*0.85).min(1.);
    let d=sites.iter().map(|l|p.distance(l.p)/l.r).fold(20.,f32::min)+(fine-0.5)*0.13;
    let rim=(1.-sm(0.85,1.4,d))*sm(0.42,0.77,d);let core=1.-sm(0.45,0.80,d);let pus=(1.-sm(0.22,0.43,d))*sm(0.48,0.66,patch)*0.75;
    let teeth_y=-8.8-((p.x-4.)*0.2).abs()*0.6;let tooth_wave=((p.x+0.8)*1.75).sin();
    let teeth=band(-3.,13.,p.x,0.3)*band(teeth_y-3.2,teeth_y+0.7,p.y,0.4)*sm(0.12,0.40,tooth_wave)*front*sm(9.,13.,p.z);
    let eye=ell(p,[10.5,1.2,2.],[4.2,4.8,8.])*front*0.95;
    let soil=sm(0.60,0.80,coarse)*0.38*(0.35+0.65*sm(-280.,-375.,p.y));
    let cap=(1.-sm(0.012,0.035,(noise(p*0.5+Vec3::new(3.,0.,0.),8)-0.50).abs()))*sm(0.60,0.78,patch)*0.24;
    let ashen=(0.12+sm(0.45,0.75,fbm(p*0.13+Vec3::splat(7.),128))*0.42)*(1.-exposed*0.8);let bony=sm(0.4,0.8,n.y)*0.08;
    let pores=(fine-0.5)*0.00065+(noise(p*2.7,909)-0.5)*0.00022;
    let fiber=((p.y*2.6+noise(p*0.3,18)*7.).sin()*0.5+0.5)*0.0007*exposed;
    Fields{masks:[warmth,ashen,bruise,exposed,scar,necrotic,rim,core,pus,lip,teeth,eye,soil,cap,bony],height:pores+rim*0.0014-core*0.0012-scar*0.001+fiber}
}
fn material(s:&Session,name:&str,color:[f32;3],rough:f32,mask:Option<TiledImage>)->Layer {
    let mut l=Layer::new_fill(name,s.selected_texture_set());if let LayerContent::Fill(f)=&mut l.content {
        f.channels.clear();for (ch,v) in [(C::BaseColor,rgb(color)),(C::Roughness,V::scalar(rough))] {f.channels.insert(ch,FillChannel::new(FillSource::uniform(v)));}
        f.projection.mode=ProjectionMode::Triplanar;let mut p=ProceduralParams::new(ProceduralKind::Clouds);p.scale=55.;p.seed=283;
        f.channels.insert(C::Roughness,FillChannel::new(FillSource::Procedural{params:p,low:V::scalar(rough-0.06),high:V::scalar(rough+0.05)}));
    }l.mask=mask.map(|base|Mask{enabled:true,base,effects:vec![]});l
}
fn insert(s:&mut Session,l:Layer) {s.execute(Command::InsertLayer{label:l.name.clone(),layer:Box::new(l)}).unwrap();}
fn masks(mesh:&Mesh,res:u32,sites:&[Lesion])->(Vec<TiledImage>,TiledImage,usize) {
    println!("Rasterizing surface at {res}...");let samples=subcraft_bake::surface::texels(mesh,0,res);let count=samples.len();println!("Authoring {count} surface texels...");
    let evaluated:Vec<_>=samples.par_iter().map(|t|fields(t.position,t.normal,sites)).collect();
    let images:Vec<_>=(0..MASKS).map(|k| {let mut dense=vec![0.;(res*res) as usize];for (s,f) in samples.iter().zip(&evaluated) {dense[s.index as usize]=f.masks[k];}TiledImage::from_dense(res,res,1,[0.;4],&dense)}).collect();
    let mut dense=vec![0.;(res*res) as usize*2];for (s,f) in samples.iter().zip(&evaluated) {dense[s.index as usize*2]=f.height;dense[s.index as usize*2+1]=1.;}
    (images,TiledImage::from_dense(res,res,2,[0.;4],&dense),count)
}
fn inspect(mesh:&Mesh,out:&Path) {
    let mut uv=image::RgbImage::from_pixel(1536,1536,image::Rgb([22,24,27]));
    for t in 0..mesh.triangle_count() {let vs=mesh.tri_uvs(t);for i in 0..3 {let a=vs[i]*1535.;let b=vs[(i+1)%3]*1535.;let steps=(b-a).abs().max_element().ceil() as u32;for j in 0..=steps {let p=a.lerp(b,j as f32/steps.max(1) as f32);if p.x>=0.&&p.y>=0.&&p.x<1536.&&p.y<1536. {uv.put_pixel(p.x as u32,p.y as u32,image::Rgb([126,142,156]));}}}}uv.save(out.join("renders/UV-layout.png")).unwrap();
    let res=512;let mut positions=vec![None::<Vec3>;res*res];let mut overlap=0;
    for t in 0..mesh.triangle_count() {subcraft_mesh::uv_raster::raster_triangle(mesh.tri_uvs(t),res as u32,res as u32,subcraft_core::IRect::from_size(res as u32,res as u32),0.,|hit|{if hit.distance>0. {return;}let p=mesh.interpolate(t,hit.bary[1],hit.bary[2]).position;let idx=hit.y as usize*res+hit.x as usize;if positions[idx].is_some_and(|q|p.distance(q)>1.) {overlap+=1;}positions[idx]=Some(p);});}
    let report=serde_json::json!({"vertices":mesh.vertex_count(),"triangles":mesh.triangle_count(),"objects":mesh.objects.iter().map(|o|&o.name).collect::<Vec<_>>(),"materials":mesh.materials.iter().map(|m|&m.name).collect::<Vec<_>>(),"bounds_min":mesh.bounds.min.to_array(),"bounds_max":mesh.bounds.max.to_array(),"uv_overlap_samples_at_512":overlap,"covered_samples_at_512":positions.iter().filter(|x|x.is_some()).count(),"separate_high_poly":false,"import_warnings":["7 degenerate triangles dropped by importer"]});println!("INSPECTION {report}");std::fs::write(out.join("inspection.json"),serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
fn bake(mesh:&Mesh,bvh:&Bvh,out:&Path,size:u32)->std::collections::BTreeMap<M,TiledImage> {
    let mut cached=std::collections::BTreeMap::new();
    for k in M::ALL {let file=out.join(format!("bakes/{k:?}.png"));if let Ok(b)=load_bitmap(&file) {if b.width==size {let c=k.components();let dense:Vec<f32>=(0..size).flat_map(|y| (0..size).flat_map({let b=&b;move |x| b.texel(x,y,false)[..c].to_vec()})).collect();cached.insert(k,TiledImage::from_dense(size,size,c,[0.;4],&dense));}}}
    if cached.len()==M::ALL.len() {println!("Reusing validated {size} mesh maps");return cached;}
    println!("Baking low-on-low at {size}, AO 64 rays; no separate high-poly source");
    let settings=BakeSettings{size,ao_samples:64,ao_distance:0.035,thickness_samples:24,thickness_distance:0.12,curvature_radius:0.003,..Default::default()};
    subcraft_bake::bake_streamed(&BakeInput{low:Geometry{mesh,bvh},high:None,material_slot:0},&settings,&Progress::new(),|k,img|{println!("Baked {k:?}");save_image(&out.join(format!("bakes/{k:?}.png")),size,size,img.comps(),&img.to_dense(),FileFormat::Png16).unwrap();}).unwrap()
}
fn render(s:&Session,out:&Path,label:&str,eye_dir:Vec3,target:Vec3,distance:f32) {
    let (_,dev,queue)=headless::device().unwrap();let mut renderer=ViewportRenderer::new(&dev,&queue);renderer.set_mesh(&dev,&s.scene().mesh);
    for ts in &s.document().texture_sets {let comp=s.composite(ts.id).unwrap();for ch in SHADED_CHANNELS {if let Some(img)=comp.channel(ch) {renderer.upload_channel(&dev,&queue,ts.material_slot,ch,img,None);}}}
    let size=[1400,1600];let eye=target+eye_dir.normalize()*distance;let mut p=ViewParams::new(Mat4::look_at_rh(eye,target,Vec3::Y),Mat4::perspective_rh(32f32.to_radians(),size[0] as f32/size[1] as f32,distance*0.001,distance*10.));
    p.background=[0.028,0.030,0.031];p.light_dir=(eye_dir.normalize()+Vec3::new(-0.7,0.9,0.)).normalize();p.light_intensity=2.2;p.environment_intensity=0.65;
    let rgba=renderer.render(&dev,&queue,size,&p).read_rgba8(&dev,&queue);image::save_buffer(out.join(format!("renders/{label}.png")),&rgba,size[0],size[1],image::ColorType::Rgba8).unwrap();
}
fn renders(s:&Session,out:&Path) {
    let b=s.scene().mesh.bounds;let h=b.extent().y;
    for (label,dir,tar,d) in [("Dead-Man-front",Vec3::new(0.,0.02,1.),b.center(),h*2.05),("Dead-Man-back",Vec3::new(-0.12,0.03,-1.),b.center(),h*2.05),("Dead-Man-reference-angle",Vec3::new(0.50,0.06,1.),Vec3::new(0.,-57.,-9.),h*1.05),("Dead-Man-head-detail",Vec3::new(0.10,0.01,1.),Vec3::new(0.,-14.,-2.),h*0.40),("Dead-Man-shoulder-detail",Vec3::new(-0.15,0.12,1.),Vec3::new(-34.,-58.,-4.),h*0.34)] {println!("Rendering {label}");render(s,out,label,dir,tar,d);}
}
fn main() {
    let args:Vec<_>=std::env::args().collect();let out=Path::new(args.get(1).map(String::as_str).unwrap_or("artworks/Dead-Man"));let res=if args.iter().any(|a|a=="--draft") {1024}else{4096};
    if args.iter().any(|a|a=="--render-only") {let mut s=Session::open_project(&out.join("Dead-Man.subcraft")).unwrap();s.update_composites();renders(&s,out);return;}
    let started=Instant::now();let imported=subcraft_mesh::import_mesh(&out.join("models/dead_man.fbx")).unwrap();println!("Import {:?}",imported.stats);let mesh=imported.mesh;inspect(&mesh,out);let bvh=Bvh::build(&mesh);let sites=lesions(&mesh,&bvh);println!("{} authored lesion sites",sites.len());
    let baked=bake(&mesh,&bvh,out,if res==4096 {2048}else{1024});let (mask_images,height,covered)=masks(&mesh,res,&sites);let mut masks=mask_images.into_iter();
    let mut resources=ResourceStore::default();let path=out.join("resources/epidermis-albedo.png");let id=subcraft_core::ResourceId::new();resources.insert(Resource{id,name:"Pale leathery epidermis · generated diffuse source".into(),kind:ResourceKind::Texture,bitmap:Arc::new(load_bitmap(&path).unwrap()),source_path:Some(path.clone()),encoded:Some(Arc::new(std::fs::read(path).unwrap()))});
    let names=mesh.materials.iter().map(|m|m.name.clone()).collect::<Vec<_>>();let mut doc=Document::from_material_slots(&names,res);doc.texture_sets[0].channels.push(ChannelDef::default_for(C::AmbientOcclusion));let mut s=Session::from_parts(mesh,doc,resources,"Dead Man artwork");let ts=s.selected_texture_set().id;s.set_mesh_maps(ts,baked);
    let mut base=material(&s,"01 | Epidermis · pale leathery skin",[0.66,0.60,0.49],0.67,None);if let LayerContent::Fill(f)=&mut base.content {f.projection.scale=[4.8,4.8];f.projection.triplanar_sharpness=3.;let mut color=FillChannel::new(FillSource::Bitmap{resource:id});color.strength=0.72;f.channels.insert(C::BaseColor,color);let mut rough=FillChannel::new(FillSource::Bitmap{resource:id});rough.strength=0.98;f.channels.insert(C::Roughness,rough);f.channels.insert(C::Metallic,FillChannel::new(FillSource::uniform(V::scalar(0.))));f.channels.insert(C::Normal,FillChannel::new(FillSource::uniform(V::rgb(0.5,0.5,1.))));f.channels.insert(C::Height,FillChannel::new(FillSource::uniform(V::scalar(0.))));}insert(&mut s,base);
    for (name,color,rough) in [
        ("02 | Subdermal warmth · ribs and thighs",[0.47,0.28,0.22],0.66),("03 | Ash gray mottling",[0.47,0.46,0.41],0.76),("04 | Bruising · cheek, flank, knees",[0.27,0.235,0.24],0.69),("05 | Exposed muscle · chest, thighs, scapula",[0.42,0.19,0.15],0.49),("06 | Torn seams · skull, abdomen and spine",[0.23,0.105,0.085],0.47),("07 | Necrotic cavities · mouth and throat",[0.105,0.043,0.038],0.39),("08 | Inflamed lesion rims · front and back",[0.46,0.23,0.18],0.55),("09 | Lesion centers · dark red tissue",[0.28,0.075,0.058],0.34),("10 | Restrained ochre pustule tips",[0.63,0.51,0.32],0.40),("11 | Dry torn lips · oxidized blood",[0.25,0.10,0.08],0.60),("12 | Aged teeth · warm ivory",[0.66,0.59,0.40],0.55),("13 | Clouded eye remnant",[0.44,0.43,0.39],0.31),("14 | Grounded decay · ankles and recesses",[0.29,0.27,0.20],0.81),("15 | Fine capillary discoloration",[0.42,0.24,0.20],0.68),("16 | Raised skin planes · restrained pallor",[0.70,0.66,0.55],0.70)
    ] {let mut l=material(&s,name,color,rough,Some(masks.next().unwrap()));if name.starts_with("02") || name.starts_with("05") {if let LayerContent::Fill(f)=&mut l.content {let mut p=ProceduralParams::new(ProceduralKind::Clouds);p.scale=90.;p.seed=401;f.channels.insert(C::BaseColor,FillChannel::new(FillSource::Procedural{params:p,low:rgb(color.map(|v|v*0.82)),high:rgb(color.map(|v|(v*1.1).min(1.)))}));}}insert(&mut s,l);}
    let mut micro=Layer::new_fill("16b | Leathery microrelief · triplanar skin source",s.selected_texture_set());if let LayerContent::Fill(f)=&mut micro.content {f.channels.clear();f.projection.mode=ProjectionMode::Triplanar;f.projection.scale=[4.8,4.8];f.projection.triplanar_sharpness=3.;let mut ch=FillChannel::new(FillSource::Bitmap{resource:id});ch.strength=0.004;f.channels.insert(C::Height,ch);}insert(&mut s,micro);
    let mut relief=Layer::new_paint("17 | Pores, tissue fibers and lesion relief",s.selected_texture_set());if let LayerContent::Paint(p)=&mut relief.content {p.channels.insert(C::Height,height);}relief.blend.insert(C::Height,ChannelBlend::new(BlendMode::SignedAddition));insert(&mut s,relief);
    let mut cavity=material(&s,"18 | Crease staining · live baked cavity generator",[0.24,0.20,0.16],0.72,None);let mut g=Generator::new(GeneratorKind::Cavity);g.level=0.32;g.softness=0.25;g.noise_amount=0.22;g.noise.scale=38.;g.organic_breakup=true;let mut e=MaskEffect::new(MaskEffectKind::Generator(g));e.opacity=0.38;cavity.mask=Some(Mask{enabled:true,base:TiledImage::new(res,res,1,[0.;4]),effects:vec![Arc::new(e)]});insert(&mut s,cavity);
    let mut ao=Layer::new_fill("19 | Ambient occlusion · live mesh map",s.selected_texture_set());if let LayerContent::Fill(f)=&mut ao.content {f.channels.clear();f.channels.insert(C::AmbientOcclusion,FillChannel::new(FillSource::MeshMap{map:M::AmbientOcclusion}));}insert(&mut s,ao);
    println!("Compositing {res} artwork...");s.update_composites();let comp=s.composite(ts).unwrap().clone();for (ch,img) in &comp.channels {assert!(img.to_dense().iter().all(|v|v.is_finite()),"finite {ch:?}");}
    let path=out.join("Dead-Man.subcraft");s.save(Some(&path)).unwrap();println!("Saved {}",path.display());drop(s);let mut reopened=Session::open_project(&path).unwrap();reopened.update_composites();assert_eq!(&comp,reopened.composite(ts).unwrap(),"saved/reopened composite must be bit-exact");
    println!("Reopen composite is bit-exact; exporting PBR...");reopened.export_textures(&out.join("textures/PBR"),&subcraft_export::ExportSettings::default()).unwrap();
    let report=serde_json::json!({"texture_resolution":res,"mesh_map_resolution":if res==4096 {2048}else{1024},"baked_maps":7,"bake_source":"low-on-low; only supplied body mesh exists","ao_rays":64,"covered_texels":covered,"layers":reopened.selected_texture_set().layers.len(),"authored_lesion_sites":sites.len(),"all_channels_finite":true,"save_reopen_composite":"bit-exact","color_encoding":"sRGB base color, linear data; OpenGL +Y normal; signed height encoded around 0.5 in PNG16","elapsed_seconds":started.elapsed().as_secs_f32()});std::fs::write(out.join("validation.json"),serde_json::to_vec_pretty(&report).unwrap()).unwrap();renders(&reopened,out);println!("DONE {report}");
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn anatomical_fields_are_localized_and_finite() {
        assert!(fields(Vec3::new(1.2+(12f32*0.17).sin()*1.15,12.,13.),Vec3::Z,&[]).masks[4]>0.5);assert!(fields(Vec3::new(5.,-19.,8.),Vec3::Z,&[]).masks[5]>0.9);assert!(fields(Vec3::new(5.,-19.,-45.),Vec3::NEG_Z,&[]).masks[5]<0.01);
        for y in -380..27 {let f=fields(Vec3::new(17.,y as f32,-4.),Vec3::Z,&[]);assert!(f.masks.iter().all(|v|v.is_finite()&&(0. ..=1.).contains(v)));assert!(f.height.is_finite()&&f.height.abs()<0.005);}
    }
    #[test] fn detail_is_deterministic_continuous_and_nonperiodic() {let p=Vec3::new(-16.2,-50.3,4.1);let a=fields(p,Vec3::Z,&[]);let b=fields(p,Vec3::Z,&[]);assert_eq!(a.masks,b.masks);assert_eq!(a.height,b.height);assert!((noise(p,31)-noise(p+Vec3::splat(0.00001),31)).abs()<0.0001);assert!((noise(p,31)-noise(p+Vec3::X,31)).abs()>0.01);}
    #[test] fn pustules_have_separate_rims_and_centers() {let sites=[Lesion{p:Vec3::new(-40.,-45.,4.),r:2.}];let center=fields(sites[0].p,Vec3::Z,&sites);let rim=fields(sites[0].p+Vec3::X*1.8,Vec3::Z,&sites);let far=fields(sites[0].p+Vec3::X*8.,Vec3::Z,&sites);assert!(center.masks[7]>0.9);assert!(rim.masks[6]>0.8);assert_eq!(far.masks[6],0.);assert_eq!(far.masks[7],0.);assert!(rim.height>center.height);}
}
