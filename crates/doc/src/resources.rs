//! Project-embedded resources (imported bitmaps, environment maps).
//!
//! Resources are immutable and identified by UUID; replacing a file creates a new resource.
//! Because history snapshots may reference any resource, nothing is removed during a session;
//! unreferenced resources are simply not written on save.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use subcraft_core::ResourceId;
use subcraft_image::Bitmap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Texture,
    Alpha,
    Environment,
}

#[derive(Debug)]
pub struct Resource {
    pub id: ResourceId,
    pub name: String,
    pub kind: ResourceKind,
    pub bitmap: Arc<Bitmap>,
    /// Original file location (informational only — never identity).
    pub source_path: Option<PathBuf>,
    /// Original encoded file bytes, kept so saving embeds the file losslessly.
    pub encoded: Option<Arc<Vec<u8>>>,
}

#[derive(Clone, Debug, Default)]
pub struct ResourceStore {
    map: HashMap<ResourceId, Arc<Resource>>,
}

impl ResourceStore {
    pub fn insert(&mut self, r: Resource) -> ResourceId {
        let id = r.id;
        self.map.insert(id, Arc::new(r));
        id
    }

    /// Insert an already shared resource unless one with its id exists.
    pub fn insert_shared(&mut self, r: Arc<Resource>) {
        self.map.entry(r.id).or_insert(r);
    }

    pub fn get(&self, id: ResourceId) -> Option<&Arc<Resource>> {
        self.map.get(&id)
    }

    pub fn bitmap(&self, id: ResourceId) -> Option<&Arc<Bitmap>> {
        self.map.get(&id).map(|r| &r.bitmap)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Arc<Resource>> {
        self.map.values()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}
