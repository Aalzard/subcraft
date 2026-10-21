//! Shelf image loading and preview presentation. Image math lives in assets; project edits in app.
use std::{collections::HashMap, path::PathBuf, sync::{Arc,mpsc::{Receiver,TryRecvError,channel}}};
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
    destination: Option<subcraft_app::asset_apply::Destination>, apply: Option<ImageJob>,
    queued: Option<usize>,
    builtin: Option<usize>,
}
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
    fn queue_to(&mut self, index: usize, destination: subcraft_app::asset_apply::Destination) {
        if self.builtin==Some(index)&&self.source.is_some()&&self.loader.is_none() {self.destination=Some(destination);self.apply=None;return;}
        if ENTRIES.get(index).is_none() {return;} self.cancel_apply();self.queued=Some(index);self.destination=Some(destination);
    }
    pub fn start(&mut self, ctx: &egui::Context, name: String, kind: Kind, source: Source, open: bool) {
        self.cancel_apply(); self.open=open;self.name=name;self.kind=Some(kind);self.settings=Settings::default();self.tiles=1;
        self.builtin=match &source {Source::Builtin(i)=>Some(*i),_=>None};
        self.source=None;self.image=None;self.example=None;self.job=None;self.error=None;self.dirty=true;
        let (tx,rx)=channel();self.loader=Some(rx); let ctx=ctx.clone();
        #[cfg(not(target_arch="wasm32"))]
        subcraft_core::task::spawn(move || {
            let result=match source {
                Source::Builtin(i)=>subcraft_assets::starter::encoded(i).ok_or_else(||"Unknown starter image".to_string()).and_then(decode),
                Source::File(p)=>load_file(&p),
                Source::Bitmap(b)=>Ok(b),
            }; let _=tx.send(result);ctx.request_repaint();
        });
        #[cfg(target_arch="wasm32")]
        wasm_bindgen_futures::spawn_local(async move {
            let result=match source {
                Source::Builtin(i)=>fetch(i).await.and_then(|b|decode(&b)),
                Source::File(p)=>load_file(&p),
                Source::Bitmap(b)=>Ok(b),
            };let _=tx.send(result);ctx.request_repaint();
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
                match ImageJob::new(source.clone(),kind,self.settings,source.width.max(source.height).min(4096)) {
                    Ok(job)=>self.apply=Some(job),Err(e)=>{self.error=Some(e.into());self.cancel_apply();}
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
        if let Some(job)=&mut self.apply {
            job.step(16384);
            state.set_status(format!("Applying {}… {:.0}%",self.name,job.progress()*100.0),ctx.input(|i|i.time));
            if job.is_done() {
                let bitmap=self.apply.take().unwrap().finish().unwrap();
                let kind=self.kind.unwrap();
                let saved_alpha=(kind==Kind::Alpha).then(||subcraft_assets::BrushAlpha {id:subcraft_core::AssetId::from_name(&format!("adjusted-alpha/{}",self.name)),name:self.name.clone(),bitmap:Some(Arc::new(bitmap.clone())),user_file:None});
                let result=self.destination.take().unwrap().apply(&mut state.session,&self.name,kind,bitmap);
                match result {
                    Ok(())=>{
                        if let Some(a)=saved_alpha {state.library.alphas.retain(|old|old.id!=a.id);state.library.alphas.push(a);state.tool=Tool::Brush;}
                        state.recovery.autosaver.request_soon();state.set_status(format!("Applied {}",self.name),ctx.input(|i|i.time));
                    }
                    Err(e)=>{log::warn!("Asset application: {e}");state.set_status(e.clone(),ctx.input(|i|i.time));self.error=Some(e);self.open=true;}
                }
                // The dock/viewport was drawn before this frame's job completion.
                ctx.request_repaint();
            }
        }
        if self.destination.is_some()&&self.loader.is_some() {state.set_status(format!("Loading {}…",self.name),ctx.input(|i|i.time));}
        if self.job.is_some()||self.apply.is_some() {ctx.request_repaint();}
        else if self.loader.is_some() {ctx.request_repaint_after(std::time::Duration::from_millis(100));}
    }
    fn window(&mut self, state: &UiState, ctx: &egui::Context) {
        if !self.open {return;} let mut open=self.open;
        egui::Window::new("Texture / alpha preview").open(&mut open).default_width(690.0).default_height(620.0).vscroll(true).show(ctx,|ui| {
            ui.heading(&self.name);
            if let Some(error)=&self.error {ui.colored_label(Color32::LIGHT_RED,error);}
            if self.loader.is_some() {ui.spinner();ui.label("Loading full image…");}
            ui.horizontal(|ui| {
                ui.label("Tiles");for n in 1..=3 {ui.selectable_value(&mut self.tiles,n,format!("{n}×{n}"));}
                if self.kind==Some(Kind::Normal) {ui.label(RichText::new("tileable · data map").small().color(crate::theme::ACCENT));}
            });
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    let side=ui.available_width().min(320.0);let (rect,_)=ui.allocate_exact_size(egui::vec2(side,side),egui::Sense::hover());
                    ui.painter().rect_filled(rect,0.0,crate::theme::BG_0);
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
                        ui.add(egui::ProgressBar::new(self.apply.as_ref().map_or(0.0,|j|j.progress())).text("Applying image…"));
                        if ui.button("Cancel application").clicked() {self.cancel_apply();}
                    } else if ui.add_enabled(self.source.is_some()&&self.error.is_none(),egui::Button::new(if self.kind==Some(Kind::Alpha) {"Use as brush tip"} else {"Add adjusted fill layer"})).clicked() {self.apply_to(&state.session,None);}
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
    use subcraft_assets::texture_maps::{Classified,MapConversion,classify};
    let bitmap=subcraft_image::load_bitmap(path).map_err(|e|e.to_string())?;
    let conversion=match classify(path.file_name().and_then(|n|n.to_str()).unwrap_or_default()) {Classified::Map(m)=>m.conversion,_=>MapConversion::None};
    Ok(Arc::new(match conversion {MapConversion::None=>bitmap,MapConversion::Invert=>bitmap.map_color(|_,v|1.0-v),_=>bitmap.map_color(|c,v|if c==1 {1.0-v}else{v})}))
}

#[cfg(target_arch="wasm32")]
async fn fetch(index: usize) -> Result<Vec<u8>,String> {
    use wasm_bindgen::JsCast;
    let entry=ENTRIES.get(index).ok_or("Unknown starter image")?;
    let url=format!("starter-pack/{}",entry.file);
    let response=wasm_bindgen_futures::JsFuture::from(web_sys::window().ok_or("Browser window unavailable")?.fetch_with_str(&url)).await.map_err(|_|"Starter image download failed; try selecting it again")?;
    let response=response.dyn_into::<web_sys::Response>().map_err(|_|"Invalid image response")?;
    if !response.ok() {return Err(format!("Image download returned HTTP {}",response.status()));}
    let buffer=wasm_bindgen_futures::JsFuture::from(response.array_buffer().map_err(|_|"Cannot read image download")?).await.map_err(|_|"Cannot read image download")?;
    let bytes=js_sys::Uint8Array::new(&buffer).to_vec();
    if bytes.len()!=entry.bytes {return Err("Image download has an unexpected size; reload the page and retry".into());} Ok(bytes)
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

pub fn shelf(state: &mut UiState, ui: &mut egui::Ui, side: f32) -> usize {
    use crate::panels::shelf::{ShelfTab,ShelfDrag};
    let kind=match state.shelf.tab {ShelfTab::Textures=>Kind::Normal,ShelfTab::Brushes=>Kind::Alpha,_=>return 0};
    let mut shown=0;let mut clicked=None;
    for (i,e) in ENTRIES.iter().enumerate() {
        if e.kind!=kind||!state.shelf.filter.matches_tagged(e.name,Some("Starter pack"),false,e.tileable()) {continue;}
        shown+=1;
        if let std::collections::hash_map::Entry::Vacant(slot)=state.asset_preview.thumbs.entry(i)
            && let Ok(b)=subcraft_image::io::decode_bitmap(e.thumb,Some("thumb.png")) {
                slot.insert(texture(ui.ctx(),&format!("starter-thumb-{i}"),&b));
        }
        let resp=crate::panels::shelf::tile(ui,side,state.asset_preview.thumbs.get(&i),e.name,false,"CC0 Starter pack: click to preview and adjust. Drag normals onto Normal, the model or a layer.");
        if kind==Kind::Normal {resp.dnd_set_drag_payload(ShelfDrag::Starter(i));}
        if resp.clicked() {clicked=Some(i);}
    }
    if let Some(i)=clicked {open(state,ui.ctx(),i,false,None);} shown
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
        for _ in 0..8 {frame(&mut s,&c,vec![]);}assert!(!s.asset_preview.is_applying());assert_eq!(s.session.brush.alpha_name,"Starter: Test");assert!(s.library.alphas.iter().any(|a|a.name=="Starter: Test"));assert!(crate::updates::can_restart(&s));
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
        for _ in 0..3 {render(&mut s);}let output=render(&mut s);
        assert!(!s.asset_preview.is_applying());assert_eq!(s.session.selected_layer().unwrap().name,"Starter: Test");
        assert_eq!(output.viewport_output[&egui::ViewportId::ROOT].repaint_delay,std::time::Duration::ZERO);
        let next=render(&mut s);assert!(next.shapes.iter().any(|v|matches!(&v.shape,egui::Shape::Text(t) if t.galley.text()=="Starter: Test")));
    }
}
