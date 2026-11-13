//! Shelf image loading and preview presentation. Image math lives in assets; project edits in app.
use std::{collections::HashMap, path::PathBuf, sync::{Arc,atomic::{AtomicBool,AtomicU32,Ordering},mpsc::{Receiver,TryRecvError,channel}}};
use subcraft_app::asset_apply::{Destination,Placeholder};
use subcraft_app::stamp_brush::PreparedStamp;
use egui::{Color32, RichText};
use subcraft_assets::{preview::{Address,ImageJob,Settings},starter::{ENTRIES,Kind}};
use subcraft_image::Bitmap;
use crate::state::{UiState,Tool};

#[derive(Clone)]
pub enum Source { Builtin(usize), File(PathBuf), Bitmap(Arc<Bitmap>) }

#[derive(Default)]
pub struct AssetPreviewUi {
    pub open: bool, pub name: String, pub kind: Option<Kind>,
    pub settings: Settings, tiles: u32,
    source: Option<Arc<Bitmap>>, loader: Option<Receiver<Result<Arc<Bitmap>,String>>>,
    error: Option<String>, job: Option<ImageJob>, dirty: bool,
    image: Option<egui::TextureHandle>, example: Option<egui::TextureHandle>,
    thumbs: HashMap<usize,egui::TextureHandle>,
    destination: Option<Destination>, apply: Option<ApplyTask>,
    queued: Option<usize>,
    builtin: Option<usize>,
    /// When the pending application was requested (click-to-ready timing in the log).
    requested: Option<web_time::Instant>,
    /// Full-resolution stamps already encoded this session, by name and kind (unadjusted
    /// images only): choosing one again is instant.
    ready: Vec<(String,subcraft_paint::StampKind,PreparedStamp)>,
}

/// Most recently used full-resolution stamps kept in memory.
const READY_STAMPS: usize = 8;

/// What the worker thread hands back: the adjusted image, encoded already for a stamp.
enum Applied { Image(Bitmap), Stamp(PreparedStamp) }

