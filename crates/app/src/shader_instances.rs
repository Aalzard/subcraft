//! Shader instances of the project (Painter's Shader Settings): the list, which instance each
//! texture set uses, and the edits of the Shader Settings panel and the texture set's shader menu.
//! They live in the project settings, so an edit marks the project modified like other settings.

use subcraft_core::TextureSetId;
use subcraft_doc::{ShaderAssignment, ShaderInstance, ShaderParams};
use uuid::Uuid;

use crate::Session;

impl Session {
    /// Every shader instance, in creation order. Never empty: a project that made none has the
    /// implicit "Main shader".
    pub fn shader_instances(&self) -> Vec<ShaderInstance> {
        if self.settings.shader_instances.is_empty() { vec![ShaderInstance::main()] } else { self.settings.shader_instances.clone() }
    }

    /// The instance texture set `ts` uses (the first one when it was never assigned, or when its
    /// instance is gone).
    pub fn shader_instance_of(&self, ts: TextureSetId) -> ShaderInstance {
        let list = self.shader_instances();
        let wanted = self.settings.shader_assignments.iter().find(|a| a.texture_set == ts).map(|a| a.instance);
        wanted.and_then(|id| list.iter().find(|i| i.id == id).cloned()).unwrap_or_else(|| list[0].clone())
    }

    /// The shader parameters texture set `ts` is drawn with (in range).
    pub fn shader_params_of(&self, ts: TextureSetId) -> ShaderParams {
        self.shader_instance_of(ts).params.clamped()
    }

    /// The editable list (the implicit Main shader made explicit on the first edit).
    fn instances_mut(&mut self) -> &mut Vec<ShaderInstance> {
        if self.settings.shader_instances.is_empty() {
            self.settings.shader_instances.push(ShaderInstance::main());
        }
        &mut self.settings.shader_instances
    }

    /// New instance with the parameters of `copy_of` (defaults when `None`), named after it with
    /// a unique number. Returns its id.
    pub fn new_shader_instance(&mut self, copy_of: Option<Uuid>) -> Uuid {
        let source = copy_of.and_then(|id| self.shader_instances().into_iter().find(|i| i.id == id));
        let list = self.instances_mut();
        let base = source.as_ref().map_or(subcraft_doc::MAIN_SHADER, |s| s.name.as_str());
        let name = subcraft_doc::shader::unique_name(base, list.iter().map(|i| i.name.as_str()));
        let id = Uuid::new_v4();
        list.push(ShaderInstance { id, name, params: source.map_or_else(ShaderParams::default, |s| s.params) });
        id
    }

    /// Rename an instance; the name is made unique among the others (Painter 12.1). Returns the
    /// name it got, or `None` when there is no such instance.
    pub fn rename_shader_instance(&mut self, id: Uuid, name: &str) -> Option<String> {
        let list = self.instances_mut();
        let i = list.iter().position(|s| s.id == id)?;
        let unique = subcraft_doc::shader::unique_name(name, list.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, s)| s.name.as_str()));
        list[i].name = unique.clone();
        Some(unique)
    }

    /// Make texture set `ts` use instance `id` (several sets may share one).
    pub fn assign_shader_instance(&mut self, ts: TextureSetId, id: Uuid) -> bool {
        if !self.shader_instances().iter().any(|i| i.id == id) || self.document().texture_set(ts).is_none() {
            return false;
        }
        self.instances_mut();
        let assignments = &mut self.settings.shader_assignments;
        assignments.retain(|a| a.texture_set != ts);
        assignments.push(ShaderAssignment { texture_set: ts, instance: id });
        true
    }

    /// Set an instance's parameters (clamped to their ranges).
    pub fn set_shader_params(&mut self, id: Uuid, params: ShaderParams) -> bool {
        let params = params.clamped();
        match self.instances_mut().iter_mut().find(|s| s.id == id) {
            Some(s) if s.params != params => {
                s.params = params;
                true
            }
            _ => false,
        }
    }

    /// Painter's Restore defaults: every parameter of the instance back to the shader's default.
    pub fn restore_shader_defaults(&mut self, id: Uuid) -> bool {
        self.set_shader_params(id, ShaderParams::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_sets() -> Session {
        let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
        let mut doc = s.document().clone();
        let mut second = doc.texture_sets[0].clone();
        second.id = TextureSetId::new();
        second.name = "Glass".into();
        second.material_slot = 1;
        doc.texture_sets.push(second);
        s = Session::from_parts(s.scene().mesh.as_ref().clone(), doc, Default::default(), "test");
        s
    }

    #[test]
    fn every_set_starts_on_the_main_shader_and_saving_nothing_changes_nothing() {
        let s = two_sets();
        let list = s.shader_instances();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Main shader");
        for ts in &s.document().texture_sets {
            assert_eq!(s.shader_instance_of(ts.id).id, list[0].id);
            assert_eq!(s.shader_params_of(ts.id), ShaderParams::default());
        }
        assert!(s.settings().shader_instances.is_empty(), "the implicit instance is not written");
        assert!(!s.is_modified());
    }

    #[test]
    fn instances_are_created_renamed_assigned_edited_and_restored() {
        let mut s = two_sets();
        let (body, glass) = (s.document().texture_sets[0].id, s.document().texture_sets[1].id);
        let main = s.shader_instances()[0].id;
        let glow = s.new_shader_instance(Some(main));
        assert_eq!(s.shader_instances().iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["Main shader", "Main shader (2)"]);
        assert_eq!(s.rename_shader_instance(glow, "Main shader").as_deref(), Some("Main shader (2)"), "names stay unique");
        assert_eq!(s.rename_shader_instance(glow, "Glow").as_deref(), Some("Glow"));
        assert!(s.assign_shader_instance(glass, glow));
        assert!(!s.assign_shader_instance(glass, Uuid::new_v4()), "an unknown instance is refused");
        let bright = ShaderParams { emissive_intensity: 12.0, ao_intensity: 0.2, ..Default::default() };
        assert!(s.set_shader_params(glow, bright));
        assert_eq!(s.shader_params_of(glass), bright);
        assert_eq!(s.shader_params_of(body), ShaderParams::default(), "the other set keeps the Main shader");
        assert!(s.is_modified(), "shader edits are project changes");
        // Out-of-range values never reach the renderer.
        s.set_shader_params(glow, ShaderParams { emissive_intensity: 1e9, ..bright });
        assert_eq!(s.shader_params_of(glass).emissive_intensity, 100.0);
        assert!(s.restore_shader_defaults(glow));
        assert_eq!(s.shader_params_of(glass), ShaderParams::default());
        assert_eq!(s.shader_instance_of(glass).name, "Glow", "restoring keeps the instance and its name");
        // A copy of an instance starts from its parameters.
        s.set_shader_params(glow, bright);
        let copy = s.new_shader_instance(Some(glow));
        assert_eq!(s.shader_instances().iter().find(|i| i.id == copy).unwrap().params, bright);
    }
}
