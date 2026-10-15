//! Pen editing controller. Front ends translate hits/gestures to UV coordinates; this module
//! owns drafts, UUID edits, style, raster jobs, cancellation and history commits.
use glam::Vec2;
use subcraft_core::AnchorId;
use subcraft_doc::{Layer, LayerContent, PenAnchor, PenPath};
use subcraft_paint::pen::PenRasterizer;
use crate::{Command, CommandError, Selection, Session};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle { Anchor, Incoming, Outgoing }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PenHit { pub anchor: AnchorId, pub handle: Handle }

#[derive(Clone)]
struct Edit {
    layer: Layer,
    path: PenPath,
    new: bool,
    resolution: u32,
    scene: std::sync::Arc<subcraft_mesh::Mesh>,
    selection: Selection,
    handle: Option<PenHit>,
    label: &'static str,
}

struct Pending { edit: Edit, raster: PenRasterizer }

#[derive(Default)]
pub struct PenEditor {
    selected: Option<AnchorId>,
    domain: Option<Selection>,
    gesture: Option<Edit>,
    pending: std::collections::VecDeque<Pending>,
    new_on_click: bool,
}

fn error(s: impl Into<String>) -> CommandError { CommandError::Invalid(s.into()) }

fn surface_uv(session: &Session, pick: &crate::Pick) -> Vec2 {
    let mesh = &session.scene().mesh;
    let indices = &mesh.indices[pick.triangle as usize * 3..][..3];
    let (_, offset) = subcraft_mesh::uv_raster::wrap_to_tile([mesh.uvs[indices[0] as usize], mesh.uvs[indices[1] as usize], mesh.uvs[indices[2] as usize]]);
    pick.uv - Vec2::new(offset[0] as f32, offset[1] as f32)
}

fn stored_path(session: &Session) -> Option<&PenPath> {
    let LayerContent::Paint(p) = &session.selected_layer()?.content else { return None; };
    p.path.as_ref()
}

impl PenEditor {
    pub fn selected_anchor(&self) -> Option<AnchorId> { self.selected }
    pub fn is_editing(&self) -> bool { self.gesture.is_some() }
    pub fn is_pending(&self) -> bool { !self.pending.is_empty() }
    pub fn progress(&self) -> f32 { self.pending.front().map_or(1.0, |p| p.raster.progress()) }

    pub fn preview<'a>(&'a self, session: &'a Session) -> Option<&'a PenPath> {
        let edit = self.gesture.as_ref().or_else(|| self.pending.back().map(|p| &p.edit));
        if let Some(edit) = edit && edit.matches_selection(session) && edit.scene_valid(session) { return Some(&edit.path); }
        stored_path(session)
    }

    pub fn cancel(&mut self) {
        self.gesture = None; self.pending.clear(); self.selected = None; self.new_on_click = true;
    }

    /// Undo an uncommitted gesture, leaving earlier queued gestures to finish normally.
    pub fn undo_pending(&mut self, session: &Session) {
        if self.gesture.take().is_none() { self.pending.pop_back(); }
        self.selected = self.preview(session).and_then(|p| p.anchors.last()).map(|a| a.id);
    }

    pub fn new_path(&mut self) { self.finish_path(); }
    /// Finish adding points without discarding a raster job already in progress.
    pub fn finish_path(&mut self) { self.selected = None; self.new_on_click = true; }

    fn edit(&mut self, session: &Session, force_new: bool) -> Result<Edit, CommandError> {
        if session.is_stroking() { return Err(CommandError::StrokeInProgress); }
        if self.pending.back().is_some_and(|p| !p.edit.matches_selection(session)) { return Err(error("wait for the current path to finish drawing")); }
        if !force_new && let Some(pending) = self.pending.back() {
            let mut edit = pending.edit.clone();
            let LayerContent::Paint(p) = &mut edit.layer.content else { unreachable!() };
            p.path = Some(edit.path.clone());
            edit.new = false; edit.handle = None; edit.label = "Edit pen path";
            return Ok(edit);
        }
        if self.domain != Some(session.selection()) {
            self.cancel(); self.domain = Some(session.selection());
            // A selected saved path is immediately editable after a project/selection change.
            self.new_on_click = false;
        }
        if !force_new && let Some(path) = stored_path(session) {
            return Ok(Edit { layer: session.selected_layer().unwrap().clone(), path: path.clone(), new: false, resolution: session.selected_texture_set().resolution, scene: session.scene().mesh.clone(), selection: session.selection(), handle: None, label: "Edit pen path" });
        }
        let ts = session.selected_texture_set(); let brush = &session.brush;
        let values = brush.material.channels.iter().filter(|(k, v)| v.enabled && ts.has_channel(**k)).map(|(&k, v)| (k, v.value)).collect();
        let path = PenPath { anchors: Vec::new(), closed: false, radius: (brush.settings.size * 0.005).clamp(0.00001, 0.5), hardness: brush.settings.hardness.clamp(0.0, 1.0), opacity: brush.settings.stroke_opacity.clamp(0.0, 1.0), values };
        path.validate().map_err(error)?;
        let mut layer = Layer::new_paint("Pen path", ts);
        layer.parts = session.selected_layer().and_then(|l| l.parts.clone());
        Ok(Edit { layer, path, new: true, resolution: ts.resolution, scene: session.scene().mesh.clone(), selection: session.selection(), handle: None, label: "New pen path" })
    }