/// The adjusted full image is resampled (and, for a stamp, PNG-encoded) on a worker thread so
/// the UI never waits for it; the frame loop only polls.
struct ApplyTask { rx: Receiver<Result<Applied,String>>, progress: Arc<AtomicU32>, cancel: Arc<AtomicBool> }
impl ApplyTask {
    fn spawn(ctx: &egui::Context, source: Arc<Bitmap>, kind: Kind, settings: Settings, stamp: bool, name: String) -> Result<Self,String> {
        let mut job=ImageJob::new(source.clone(),kind,settings,source.width.max(source.height).min(4096)).map_err(str::to_string)?;
        let (tx,rx)=channel(); let progress=Arc::new(AtomicU32::new(0)); let cancel=Arc::new(AtomicBool::new(false));
        let (p,c,ctx)=(progress.clone(),cancel.clone(),ctx.clone());
        subcraft_core::task::spawn(move || {
            while !job.is_done() {
                if c.load(Ordering::Relaxed) {return;}
                job.step(1<<16); p.store((job.progress()*1000.0) as u32,Ordering::Relaxed);
            }
            let bitmap=job.finish().expect("finished job");
            let result=if stamp {subcraft_app::stamp_brush::prepare_stamp(&name,bitmap).map(Applied::Stamp)} else {Ok(Applied::Image(bitmap))};
            if !c.load(Ordering::Relaxed) {let _=tx.send(result);ctx.request_repaint();}
        });
        Ok(Self{rx,progress,cancel})
    }
    fn progress(&self) -> f32 {self.progress.load(Ordering::Relaxed) as f32/1000.0}
}
impl Drop for ApplyTask { fn drop(&mut self) {self.cancel.store(true,Ordering::Relaxed);} }
impl AssetPreviewUi {
    pub fn is_applying(&self) -> bool {self.destination.is_some()}
    pub fn cancel_apply(&mut self) {self.destination=None;self.apply=None;self.queued=None;}
    pub fn queue(&mut self, session: &subcraft_app::Session, index: usize, channel: Option<(subcraft_core::LayerId,subcraft_core::ChannelKind)>) {
        self.queue_to(index,subcraft_app::asset_apply::Destination::capture(session,channel));
    }
    /// Use starter image `index` as the brush's image for `channel` (dropped on a Brush slot).
    pub fn queue_brush(&mut self, session: &subcraft_app::Session, index: usize, channel: subcraft_core::ChannelKind) {
        self.queue_to(index,subcraft_app::asset_apply::Destination::capture_brush(session,channel));
    }
    /// Use starter image `index` as the normal-map (or height) stamp brush.
    pub fn queue_stamp(&mut self, session: &subcraft_app::Session, index: usize, kind: subcraft_paint::StampKind) {
        self.queue_to(index,Destination::capture_stamp(session,kind));
    }
    /// Click on a Starter pack image in the Shelf: the brush changes in this frame. A
    /// full-resolution copy already encoded this session is used directly; otherwise the
    /// embedded thumbnail becomes the brush at once and the full image replaces it when its
    /// background load finishes (painting meanwhile uses the thumbnail resolution).
    /// `stamp: None` makes the image the brush tip (alpha).
    pub fn select_starter(&mut self, session: &mut subcraft_app::Session, index: usize, stamp: Option<subcraft_paint::StampKind>) {
        let Some(e)=ENTRIES.get(index) else {return};
        let started=web_time::Instant::now(); let name=format!("Starter: {}",e.name);
        match stamp {
            Some(kind) => {
                if self.use_ready(session,&name,kind) {return;}
                let thumb=subcraft_image::io::decode_bitmap(e.thumb,Some("thumb.png")).ok();
                let destination=Destination::capture_stamp(session,kind);
                let destination=match thumb.and_then(|t|subcraft_app::stamp_brush::prepare_stamp(&name,t).ok()) {
                    Some(prepared)=>{let id=session.set_prepared_stamp(kind,prepared);destination.with_placeholder(Placeholder::Stamp(id))}
                    None=>destination,
                };
                self.queue_to(index,destination);
            }
            None => {
                if let Ok(t)=subcraft_image::io::decode_bitmap(e.thumb,Some("thumb.png")) {
                    session.set_brush_alpha(&adjusted_alpha(&name,t));
                }
                self.queue_to(index,Destination::capture_tip(session).with_placeholder(Placeholder::Tip));
            }
        }
        self.requested=Some(started);
        log::info!("brush timing: {name} selected (thumbnail brush) in {:.1} ms",started.elapsed().as_secs_f64()*1000.0);
    }
    /// Use an image file (imported / Mine) as the stamp brush. `thumb` (the Shelf thumbnail)
    /// is the brush until the full file is loaded.
    pub fn start_stamp(&mut self, ctx: &egui::Context, session: &mut subcraft_app::Session, name: String, kind: Kind, source: Source, stamp: subcraft_paint::StampKind, thumb: Option<Arc<Bitmap>>) {
        let started=web_time::Instant::now();
        if self.use_ready(session,&name,stamp) {return;}
        let mut destination=Destination::capture_stamp(session,stamp);
        if let Some(t)=thumb {
            let t=if let Source::File(p)=&source {convert_for(p,(*t).clone())} else {(*t).clone()};
            if let Ok(prepared)=subcraft_app::stamp_brush::prepare_stamp(&name,t) {destination=destination.with_placeholder(Placeholder::Stamp(session.set_prepared_stamp(stamp,prepared)));}
        }
        self.start(ctx,name.clone(),kind,source,false); self.destination=Some(destination); self.requested=Some(started);
        log::info!("brush timing: {name} selected (thumbnail brush) in {:.1} ms",started.elapsed().as_secs_f64()*1000.0);
    }
    /// An already encoded full-resolution stamp: becomes the brush at once.
    fn use_ready(&mut self, session: &mut subcraft_app::Session, name: &str, kind: subcraft_paint::StampKind) -> bool {
        let Some(i)=self.ready.iter().position(|(n,k,_)|n==name&&*k==kind) else {return false};
        let entry=self.ready.remove(i); session.set_prepared_stamp(kind,entry.2.clone()); self.ready.push(entry);
        self.cancel_apply();
        log::info!("brush timing: {name} selected (full resolution, cached)");
        true
    }
    /// The brush is waiting for this image's full resolution: name and progress (0..1).
    pub fn pending_brush(&self) -> Option<(&str,f32)> {
        let d=self.destination.as_ref()?;
        (d.stamp_kind().is_some()||self.kind==Some(Kind::Alpha)).then(||(self.name.as_str(),self.apply.as_ref().map_or(0.0,ApplyTask::progress)))
    }
    /// Make the image shown in the preview window the stamp brush (with its adjustments).
    pub fn stamp_from_preview(&mut self, session: &subcraft_app::Session, stamp: subcraft_paint::StampKind) {
        self.destination=Some(subcraft_app::asset_apply::Destination::capture_stamp(session,stamp));self.apply=None;
    }
    fn queue_to(&mut self, index: usize, destination: Destination) {
        self.requested=Some(web_time::Instant::now());
        if self.builtin==Some(index)&&self.source.is_some()&&self.loader.is_none() {self.destination=Some(destination);self.apply=None;return;}
        if ENTRIES.get(index).is_none() {return;} self.cancel_apply();self.queued=Some(index);self.destination=Some(destination);
    }
    pub fn start(&mut self, ctx: &egui::Context, name: String, kind: Kind, source: Source, open: bool) {
        self.cancel_apply(); self.open=open;self.name=name;self.kind=Some(kind);self.settings=Settings::default();self.tiles=1;
        self.builtin=match &source {Source::Builtin(i)=>Some(*i),_=>None};
        self.source=None;self.image=None;self.example=None;self.job=None;self.error=None;self.dirty=true;
        let (tx,rx)=channel();self.loader=Some(rx); let ctx=ctx.clone();
        subcraft_core::task::spawn(move || {
            let result=match source {
                Source::Builtin(i)=>subcraft_assets::starter::encoded(i).ok_or_else(||"Unknown starter image".to_string()).and_then(decode),
                Source::File(p)=>load_file(&p),
                Source::Bitmap(b)=>Ok(b),
            }; let _=tx.send(result);ctx.request_repaint();
        });
    }
    pub fn apply_to(&mut self, session: &subcraft_app::Session, channel: Option<(subcraft_core::LayerId,subcraft_core::ChannelKind)>) {
        self.destination=Some(subcraft_app::asset_apply::Destination::capture(session,channel));self.apply=None;
    }
    fn tick(&mut self, state: &mut UiState, ctx: &egui::Context) {
        if let Some(index)=self.queued.take() {
            let destination=self.destination.take(); let e=&ENTRIES[index];
            self.start(ctx,format!("Starter: {}",e.name),e.kind,Source::Builtin(index),false); self.destination=destination;
        }
        let result=self.loader.as_ref().and_then(|rx|match rx.try_recv() {Ok(value)=>Some(value),Err(TryRecvError::Empty)=>None,Err(TryRecvError::Disconnected)=>Some(Err("Image loader stopped; select the image again to retry".into()))});
        if let Some(result)=result {
            self.loader=None;
            match result {Ok(b)=>{self.source=Some(b);self.dirty=true;},Err(e)=>{log::warn!("Asset image: {e}");state.set_status(e.clone(),ctx.input(|i|i.time));self.error=Some(e);self.cancel_apply();self.open=true;}}
        }
        if let (Some(source),Some(kind))=(&self.source,self.kind) {
            if self.dirty {
                self.job=ImageJob::new(source.clone(),kind,self.settings,256).ok(); self.dirty=false;
            }
            if self.destination.is_some()&&self.apply.is_none() {
                let stamp=self.destination.as_ref().is_some_and(|d|d.stamp_kind().is_some());
                match ApplyTask::spawn(ctx,source.clone(),kind,self.settings,stamp,self.name.clone()) {
                    Ok(task)=>self.apply=Some(task),Err(e)=>{self.error=Some(e);self.cancel_apply();}
                }
            }
        }
        if let Some(job)=&mut self.job {
            job.step(16384);
            if job.is_done() {
                let bitmap=self.job.take().unwrap().finish().unwrap();
                if self.kind!=Some(Kind::Texture) { self.example=Some(texture(ctx,"asset-example",&subcraft_assets::preview::example(&bitmap,self.kind.unwrap()))); }
                self.image=Some(texture(ctx,"asset-preview",&bitmap));
            }
        }
        let finished=self.apply.as_ref().and_then(|t|match t.rx.try_recv() {Ok(r)=>Some(r),Err(TryRecvError::Empty)=>None,Err(TryRecvError::Disconnected)=>Some(Err("Image worker stopped; select the image again to retry".into()))});
        if let Some(result)=finished {
            self.apply=None;
            let kind=self.kind.unwrap();
            let destination=self.destination.take().unwrap(); let stamp_kind=destination.stamp_kind();
            let mut saved_alpha=None;
            let result=result.and_then(|applied|match applied {
                Applied::Stamp(prepared)=>{
                    if self.settings==Settings::default() && let Some(k)=stamp_kind {
                        self.ready.retain(|(n,kk,_)|!(n==&self.name&&*kk==k)); self.ready.push((self.name.clone(),k,prepared.clone()));
                        if self.ready.len()>READY_STAMPS {self.ready.remove(0);}
                    }
                    destination.apply_prepared_stamp(&mut state.session,prepared)
                }
                Applied::Image(bitmap)=>{
                    if kind==Kind::Alpha {saved_alpha=Some(adjusted_alpha(&self.name,bitmap.clone()));}
                    destination.apply(&mut state.session,&self.name,kind,bitmap)
                }
            });
            match result {
                Ok(())=>{
                    if stamp_kind.is_some() {state.tool=Tool::Brush;}
                    if let Some(a)=saved_alpha.filter(|a|state.session.brush.alpha_name==a.name) {state.library.alphas.retain(|old|old.id!=a.id);state.library.alphas.push(a);state.tool=Tool::Brush;}
                    if let Some(t)=self.requested.take() {log::info!("brush timing: {} full resolution ready {:.0} ms after the click",self.name,t.elapsed().as_secs_f64()*1000.0);}
                    state.recovery.autosaver.request_soon();state.set_status(format!("Applied {}",self.name),ctx.input(|i|i.time));
                }
                Err(e)=>{log::warn!("Asset application: {e}");state.set_status(e.clone(),ctx.input(|i|i.time));self.error=Some(e);self.open=true;}
            }
            // The dock/viewport was drawn before this frame's job completion.
            ctx.request_repaint();
        }
        if self.destination.is_some()&&(self.loader.is_some()||self.apply.is_some()) {state.set_status(format!("Loading full resolution of {}…",self.name),ctx.input(|i|i.time));}
        if self.job.is_some() {ctx.request_repaint();}
        else if self.loader.is_some()||self.apply.is_some() {ctx.request_repaint_after(std::time::Duration::from_millis(100));}
    }
    fn window(&mut self, state: &UiState, ctx: &egui::Context) {
        if !self.open {return;} let mut open=self.open;
        crate::widgets::window(ctx, "Texture / alpha preview").open(&mut open).default_width(690.0).default_height(620.0).vscroll(true).show(ctx,|ui| {
            ui.heading(&self.name);
            if let Some(error)=&self.error {ui.add(egui::Label::new(egui::RichText::new(error).color(crate::theme::on_panel(Color32::LIGHT_RED))).wrap());}
            if self.loader.is_some() {ui.spinner();ui.label("Loading full image…");}
            ui.horizontal(|ui| {
                ui.label("Tiles");for n in 1..=3 {ui.selectable_value(&mut self.tiles,n,format!("{n}×{n}"));}
                if self.kind==Some(Kind::Normal) {ui.label(RichText::new("tileable · data map").small().color(crate::theme::ACCENT()));}
            });
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    let side=ui.available_width().min(320.0);let (rect,_)=ui.allocate_exact_size(egui::vec2(side,side),egui::Sense::hover());
                    ui.painter().rect_filled(rect,0.0,crate::theme::BG_0());
                    if let Some(t)=&self.image {
                        let side=side/self.tiles.max(1) as f32;
                        for y in 0..self.tiles.max(1) {for x in 0..self.tiles.max(1) {
                            ui.painter().image(t.id(),egui::Rect::from_min_size(rect.min+egui::vec2(x as f32*side,y as f32*side),egui::vec2(side,side)),egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.0,1.0)),Color32::WHITE);
                        }}
                    }
                    if let Some(t)=&self.example {ui.label(if self.kind==Some(Kind::Normal) {"Lit preview"} else {"Sample stroke"});ui.image((t.id(),t.size_vec2()));}
                });
                ui.vertical(|ui| {ui.set_width(270.0);
                    let before=self.settings;
                    ui.add_enabled_ui(!self.is_applying(),|ui| {
                        if self.kind==Some(Kind::Normal) {
                            ui.add(egui::Slider::new(&mut self.settings.strength,0.0..=4.0).text("Normal strength"));
                            ui.checkbox(&mut self.settings.flip_green,"Flip green (OpenGL ↔ DirectX)");
                        } else {
                            ui.add(egui::Slider::new(&mut self.settings.brightness,-1.0..=1.0).text("Brightness"));
                            ui.add(egui::Slider::new(&mut self.settings.contrast,0.0..=4.0).text("Contrast"));
                        }
                        ui.checkbox(&mut self.settings.invert,if self.kind==Some(Kind::Normal) {"Invert bumps (X/Y)"} else {"Invert"});
                        ui.add(egui::Slider::new(&mut self.settings.scale,0.1..=10.0).logarithmic(true).text("Scale"));
                        ui.add(egui::Slider::new(&mut self.settings.rotation,-180.0..=180.0).text("Rotation"));
                        egui::ComboBox::from_label("Wrap").selected_text(format!("{:?}",self.settings.wrap)).show_ui(ui,|ui| {for v in [Address::Repeat,Address::Mirror,Address::Clamp] {ui.selectable_value(&mut self.settings.wrap,v,format!("{v:?}"));}});
                        ui.add(egui::Slider::new(&mut self.settings.seamless,0.0..=0.25).text("Make seamless"));
                        ui.small("Edge blend. Inspect 2×2 / 3×3; this softens edge details.");
                        if ui.button("Reset adjustments").clicked() {self.settings=Settings::default();}
                    });
                    self.dirty|=before!=self.settings;
                    ui.separator();
                    if self.is_applying() {
                        ui.add(egui::ProgressBar::new(self.apply.as_ref().map_or(0.0,ApplyTask::progress)).text("Applying image…"));
                        if ui.button("Cancel application").clicked() {self.cancel_apply();}
                    } else if ui.add_enabled(self.source.is_some()&&self.error.is_none(),egui::Button::new(if self.kind==Some(Kind::Alpha) {"Use as brush tip"} else {"Add adjusted fill layer"})).clicked() {self.apply_to(&state.session,None);}
                    if !self.is_applying()&&self.source.is_some()&&self.error.is_none() {
                        match self.kind {
                            Some(Kind::Normal)=>if ui.button("Use as normal-map brush").on_hover_text("Stamp this normal map onto the model: paints the Normal channel of the selected paint layer").clicked() {self.stamp_from_preview(&state.session,subcraft_paint::StampKind::Normal);},
                            Some(Kind::Alpha)|Some(Kind::Texture)=>if ui.button("Use as height brush").on_hover_text("Stamp this grayscale image as height: paints the Height channel of the selected paint layer").clicked() {self.stamp_from_preview(&state.session,subcraft_paint::StampKind::Height);},
                            None=>{}
                        }
                    }
                    ui.small("Adjustments are baked into a copy. Full images load only when selected; the original pack stays unchanged.");
                    if let Some(b)=&self.source {ui.small(format!("{} × {} pixels",b.width,b.height));}
                });
            });
        });self.open=open;
    }
}

