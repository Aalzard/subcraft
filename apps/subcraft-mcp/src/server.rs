//! Server state (the open project, the shelf library, the GPU) and the lookups every tool
//! shares: texture sets, layers and effects by id or name.

use std::panic::AssertUnwindSafe;

use serde_json::{Value, json};
use subcraft_app::{Command, Session};
use subcraft_app::assets::{Library, UserLibrary};
use subcraft_core::{EffectId, LayerId, TextureSetId};
use subcraft_doc::{Layer, TextureSet};

use crate::params::{Args, Res};
use crate::render::Gpu;

/// What a tool returns: MCP content items (text and images).
#[derive(Default)]
pub struct Reply {
    pub content: Vec<Value>,
}

impl Reply {
    pub fn json(v: Value) -> Self {
        let mut r = Self::default();
        r.push_json(v);
        r
    }
    pub fn push_json(&mut self, v: Value) {
        let text = serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string());
        self.content.push(json!({ "type": "text", "text": text }));
    }
    pub fn push_png(&mut self, png: &[u8]) {
        self.content.push(json!({ "type": "image", "mimeType": "image/png", "data": crate::render::base64(png) }));
    }
}

pub struct Server {
    pub session: Option<Session>,
    library: Option<Library>,
    pub library_notes: Vec<String>,
    gpu: Option<Gpu>,
    gpu_error: Option<String>,
}

impl Server {
    pub fn new() -> Self {
        Self { session: None, library: None, library_notes: Vec::new(), gpu: None, gpu_error: None }
    }

    /// Run one tool. Errors and panics become an `isError` result the agent can read.
    pub fn call(&mut self, name: &str, args: Value) -> Value {
        let map = match args {
            Value::Object(m) => m,
            Value::Null => Default::default(),
            other => return error_result(&format!("arguments must be an object, got {other}")),
        };
        let started = std::time::Instant::now();
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| crate::tools::dispatch(self, name, &Args(&map))));
        // Each tool call is its own undo step (slider-like edits otherwise merge).
        if let Some(s) = self.session.as_mut() {
            if s.is_stroking() {
                s.cancel_stroke();
            }
            let _ = s.execute(Command::Seal);
        }
        eprintln!("[subcraft-mcp] {name} took {:.2}s", started.elapsed().as_secs_f32());
        match outcome {
            Ok(Ok(reply)) => json!({ "content": reply.content, "isError": false }),
            Ok(Err(e)) => error_result(&e),
            Err(panic) => {
                let msg = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "unknown panic".into());
                error_result(&format!("internal error in {name}: {msg}. The project may be in a partial state; use undo or project_info to check."))
            }
        }
    }

    pub fn session(&mut self) -> Res<&mut Session> {
        self.session.as_mut().ok_or_else(|| "no project is open: use project_new (from a model) or project_open first".to_string())
    }

    /// The shelf: built-in assets plus the user's library (`SUBCRAFT_LIBRARY`, or
    /// Documents/SubCraft/Library), loaded on first use.
    pub fn library(&mut self) -> &Library {
        if self.library.is_none() {
            let mut lib = Library::builtin();
            let root = std::env::var_os(subcraft_app::assets::user_library::ENV_VAR).map(std::path::PathBuf::from).or_else(UserLibrary::default_root);
            if let Some(root) = root {
                let user = UserLibrary::new(root.clone());
                let (alphas, mut errors) = user.load_alphas();
                lib.alphas.extend(alphas);
                let (materials, e) = subcraft_app::library::load_smart_materials(&user);
                errors.extend(e);
                lib.smart_materials.extend(materials);
                let (masks, e) = subcraft_app::library::load_smart_masks(&user);
                errors.extend(e);
                lib.smart_masks.extend(masks);
                for env in user.list(subcraft_app::assets::UserAssetKind::Environment) {
                    match subcraft_app::assets::user_library::load_environment(&env) {
                        Ok(e) => lib.environments.push(e),
                        Err(e) => errors.push(e),
                    }
                }
                self.library_notes.push(format!("user library: {}", crate::paths::output(&root)));
                self.library_notes.extend(errors.into_iter().map(|e| format!("skipped: {e}")));
            }
            self.library = Some(lib);
        }
        self.library.as_ref().expect("just loaded")
    }

    /// Session, library and GPU together (renders need all three).
    pub fn render_parts(&mut self) -> Res<(&mut Session, &Library, &mut Gpu)> {
        self.library();
        if self.gpu.is_none() {
            if let Some(e) = &self.gpu_error {
                return Err(e.clone());
            }
            match Gpu::new() {
                Ok(g) => {
                    eprintln!("[subcraft-mcp] rendering on {}", g.adapter);
                    self.gpu = Some(g);
                }
                Err(e) => {
                    self.gpu_error = Some(e.clone());
                    return Err(e);
                }
            }
        }
        let session = self.session.as_mut().ok_or("no project is open: use project_new or project_open first")?;
        Ok((session, self.library.as_ref().expect("loaded above"), self.gpu.as_mut().expect("created above")))
    }

    pub fn gpu_name(&self) -> Option<&str> {
        self.gpu.as_ref().map(|g| g.adapter.as_str())
    }

    pub fn library_mut_and_session(&mut self) -> Res<(&mut Library, &mut Session)> {
        self.library();
        let session = self.session.as_mut().ok_or("no project is open: use project_new or project_open first")?;
        Ok((self.library.as_mut().expect("loaded above"), session))
    }

    pub fn library_and_session(&mut self) -> Res<(&Library, &mut Session)> {
        self.library();
        let session = self.session.as_mut().ok_or("no project is open: use project_new or project_open first")?;
        Ok((self.library.as_ref().expect("loaded above"), session))
    }
}