    pub fn begin(&mut self, session: &Session, uv: Vec2, hit: Option<PenHit>) -> Result<(), CommandError> {
        let force = self.new_on_click && hit.is_none();
        if force && !self.pending.is_empty() { return Err(error("wait for the current path to finish drawing")); }
        if self.pending.len() >= 32 { return Err(error("wait for the queued pen points to finish drawing")); }
        let mut edit = self.edit(session, force)?;
        if let Some(hit) = hit {
            if !edit.path.anchors.iter().any(|a| a.id == hit.anchor) { return Err(error("pen anchor no longer exists")); }
            self.selected = Some(hit.anchor); edit.handle = Some(hit);
        } else {
            if edit.path.closed { return Err(error("open the path or choose New path before adding points")); }
            if edit.path.anchors.len() >= PenPath::MAX_ANCHORS { return Err(error("a path supports at most 256 anchors")); }
            let anchor = PenAnchor::new(uv); let id = anchor.id;
            edit.path.anchors.push(anchor); self.selected = Some(id);
            edit.handle = Some(PenHit { anchor: id, handle: Handle::Outgoing });
        }
        edit.path.validate().map_err(error)?;
        self.new_on_click = false; self.gesture = Some(edit); Ok(())
    }

    pub fn begin_surface(&mut self, session: &mut Session, camera: &crate::Camera, screen: Vec2, hit: Option<PenHit>) -> Result<(), CommandError> {
        let pick = session.pick(camera, screen).ok_or_else(|| error("click the model to place a pen point"))?;
        if pick.material_slot != session.selected_texture_set().material_slot {
            if hit.is_some() { return Err(error("edit this handle in the 2D view")); }
            let id = session.document().texture_set_for_slot(pick.material_slot).ok_or(CommandError::TextureSetNotFound)?.id;
            self.cancel(); session.execute(Command::SelectTextureSet(id))?;
        }
        self.begin(session, surface_uv(session, &pick), hit)
    }

    pub fn drag_surface(&mut self, session: &Session, camera: &crate::Camera, screen: Vec2, separate: bool) {
        if let Some(pick) = session.pick(camera, screen) && pick.material_slot == session.selected_texture_set().material_slot {
            self.drag_to(surface_uv(session, &pick), separate);
        }
    }

    pub fn drag_to(&mut self, uv: Vec2, separate: bool) {
        if !uv.is_finite() || uv.abs().max_element() > 4.0 { return; }
        let Some(edit) = self.gesture.as_mut() else { return; };
        let Some(hit) = edit.handle else { return; };
        let Some(a) = edit.path.anchors.iter_mut().find(|a| a.id == hit.anchor) else { return; };
        match hit.handle {
            Handle::Anchor => a.position = uv,
            Handle::Incoming => { a.incoming = uv - a.position; if !separate { a.outgoing = -a.incoming; } }
            Handle::Outgoing => { a.outgoing = uv - a.position; if !separate { a.incoming = -a.outgoing; } }
        }
    }

    /// Called on release, including when the pointer has left the canvas.
    pub fn release(&mut self, session: &Session) -> Result<(), CommandError> {
        let Some(edit) = self.gesture.take() else { return Ok(()); };
        if self.pending.len() >= 32 { return Err(error("wait for the queued pen edits to finish drawing")); }
        if !edit.matches_selection(session) || !edit.scene_valid(session) { return Err(error("project changed; pen edit canceled")); }
        let expected = match &edit.layer.content { LayerContent::Paint(p) => p.path.as_ref(), _ => None };
        if !edit.new && expected == Some(&edit.path) { return Ok(()); }
        if self.pending.is_empty() && !edit.valid(session) { return Err(error("path changed; pen edit canceled")); }
        let raster = PenRasterizer::new(&edit.path, edit.resolution).map_err(error)?;
        self.pending.push_back(Pending { edit, raster }); Ok(())
    }

    pub fn set_path(&mut self, session: &Session, path: PenPath) -> Result<(), CommandError> {
        path.validate().map_err(error)?;
        let mut edit = self.gesture.take().map(Ok).unwrap_or_else(|| self.edit(session, false))?;
        edit.path = path; edit.handle = None; edit.label = "Pen style";
        self.gesture = Some(edit); Ok(())
    }