fn decode(bytes: &[u8]) -> Result<Arc<Bitmap>,String> {
    let bitmap=subcraft_image::io::decode_bitmap(bytes,Some("starter.png")).map_err(|e|e.to_string())?;
    if bitmap.width!=1024||bitmap.height!=1024 {return Err("Starter image must be 1024 × 1024".into());} Ok(Arc::new(bitmap))
}

fn load_file(path: &std::path::Path) -> Result<Arc<Bitmap>,String> {
    let bitmap=subcraft_image::load_bitmap(path).map_err(|e|e.to_string())?;
    Ok(Arc::new(convert_for(path,bitmap)))
}

/// The conversion a texture's file name asks for (inverted gloss, DirectX normal green...).
fn convert_for(path: &std::path::Path, bitmap: Bitmap) -> Bitmap {
    use subcraft_assets::texture_maps::{Classified,MapConversion,classify};
    let conversion=match classify(path.file_name().and_then(|n|n.to_str()).unwrap_or_default()) {Classified::Map(m)=>m.conversion,_=>MapConversion::None};
    match conversion {MapConversion::None=>bitmap,MapConversion::Invert=>bitmap.map_color(|_,v|1.0-v),_=>bitmap.map_color(|c,v|if c==1 {1.0-v}else{v})}
}