fn error_result(msg: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": msg }], "isError": true })
}

// ----- lookups ---------------------------------------------------------------------------------

fn same(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// A texture set by id or name; the selected one when `None`.
pub fn texture_set(session: &Session, key: Option<&str>) -> Res<TextureSetId> {
    let Some(key) = key else { return Ok(session.selection().texture_set) };
    let sets = &session.document().texture_sets;
    if let Ok(u) = uuid::Uuid::parse_str(key.trim())
        && let Some(ts) = sets.iter().find(|t| t.id.0 == u) {
            return Ok(ts.id);
        }
    let hits: Vec<&TextureSet> = sets.iter().filter(|t| same(&t.name, key)).collect();
    match hits.as_slice() {
        [one] => Ok(one.id),
        [] => {
            let partial: Vec<&TextureSet> = sets.iter().filter(|t| t.name.to_lowercase().contains(&key.trim().to_lowercase())).collect();
            if let [one] = partial.as_slice() {
                return Ok(one.id);
            }
            Err(format!("no texture set `{key}`; texture sets: {}", sets.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", ")))
        }
        _ => Err(format!("several texture sets are named `{key}`; use the id")),
    }
}

pub fn ts_ref(session: &Session, id: TextureSetId) -> Res<&TextureSet> {
    session.document().texture_set(id).ok_or_else(|| "texture set not found".to_string())
}

/// A layer by id or (unique) name, optionally only within one texture set.
pub fn layer(session: &Session, key: &str, within: Option<TextureSetId>) -> Res<(TextureSetId, LayerId)> {
    let sets: Vec<&TextureSet> = session.document().texture_sets.iter().filter(|t| within.is_none_or(|w| w == t.id)).collect();
    if let Ok(u) = uuid::Uuid::parse_str(key.trim()) {
        for ts in &sets {
            if let Some(l) = ts.find(LayerId(u)) {
                return Ok((ts.id, l.id));
            }
        }
        return Err(format!("no layer with id {key}"));
    }
    let mut hits = Vec::new();
    for ts in &sets {
        ts.walk(|l, _| {
            if same(&l.name, key) {
                hits.push((ts.id, l.id, ts.name.clone()));
            }
        });
    }
    match hits.as_slice() {
        [(ts, l, _)] => Ok((*ts, *l)),
        [] => Err(format!("no layer named `{key}` (use layers_list to see names and ids)")),
        many => Err(format!(
            "{} layers are named `{key}` (in {}); pass the id or texture_set",
            many.len(),
            many.iter().map(|h| h.2.as_str()).collect::<Vec<_>>().join(", ")
        )),
    }
}

/// Resolve `args.layer` (+ optional `args.texture_set`).
pub fn layer_arg(session: &Session, args: &Args, key: &str) -> Res<(TextureSetId, LayerId)> {
    let within = match args.str("texture_set")? {
        Some(t) => Some(texture_set(session, Some(t))?),
        None => None,
    };
    layer(session, args.req_str(key)?, within)
}

pub fn layer_ref(session: &Session, ts: TextureSetId, id: LayerId) -> Res<&Layer> {
    ts_ref(session, ts)?.find(id).ok_or_else(|| "layer not found".to_string())
}

/// Where an effect lives.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stack {
    Mask,
    Content,
}

/// An effect of a layer's mask or content stack, by id or name.
pub fn effect(layer: &Layer, key: &str) -> Res<(Stack, EffectId)> {
    let uid = uuid::Uuid::parse_str(key.trim()).ok();
    let mut hits = Vec::new();
    if let Some(m) = &layer.mask {
        for e in &m.effects {
            if uid == Some(e.id.0) || same(&e.name, key) {
                hits.push((Stack::Mask, e.id));
            }
        }
    }
    for e in &layer.content_effects {
        if uid == Some(e.id.0) || same(&e.name, key) {
            hits.push((Stack::Content, e.id));
        }
    }
    match hits.as_slice() {
        [one] => Ok(*one),
        [] => Err(format!("layer `{}` has no effect `{key}` (layer_get lists them)", layer.name)),
        _ => Err(format!("several effects of `{}` match `{key}`; pass the effect id", layer.name)),
    }
}

/// Run a command, turning its error into text.
pub fn exec(session: &mut Session, cmd: Command) -> Res<()> {
    session.execute(cmd).map_err(|e| e.to_string())
}