    pub fn delete_anchor(&mut self, session: &Session) -> Result<(), CommandError> {
        let Some(id) = self.selected else { return Ok(()); };
        let mut path = self.preview(session).ok_or_else(|| error("select a pen path"))?.clone();
        path.anchors.retain(|a| a.id != id); if path.anchors.len() < 2 { path.closed = false; }
        self.set_path(session, path)?; self.gesture.as_mut().unwrap().label = "Delete pen point";
        self.selected = None; self.release(session)
    }

    pub fn shape_point(&mut self, session: &Session, smooth: bool) -> Result<(), CommandError> {
        let Some(id) = self.selected else { return Ok(()); };
        let mut path = self.preview(session).ok_or_else(|| error("select a pen path"))?.clone();
        let i = path.anchors.iter().position(|a| a.id == id).ok_or_else(|| error("select a pen point"))?;
        let p = path.anchors[i].position;
        let before = if i > 0 { path.anchors[i - 1].position } else if path.closed { path.anchors.last().unwrap().position } else { p };
        let after = path.anchors.get(i + 1).map_or(if path.closed { path.anchors[0].position } else { p }, |a| a.position);
        let tangent = if smooth { (after - before) / 6.0 } else { Vec2::ZERO };
        path.anchors[i].incoming = -tangent; path.anchors[i].outgoing = tangent;
        self.set_path(session, path)?; self.gesture.as_mut().unwrap().label = "Shape pen point";
        self.release(session)
    }

    /// Each frame performs bounded raster work. A pending edit never overwrites changes made
    /// by undo, another tool, project loading or texture-set/layer selection.
    pub fn poll(&mut self, session: &mut Session, budget: usize) -> Result<bool, CommandError> {
        let Some(mut pending) = self.pending.pop_front() else { return Ok(false); };
        if !pending.edit.valid(session) { self.cancel(); return Err(error("path changed; pending pen edit canceled")); }
        pending.raster.step(budget);
        if !pending.raster.is_done() { self.pending.push_front(pending); return Ok(true); }
        let Edit { mut layer, path, new, label, selection, .. } = pending.edit;
        let current = session.selection();
        if !new {
            layer = session.document().texture_set(selection.texture_set).and_then(|ts| ts.find(layer.id)).ok_or(CommandError::LayerNotFound)?.clone();
        }
        let LayerContent::Paint(p) = &mut layer.content else { return Err(error("pen layer must be paint content")); };
        let mut images = pending.raster.finish().unwrap();
        // Reuse byte-identical tiles from the previous cache, keeping path history sparse.
        for (kind, image) in &mut images {
            if let Some(old) = p.channels.get(kind) && old.same_layout(image) {
                for i in 0..image.tile_count() {
                    if !image.tile_changed(old, i) { image.set_slot(i, old.tile(i).cloned()); }
                }
            }
        }
        p.path = Some(path); p.channels = images;
        let id = layer.id;
        if new {
            if current != selection {
                session.execute(Command::SelectTextureSet(selection.texture_set))?;
                session.execute(Command::SelectLayer(selection.layer))?;
            }
            session.execute(Command::InsertLayer { layer: Box::new(layer), label: label.into() })?;
            if current != selection {
                session.execute(Command::SelectTextureSet(current.texture_set))?;
                session.execute(Command::SelectLayer(current.layer))?;
                self.selected = None;
            }
        }
        else { session.execute(Command::ReplaceLayer { layer: id, replacement: Box::new(layer), label: label.into() })?; }
        if current != selection && current.layer != Some(id) { self.selected = None; }
        self.domain = Some(session.selection());
        Ok(!self.pending.is_empty())
    }
}

impl Edit {
    fn matches_selection(&self, session: &Session) -> bool {
        self.selection == session.selection() || (self.selection.texture_set == session.selection().texture_set && session.selection().layer == Some(self.layer.id))
    }
    fn scene_valid(&self, session: &Session) -> bool {
        std::sync::Arc::ptr_eq(&self.scene, &session.scene().mesh)
            && session.document().texture_set(self.selection.texture_set).is_some_and(|ts| ts.resolution == self.resolution)
    }
    fn valid(&self, session: &Session) -> bool {
        if !self.scene_valid(session) { return false; }
        let ts = session.document().texture_set(self.selection.texture_set).unwrap();
        if self.new { return self.selection.layer.is_none_or(|id| ts.find(id).is_some()); }
        let Some(layer) = ts.find(self.layer.id) else { return false; };
        match (&self.layer.content, &layer.content) {
            (LayerContent::Paint(before), LayerContent::Paint(now)) => before.path == now.path,
            _ => false,
        }
    }
}