/// A Shelf image used as the brush tip (same id for its thumbnail and full resolution).
fn adjusted_alpha(name: &str, bitmap: Bitmap) -> subcraft_assets::BrushAlpha {
    subcraft_assets::BrushAlpha {id:subcraft_core::AssetId::from_name(&format!("adjusted-alpha/{name}")),name:name.to_string(),bitmap:Some(Arc::new(bitmap)),user_file:None}
}


fn texture(ctx: &egui::Context, name: &str, bitmap: &Bitmap) -> egui::TextureHandle {
    let (w,h,rgba)=bitmap.preview_rgba8(4096);ctx.load_texture(name,egui::ColorImage::from_rgba_unmultiplied([w as usize,h as usize],&rgba),egui::TextureOptions::LINEAR)
}

pub fn show(state: &mut UiState, ctx: &egui::Context) {let mut p=std::mem::take(&mut state.asset_preview);p.tick(state,ctx);p.window(state,ctx);state.asset_preview=p;}

pub fn open(state: &mut UiState, ctx: &egui::Context, index: usize, apply: bool, channel: Option<(subcraft_core::LayerId,subcraft_core::ChannelKind)>) {
    let Some(e)=ENTRIES.get(index) else {return;};
    if state.asset_preview.builtin==Some(index)&&state.asset_preview.source.is_some() {
        state.asset_preview.open=!apply;if apply {state.asset_preview.apply_to(&state.session,channel);}return;
    }
    state.asset_preview.start(ctx,format!("Starter: {}",e.name),e.kind,Source::Builtin(index),!apply);
    if apply {state.asset_preview.apply_to(&state.session,channel);}
}

