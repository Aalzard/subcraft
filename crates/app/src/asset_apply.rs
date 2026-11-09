//! Applying adjusted shelf images to the captured project/layer, after asynchronous loading.
use std::sync::Arc;
use subcraft_assets::{alphas::BrushAlpha, starter::Kind};
use subcraft_core::{AssetId, ChannelKind, LayerId, ResourceId};
use subcraft_doc::{FillChannel, FillSource, Layer, LayerContent, Resource, ResourceKind};
use subcraft_image::Bitmap;
use crate::{Command, Selection, Session};

/// A low-resolution stand-in that is already the active brush while the full image loads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placeholder {
    /// The stamp brush uses this (thumbnail) resource.
    Stamp(ResourceId),
    /// The brush tip is a thumbnail of the image with the same name.
    Tip,
}

pub struct Destination { scene: Arc<subcraft_mesh::Mesh>, selection: Selection, channel: Option<(LayerId,ChannelKind)>, brush_channel: Option<ChannelKind>, stamp: Option<subcraft_paint::StampKind>, placeholder: Option<Placeholder> }
impl Destination {
    pub fn capture(session: &Session, channel: Option<(LayerId,ChannelKind)>) -> Self { Self { scene:session.scene().mesh.clone(),selection:session.selection(),channel,brush_channel:None,stamp:None,placeholder:None } }
    /// The brush tip (brush alpha); tool state only.
    pub fn capture_tip(session: &Session) -> Self { Self::capture(session,None) }
    /// The full image replaces `placeholder` only while the brush still uses it.
    pub fn with_placeholder(mut self, placeholder: Placeholder) -> Self { self.placeholder=Some(placeholder); self }
    pub fn stamp_kind(&self) -> Option<subcraft_paint::StampKind> { self.stamp }
    /// Finish a stamp destination with an image encoded off the UI thread. With a placeholder,
    /// only swaps the image in (keeping the user's stamp settings) if it is still the brush.
    pub fn apply_prepared_stamp(self, session: &mut Session, prepared: crate::stamp_brush::PreparedStamp) -> Result<(),String> {
        if !Arc::ptr_eq(&self.scene,&session.scene().mesh) { return Err("Project changed; image application canceled".into()); }
        let kind=self.stamp.ok_or("Not a stamp brush destination")?;
        match self.placeholder {
            Some(Placeholder::Stamp(id)) => { session.upgrade_brush_stamp(id,prepared); }
            _ => { session.set_prepared_stamp(kind,prepared); }
        }
        Ok(())
    }
    /// The brush's image for `channel` (tool state: no layer or document change).
    pub fn capture_brush(session: &Session, channel: ChannelKind) -> Self { Self { brush_channel:Some(channel),..Self::capture(session,None) } }
    /// The active normal-map / height stamp brush (tool state: no layer or document change).
    pub fn capture_stamp(session: &Session, kind: subcraft_paint::StampKind) -> Self { Self { stamp:Some(kind),..Self::capture(session,None) } }
    pub fn is_stamp(&self) -> bool { self.stamp.is_some() }
    pub fn apply(self, session: &mut Session, name: &str, kind: Kind, bitmap: Bitmap) -> Result<(),String> {
        if !Arc::ptr_eq(&self.scene,&session.scene().mesh) { return Err("Project changed; image application canceled".into()); }
        if let Some(stamp)=self.stamp { return session.set_brush_stamp(name,stamp,bitmap); }
        if kind==Kind::Alpha {
            // A thumbnail tip is replaced only while it is still the brush's tip.
            if self.placeholder==Some(Placeholder::Tip) && (session.brush.alpha_name!=name || session.brush.material.stamp.is_some()) { return Ok(()); }
            let alpha=BrushAlpha { id:AssetId::from_name(&format!("adjusted-alpha/{name}")),name:name.into(),bitmap:Some(Arc::new(bitmap)),user_file:None };
            session.set_brush_alpha(&alpha); return Ok(());
        }
        if let Some(channel)=self.brush_channel { return session.set_brush_channel_bitmap(channel,name,bitmap); }
        let ts=session.document().texture_set(self.selection.texture_set).ok_or("Texture set no longer exists")?;
        let channel=if kind==Kind::Normal {ChannelKind::Normal} else {match subcraft_assets::texture_maps::classify(name) {subcraft_assets::texture_maps::Classified::Map(m)=>m.channel,_=>ChannelKind::BaseColor}};
        if !ts.has_channel(channel) { return Err("Texture set does not contain that channel".into()); }
        if let Some((id,ch))=self.channel {
            if ch!=channel { return Err(format!("Drop this image on {}",channel.display_name())); }
            if !ts.find(id).is_some_and(|l| matches!(l.content,LayerContent::Fill(_))) { return Err("Target fill layer no longer exists".into()); }
        } else if self.selection.layer.is_some_and(|id| ts.find(id).is_none()) { return Err("Target layer no longer exists".into()); }
        let strength=self.channel.and_then(|(id,ch)|ts.find(id).and_then(|l|if let LayerContent::Fill(f)=&l.content {f.channels.get(&ch).map(|c|c.strength)} else {None})).unwrap_or(1.0);
        let mut layer=Layer::new_fill(name,ts);
        let current=session.selection();
        let (encoded,ext)=subcraft_image::encode_bitmap(&bitmap).map_err(|e|e.to_string())?;
        let existing=session.resources().iter().find(|r|r.kind==ResourceKind::Texture&&r.encoded.as_deref().is_some_and(|e|*e==encoded)).map(|r|r.id);
        let id=existing.unwrap_or_else(ResourceId::new);
        if existing.is_none() {session.resources_mut().insert(Resource { id,name:format!("{name}.{ext}"),kind:ResourceKind::Texture,bitmap:Arc::new(bitmap),source_path:None,encoded:Some(Arc::new(encoded)) });}
        let fill=FillChannel {strength,..FillChannel::new(FillSource::Bitmap{resource:id})};
        session.execute(Command::SelectTextureSet(self.selection.texture_set)).map_err(|e|e.to_string())?;
        let result=if let Some((target,_))=self.channel { session.execute(Command::SetFillChannel {layer:target,channel,fill}) }
        else {
            let LayerContent::Fill(f)=&mut layer.content else { unreachable!() };
            for c in f.channels.values_mut() {c.enabled=false;} f.channels.insert(channel,fill);
            session.execute(Command::SelectLayer(self.selection.layer)).map_err(|e|e.to_string())?;
            session.execute(Command::InsertLayer {layer:Box::new(layer),label:format!("Apply {name}")})
        };
        if current!=self.selection { let _=session.execute(Command::SelectTextureSet(current.texture_set)); let _=session.execute(Command::SelectLayer(current.layer)); }
        result.map_err(|e|e.to_string())
    }
}
