use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use subcraft_core::ResourceId;

use crate::TextError;

/// Bundled fonts: (display name, file bytes). DejaVu covers Latin, Greek, Cyrillic, Arabic and
/// Hebrew (Bitstream Vera licence, `assets/fonts/DejaVu-LICENSE.txt`).
pub const BUNDLED_FONTS: &[(&str, &[u8])] = &[
    ("DejaVu Sans", include_bytes!("../assets/fonts/DejaVuSans.ttf")),
    ("DejaVu Sans Bold", include_bytes!("../assets/fonts/DejaVuSans-Bold.ttf")),
    ("DejaVu Serif", include_bytes!("../assets/fonts/DejaVuSerif.ttf")),
    ("DejaVu Sans Mono", include_bytes!("../assets/fonts/DejaVuSansMono.ttf")),
];

/// Persistent font identity. Never a file path: picked files are embedded in the project as
/// a resource and referenced by UUID.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FontRef {
    Bundled { name: String },
    /// An installed font by family and style. Falls back to the default font when missing.
    System { family: String, bold: bool, italic: bool },
    Embedded { resource: ResourceId },
}

impl Default for FontRef {
    fn default() -> Self {
        FontRef::Bundled { name: BUNDLED_FONTS[0].0.into() }
    }
}

/// A parsed-on-demand font file (shared bytes + face index within a collection).
#[derive(Clone)]
pub struct Font {
    pub(crate) data: Arc<Vec<u8>>,
    pub(crate) index: u32,
}

impl std::fmt::Debug for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Font({} bytes, face {})", self.data.len(), self.index)
    }
}

impl Font {
    /// Validates that `data` holds a TrueType / OpenType face that can be shaped.
    pub fn from_bytes(data: Arc<Vec<u8>>, index: u32) -> Result<Self, TextError> {
        let font = Font { data, index };
        font.with_face(|_| ()).ok_or(TextError::BadFont)?;
        Ok(font)
    }

    pub(crate) fn with_face<R>(&self, f: impl FnOnce(&rustybuzz::Face<'_>) -> R) -> Option<R> {
        let face = rustybuzz::Face::from_slice(&self.data, self.index)?;
        Some(f(&face))
    }

    pub fn bytes(&self) -> &Arc<Vec<u8>> {
        &self.data
    }

    /// Family name from the font's name table, when present.
    pub fn family_name(&self) -> Option<String> {
        self.with_face(|f| {
            f.names().into_iter().filter(|n| n.name_id == 1).find_map(|n| n.to_string())
        })
        .flatten()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemFontInfo {
    pub family: String,
    pub bold: bool,
    pub italic: bool,
}

/// Font lookup: bundled, installed (scanned lazily once), and loaded files, cached by identity.
#[derive(Default)]
pub struct FontLibrary {
    db: Mutex<Option<fontdb::Database>>,
    cache: Mutex<HashMap<FontRef, Font>>,
}

impl FontLibrary {
    pub fn new() -> Self {
        Self::default()
    }

    fn with_db<R>(&self, f: impl FnOnce(&fontdb::Database) -> R) -> R {
        let mut guard = self.db.lock();
        let db = guard.get_or_insert_with(|| {
            let mut db = fontdb::Database::new();
            db.load_system_fonts();
            db
        });
        f(db)
    }

    /// Installed fonts, one entry per family + style, sorted by family.
    pub fn system_fonts(&self) -> Vec<SystemFontInfo> {
        let mut out: Vec<SystemFontInfo> = self.with_db(|db| {
            db.faces()
                .filter_map(|f| {
                    Some(SystemFontInfo {
                        family: f.families.first()?.0.clone(),
                        bold: f.weight.0 >= 600,
                        italic: f.style != fontdb::Style::Normal,
                    })
                })
                .collect()
        });
        out.sort_by(|a, b| a.family.to_lowercase().cmp(&b.family.to_lowercase()).then(a.bold.cmp(&b.bold)).then(a.italic.cmp(&b.italic)));
        out.dedup();
        out
    }

    pub fn bundled() -> Result<Vec<(&'static str, Font)>, TextError> {
        BUNDLED_FONTS.iter().map(|(n, d)| Ok((*n, Font::from_bytes(Arc::new(d.to_vec()), 0)?))).collect()
    }

    /// Load a `.ttf` / `.otf` / `.ttc` file picked by the user. The caller embeds `bytes` in the
    /// project; this only validates and returns the face.
    pub fn load_bytes(bytes: Vec<u8>) -> Result<Font, TextError> {
        Font::from_bytes(Arc::new(bytes), 0)
    }

    /// Resolve a reference. `embedded` supplies the bytes of project-embedded fonts.
    /// A missing system font resolves to the default bundled font (the layer stays usable on
    /// another computer); a missing embedded font is an error.
    pub fn resolve(&self, font: &FontRef, embedded: &dyn Fn(ResourceId) -> Option<Arc<Vec<u8>>>) -> Result<Font, TextError> {
        if let Some(f) = self.cache.lock().get(font) {
            return Ok(f.clone());
        }
        let loaded = match font {
            FontRef::Bundled { name } => {
                let (_, data) = BUNDLED_FONTS.iter().find(|(n, _)| n == name).unwrap_or(&BUNDLED_FONTS[0]);
                Font::from_bytes(Arc::new(data.to_vec()), 0)?
            }
            FontRef::System { family, bold, italic } => {
                let found = self.with_db(|db| {
                    let query = fontdb::Query {
                        families: &[fontdb::Family::Name(family)],
                        weight: if *bold { fontdb::Weight::BOLD } else { fontdb::Weight::NORMAL },
                        stretch: fontdb::Stretch::Normal,
                        style: if *italic { fontdb::Style::Italic } else { fontdb::Style::Normal },
                    };
                    let id = db.query(&query)?;
                    db.with_face_data(id, |data, index| (data.to_vec(), index))
                });
                match found.and_then(|(d, i)| Font::from_bytes(Arc::new(d), i).ok()) {
                    Some(f) => f,
                    None => Font::from_bytes(Arc::new(BUNDLED_FONTS[0].1.to_vec()), 0)?,
                }
            }
            FontRef::Embedded { resource } => {
                let bytes = embedded(*resource).ok_or_else(|| TextError::FontNotFound(resource.to_string()))?;
                Font::from_bytes(bytes, 0)?
            }
        };
        self.cache.lock().insert(font.clone(), loaded.clone());
        Ok(loaded)
    }
}