/// Starter-pack tiles of Shelf type `tab`; `visible` holds the indices the search lets through.
pub fn shelf(state: &mut UiState, ui: &mut egui::Ui, side: f32, tab: crate::panels::shelf::ShelfTab, visible: &std::collections::HashSet<usize>) -> usize {
    use crate::panels::shelf::{ShelfTab,ShelfDrag};
    let kind=match tab {ShelfTab::NormalMaps=>Kind::Normal,ShelfTab::Textures=>Kind::Texture,ShelfTab::Alphas=>Kind::Alpha,_=>return 0};
    let mut shown=0;let mut clicked=None;let mut stamped=None;
    for (i,e) in ENTRIES.iter().enumerate() {
        if e.kind!=kind||!visible.contains(&i) {continue;}
        shown+=1;
        if let std::collections::hash_map::Entry::Vacant(slot)=state.asset_preview.thumbs.entry(i)
            && let Ok(b)=subcraft_image::io::decode_bitmap(e.thumb,Some("thumb.png")) {
                slot.insert(texture(ui.ctx(),&format!("starter-thumb-{i}"),&b));
        }
        let tip=match kind {
            Kind::Normal=>"CC0 Starter pack normal map: click to use it as the normal-map brush, double-click to preview and adjust. Drag it onto Normal, the model or a layer.",
            Kind::Alpha=>"CC0 Starter pack alpha: click to use it as the brush tip, double-click to preview and adjust (or use it as a height brush).",
            Kind::Texture=>"CC0 Starter pack: click to preview and adjust.",
        };
        let selected=match kind {
            Kind::Normal=>state.session.brush.material.stamp.as_ref().is_some_and(|s|s.kind==subcraft_paint::StampKind::Normal&&s.name==e.name),
            Kind::Alpha=>state.session.brush.material.stamp.is_none()&&state.session.brush.alpha_name.strip_prefix("Starter: ")==Some(e.name),
            Kind::Texture=>false,
        };
        let resp=crate::panels::shelf::tile(ui,side,state.asset_preview.thumbs.get(&i),e.name,selected,tip);
        if kind==Kind::Normal {resp.dnd_set_drag_payload(ShelfDrag::Starter(i));}
        if kind==Kind::Texture {if resp.clicked() {clicked=Some(i);}}
        else if resp.double_clicked() {clicked=Some(i);} else if resp.clicked() {stamped=Some(i);}
    }
    if let Some(i)=clicked {open(state,ui.ctx(),i,false,None);}
    if let Some(i)=stamped {
        let stamp=(kind==Kind::Normal).then_some(subcraft_paint::StampKind::Normal);
        let UiState{asset_preview,session,..}=state; asset_preview.select_starter(session,i,stamp);
        state.tool=Tool::Brush; state.show_selected_brush(None);
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup(kind: Kind)->(UiState,egui::Context) {
        let mut state=UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(2.0),128),None);
        let p=&mut state.asset_preview;p.open=true;p.name="Starter: Test".into();p.kind=Some(kind);p.tiles=1;
        p.source=Some(Arc::new(if kind==Kind::Normal {Bitmap::new_u8(1,1,3,vec![128,190,230])}else{Bitmap::new_u8(1,1,1,vec![190])}));p.dirty=true;
        let ctx=egui::Context::default();crate::theme::apply(&ctx);(state,ctx)
    }
    fn frame(s: &mut UiState,c: &egui::Context,events: Vec<egui::Event>)->egui::FullOutput {
        let mut output=c.run_ui(egui::RawInput{screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1100.0,900.0))),events,..Default::default()},|ui|show(s,ui.ctx()));output.textures_delta.clear();output
    }
    fn at(output: &egui::FullOutput,label: &str)->egui::Pos2 {
        output.shapes.iter().find_map(|s|match &s.shape {egui::Shape::Text(t) if t.galley.text()==label=>Some(t.pos+t.galley.size()*0.5),_=>None}).unwrap_or_else(||panic!("missing {label}; visible labels: {:?}",output.shapes.iter().filter_map(|s|if let egui::Shape::Text(t)=&s.shape {Some(t.galley.text())}else{None}).collect::<Vec<_>>()))
    }
    fn click(s: &mut UiState,c: &egui::Context,p: egui::Pos2) {
        for pressed in [true,false] {frame(s,c,vec![egui::Event::PointerMoved(p),egui::Event::PointerButton{pos:p,button:egui::PointerButton::Primary,pressed,modifiers:egui::Modifiers::NONE}]);}
    }
    /// Run frames until `done` (worker threads finish in the background), at most ~60 s.
    fn settle(s: &mut UiState,c: &egui::Context,done: impl Fn(&UiState)->bool)->egui::FullOutput {
        let start=std::time::Instant::now();
        loop {
            let out=frame(s,c,vec![]);
            if done(s) {return out;}
            assert!(start.elapsed().as_secs()<60,"background load did not finish");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    fn bolted_armor()->usize {ENTRIES.iter().position(|e|e.kind==Kind::Normal&&e.name=="Bolted Armor").unwrap()}
    fn stamp_width(s: &UiState)->u32 {s.session.brush_stamp().unwrap().1.width}

    #[test]
    fn starter_normal_click_is_the_brush_at_once_then_full_resolution_arrives_in_the_background() {
        let (mut s,c)=setup(Kind::Normal);s.asset_preview=Default::default();
        let t=std::time::Instant::now();
        let UiState{asset_preview,session,..}=&mut s; asset_preview.select_starter(session,bolted_armor(),Some(subcraft_paint::StampKind::Normal));
        let click=t.elapsed();
        assert!(click.as_millis()<250,"the click itself must not decode the full image ({click:?})");
        assert_eq!(stamp_width(&s),96,"the embedded thumbnail is the brush in the same frame");
        assert_eq!(s.session.brush.material.stamp.as_ref().unwrap().name,"Bolted Armor");
        assert!(s.asset_preview.pending_brush().is_some());
        // The user adjusts the stamp while the full image loads.
        s.session.brush.material.stamp.as_mut().unwrap().strength=1.7;
        // Frames keep running (no UI blocking) until the worker delivers.
        settle(&mut s,&c,|s|s.asset_preview.pending_brush().is_none());
        assert_eq!(stamp_width(&s),1024,"full resolution swapped in");
        assert_eq!(s.session.brush.material.stamp.as_ref().unwrap().strength,1.7,"adjustment kept");
        assert_eq!(s.tool,Tool::Brush);
        // Choosing it again uses the encoded full-resolution copy: no load at all.
        let UiState{asset_preview,session,..}=&mut s; session.clear_brush_stamp(); asset_preview.select_starter(session,bolted_armor(),Some(subcraft_paint::StampKind::Normal));
        assert_eq!(stamp_width(&s),1024);
        assert!(s.asset_preview.pending_brush().is_none());
    }

    #[test]
    fn a_late_full_image_never_replaces_a_brush_chosen_meanwhile() {
        let (mut s,c)=setup(Kind::Normal);s.asset_preview=Default::default();
        let UiState{asset_preview,session,..}=&mut s; asset_preview.select_starter(session,bolted_armor(),Some(subcraft_paint::StampKind::Normal));
        // Before the load finishes the user picks a tip from the Shelf.
        let tip=s.library.alphas.iter().find(|a|a.bitmap.is_some()).cloned().unwrap();
        s.select_shelf_brush(subcraft_app::brush_preset::ShelfBrush::Tip(&tip),0.0);
        assert!(s.asset_preview.pending_brush().is_none(),"the pending load is canceled");
        for _ in 0..20 {frame(&mut s,&c,vec![]);std::thread::sleep(std::time::Duration::from_millis(5));}
        assert!(s.session.brush.material.stamp.is_none(),"the tip stays the brush");
        assert_eq!(s.session.brush.alpha_name,tip.name);
        assert!(s.is_selected_shelf_brush(&tip.name));
    }

    #[test]
    fn starter_alpha_click_sets_the_thumbnail_tip_then_the_full_image() {
        let (mut s,c)=setup(Kind::Alpha);s.asset_preview=Default::default();
        let i=ENTRIES.iter().position(|e|e.kind==Kind::Alpha).unwrap();
        let UiState{asset_preview,session,..}=&mut s; asset_preview.select_starter(session,i,None);
        let name=format!("Starter: {}",ENTRIES[i].name);
        assert_eq!(s.session.brush.alpha_name,name);
        assert_eq!(s.session.brush.alpha.as_ref().unwrap().width,96,"thumbnail tip at once");
        settle(&mut s,&c,|s|s.asset_preview.pending_brush().is_none());
        assert_eq!(s.session.brush.alpha.as_ref().unwrap().width,1024);
        assert!(s.library.alphas.iter().any(|a|a.name==name));
    }

    #[test]
    fn shelf_selection_highlights_the_chosen_brush_only() {
        let (mut s,_)=setup(Kind::Alpha);
        let tips: Vec<_>=s.library.alphas.iter().filter(|a|a.bitmap.is_some()).take(2).cloned().collect();
        s.select_shelf_brush(subcraft_app::brush_preset::ShelfBrush::Tip(&tips[0]),0.0);
        assert!(s.open_brush_editor&&s.brush_scroll_top,"Properties is asked to show the brush, scrolled to its preview");
        assert!(s.is_selected_shelf_brush(&tips[0].name));
        assert!(!s.is_selected_shelf_brush(&tips[1].name));
        // A stamp brush: no tip tile stays highlighted.
        s.session.set_brush_stamp("Bolts",subcraft_paint::StampKind::Normal,Bitmap::new_u8(1,1,3,vec![128,128,255])).unwrap();
        assert!(!s.is_selected_shelf_brush(&tips[0].name));
        assert_eq!(crate::panels::brush_header::title(&s),tips[0].name);
    }

    #[test]
    fn actual_preview_controls_flip_normal_green_without_modifying_source() {
        let (mut s,c)=setup(Kind::Normal);let source=s.asset_preview.source.clone().unwrap();let original=source.clone();
        for _ in 0..6 {frame(&mut s,&c,vec![]);}let output=frame(&mut s,&c,vec![]);
        let p=at(&output,"Flip green (OpenGL ↔ DirectX)");click(&mut s,&c,p);
        assert!(s.asset_preview.settings.flip_green);assert!(Arc::ptr_eq(&original,s.asset_preview.source.as_ref().unwrap()));assert_eq!(source.texel(0,0,false)[1],190.0/255.0);
        let p=at(&frame(&mut s,&c,vec![]),"3×3");click(&mut s,&c,p);assert_eq!(s.asset_preview.tiles,3);
    }
    #[test]
    fn use_button_sets_adjusted_alpha_and_restart_waits_for_pending_application() {
        let (mut s,c)=setup(Kind::Alpha);for _ in 0..6 {frame(&mut s,&c,vec![]);}let out=frame(&mut s,&c,vec![]);
        let p=at(&out,"Use as brush tip");click(&mut s,&c,p);assert!(s.asset_preview.is_applying());assert!(!crate::updates::can_restart(&s));
        settle(&mut s,&c,|s|!s.asset_preview.is_applying());assert_eq!(s.session.brush.alpha_name,"Starter: Test");assert!(s.library.alphas.iter().any(|a|a.name=="Starter: Test"));assert!(crate::updates::can_restart(&s));
    }
    #[test]
    fn cancel_application_leaves_document_unchanged_and_tileable_filter_uses_metadata() {
        let (mut s,_)=setup(Kind::Normal);let doc=s.session.document().clone();s.asset_preview.queue(&s.session,0,None);assert!(s.asset_preview.is_applying());s.asset_preview.cancel_apply();assert!(!s.asset_preview.is_applying());assert_eq!(s.session.document(),&doc);
        let filter=crate::panels::shelf_filter::ShelfFilter {query:"brick".into(),chip:crate::panels::shelf_filter::Chip::Tileable};assert!(filter.matches_tagged("Brick",Some("Starter pack"),false,true));assert!(!filter.matches_tagged("Brick",Some("Starter pack"),false,false));
    }
    #[test]
    fn completing_a_job_requests_the_frame_that_displays_the_changed_document() {
        let (mut s,c)=setup(Kind::Normal);s.asset_preview.open=false;s.asset_preview.dirty=false;
        s.asset_preview.source=Some(Arc::new(Bitmap::new_u8(256,256,3,vec![180;256*256*3])));
        let render=|s: &mut UiState| {let mut o=c.run_ui(egui::RawInput::default(),|ui| {ui.label(&s.session.selected_layer().unwrap().name);show(s,ui.ctx());});o.textures_delta.clear();o};
        for _ in 0..3 {render(&mut s);}s.asset_preview.apply_to(&s.session,None);
        let start=std::time::Instant::now();
        let output=loop {let o=render(&mut s);if !s.asset_preview.is_applying() {break o;}assert!(start.elapsed().as_secs()<60);std::thread::sleep(std::time::Duration::from_millis(5));};assert_eq!(s.session.selected_layer().unwrap().name,"Starter: Test");
        assert_eq!(output.viewport_output[&egui::ViewportId::ROOT].repaint_delay,std::time::Duration::ZERO);
        let next=render(&mut s);assert!(next.shapes.iter().any(|v|matches!(&v.shape,egui::Shape::Text(t) if t.galley.text()=="Starter: Test")));
    }
}
