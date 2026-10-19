//! Native filter fixtures and CPU-only 4K edit profile; no owner UI input.
use std::time::{Duration,Instant};
use subcraft_app::{Command,NewLayer,Session};
use subcraft_core::{ChannelKind as C,ChannelValue};
use subcraft_doc::{ContentEffectKind,FillChannel,FillSource,Filter,MaskEffectKind,OutlinePosition,ProceduralKind,ProceduralParams};
#[global_allocator]
static GLOBAL:mimalloc::MiMalloc=mimalloc::MiMalloc;

fn fixture(size:u32,choice:&str)->Result<Session,String> {
    let mut s=Session::new(subcraft_mesh::primitives::sample_crate(),size);
    let sets:Vec<_>=s.document().texture_sets.iter().map(|t|t.id).collect();
    for set in sets {
        s.execute(Command::SelectTextureSet(set)).map_err(|e|format!("{e:?}"))?;
        s.execute(Command::AddLayer(NewLayer::Fill)).map_err(|e|format!("{e:?}"))?;
        let layer=s.selection().layer.unwrap();
        s.execute(Command::Rename {layer,name:"Filter study".into()}).map_err(|e|format!("{e:?}"))?;
        let source=FillSource::Procedural {params:ProceduralParams {scale:if matches!(choice,"blur"|"sharpen"|"directional"|"highpass"|"mask-directional"|"mask-highpass"){24.0}else{5.0},..ProceduralParams::new(ProceduralKind::Clouds)},
            low:ChannelValue::rgb(0.03,0.005,0.001),high:ChannelValue::rgb(0.9,0.3,0.03)};
        s.execute(Command::SetFillChannel {layer,channel:C::BaseColor,fill:FillChannel::new(source)}).map_err(|e|format!("{e:?}"))?;
        if choice=="bevel" {
            s.execute(Command::SetFillChannel {layer,channel:C::Height,fill:FillChannel::new(FillSource::Procedural {params:ProceduralParams {scale:8.0,..ProceduralParams::new(ProceduralKind::Checker)},low:ChannelValue::scalar(0.0),high:ChannelValue::scalar(1.0)})}).map_err(|e|format!("{e:?}"))?;
            s.execute(Command::SetFillChannel {layer,channel:C::BaseColor,fill:FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.3,0.32,0.34)))}).map_err(|e|format!("{e:?}"))?;
        }
        if choice=="threshold" || choice.starts_with("mask-") {
            s.execute(Command::AddMask {layer,white:false}).map_err(|e|format!("{e:?}"))?;
            s.execute(Command::AddMaskEffect {layer,kind:MaskEffectKind::Fill {value:1.0,
                procedural:Some(ProceduralParams {scale:if matches!(choice,"mask-directional"|"mask-highpass"){24.0}else{5.0},..ProceduralParams::new(ProceduralKind::Clouds)}),projection:Default::default()}}).map_err(|e|format!("{e:?}"))?;
            s.execute(Command::AddMaskEffect {layer,kind:MaskEffectKind::Filter(match choice {"mask-bevel"=>Filter::Bevel {radius:16.0,amount:1.0,threshold:0.5},"mask-outline"=>Filter::MaskOutline {radius:8.0,threshold:0.5,position:OutlinePosition::Both},"mask-blur"=>Filter::Blur {radius:16.0},"mask-directional"=>Filter::DirectionalBlur {radius:16.0,angle_deg:35.0},"mask-highpass"=>Filter::Highpass {radius:8.0,amount:2.0},_=>Filter::Threshold {cutoff:0.5}})}).map_err(|e|format!("{e:?}"))?;
        } else if choice!="folder" {
            let filter=match choice {"bevel"=>Filter::Bevel {radius:12.0,amount:0.025,threshold:0.5},"outline"=>Filter::MaskOutline {radius:8.0,threshold:0.25,position:OutlinePosition::Outer},"directional"=>Filter::DirectionalBlur {radius:16.0,angle_deg:35.0},"highpass"=>Filter::Highpass {radius:8.0,amount:2.0},"blur"=>Filter::Blur {radius:16.0},"sharpen"=>Filter::Sharpen {radius:4.0,amount:2.0},"contrast"=>Filter::Contrast {gain:2.0,pivot:0.3},"clamp"=>Filter::Clamp {low:0.1,high:0.4},
                _=>Filter::Hsl {hue:120.0,saturation:1.0,lightness:0.0}};
            s.execute(Command::AddContentEffect {layer,kind:ContentEffectKind::Filter {channel:if choice=="bevel" {C::Height}else{C::BaseColor},filter}}).map_err(|e|format!("{e:?}"))?;
        }
        if choice=="folder" {
            s.execute(Command::AddLayer(NewLayer::Folder)).map_err(|e|format!("{e:?}"))?;
            let folder=s.selection().layer.unwrap();
            s.execute(Command::Rename {layer:folder,name:"Color adjustment".into()}).map_err(|e|format!("{e:?}"))?;
            s.execute(Command::AddContentEffect {layer:folder,kind:ContentEffectKind::Filter {channel:C::BaseColor,
                filter:Filter::Hsl {hue:120.0,saturation:1.0,lightness:0.0}}}).map_err(|e|format!("{e:?}"))?;
        }
    }
    s.execute(Command::SelectTextureSet(s.document().texture_sets[0].id)).map_err(|e|format!("{e:?}"))?;
    Ok(s)
}

fn main()->Result<(),String> {
    let args:Vec<_>=std::env::args().skip(1).collect();
    if args.first().is_some_and(|a|a=="--profile") {
        let size=args.get(1).and_then(|a|a.parse().ok()).unwrap_or(4096);
        let choice=args.get(2).map_or("hsl",|s|s.as_str());let mut s=fixture(size,choice)?;let start=Instant::now();s.update_composites();println!("{size}², two sets, cold composite {:.1} ms",start.elapsed().as_secs_f64()*1000.0);
        let start=Instant::now();let mut times=Vec::new();
        for frame in 0.. {
            let now=Instant::now();
            if frame<8 {
                let set_ids:Vec<_>=s.document().texture_sets.iter().map(|t|t.id).collect();
                for set in set_ids {
                    s.execute(Command::SelectTextureSet(set)).map_err(|e|format!("{e:?}"))?;
                    let layer=s.selection().layer.unwrap();let mut effect=s.selected_layer().unwrap().content_effects[0].as_ref().clone();
                    effect.kind=ContentEffectKind::Filter {channel:if choice=="bevel" {C::Height}else{C::BaseColor},filter:match choice {"bevel"=>Filter::Bevel {radius:8.0+frame as f32,amount:0.025,threshold:0.5},"outline"=>Filter::MaskOutline {radius:4.0+frame as f32,threshold:0.25,position:OutlinePosition::Both},"directional"=>Filter::DirectionalBlur {radius:8.0+frame as f32,angle_deg:frame as f32*10.0},"highpass"=>Filter::Highpass {radius:4.0+frame as f32,amount:1.0},"blur"=>Filter::Blur {radius:4.0+frame as f32},"sharpen"=>Filter::Sharpen {radius:4.0,amount:0.5+frame as f32*0.2},_=>Filter::Hsl {hue:60.0+frame as f32*10.0,saturation:1.0,lightness:0.0}}};
                    s.execute(Command::SetContentEffect {layer,effect}).map_err(|e|format!("{e:?}"))?;
                }
            }
            s.update_composites_within(Some(Duration::from_millis(10)));times.push(now.elapsed().as_secs_f64()*1000.0);
            if frame>=8 && !s.composites_pending(){break;}
            if start.elapsed()>Duration::from_secs(120){return Err("worker convergence timeout".into());}
            std::thread::sleep(Duration::from_millis(16).saturating_sub(now.elapsed()));
        }
        times.sort_by(f64::total_cmp);println!("CPU edit/poll p50 {:.3}, p95 {:.3}, max {:.3} ms; convergence {:.1} ms; {} frames. GPU/render/upload excluded.",
            times[times.len()/2],times[times.len()*95/100],times[times.len()-1],start.elapsed().as_secs_f64()*1000.0,times.len());
    } else {
        let path=args.first().ok_or("expected output project path")?;let choice=args.get(1).map_or("hsl",|s|s.as_str());
        let mut s=fixture(512,choice)?;s.save(Some(std::path::Path::new(path)))?;println!("Saved {choice} filter fixture");
    }
    Ok(())
}
