//! Stable object identity.
//!
//! Every persistent object (layer, texture set, resource, asset, effect) is identified by a UUID.
//! File paths are *never* identity: a resource keeps its id when its source file moves.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
            pub const fn nil() -> Self {
                Self(Uuid::nil())
            }
            /// Deterministic id derived from a name; used for built-in assets so that projects
            /// referencing them stay valid across application versions.
            pub fn from_name(name: &str) -> Self {
                Self(Uuid::new_v5(&Uuid::NAMESPACE_OID, name.as_bytes()))
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let s = self.0.simple().to_string();
                write!(f, "{}({})", stringify!($name), &s[..8])
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

define_id!(
    /// A node in a layer stack (paint layer, fill layer or folder).
    LayerId
);
define_id!(
    /// A texture set (one imported material slot).
    TextureSetId
);
define_id!(
    /// A project-embedded resource (imported bitmap, baked map, ...).
    ResourceId
);
define_id!(
    /// A library asset (brush alpha, material preset, smart material, environment, ...).
    AssetId
);
define_id!(
    /// An effect inside a mask or content effect stack.
    EffectId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_ids_are_stable() {
        assert_eq!(AssetId::from_name("brush/round"), AssetId::from_name("brush/round"));
        assert_ne!(AssetId::from_name("brush/round"), AssetId::from_name("brush/square"));
    }

    #[test]
    fn ids_roundtrip_json_as_plain_uuid() {
        let id = LayerId::new();
        let json = serde_json_like(&id);
        assert!(json.starts_with('"') && json.len() == 38, "{json}");
    }

    fn serde_json_like(id: &LayerId) -> String {
        // Avoid a serde_json dev-dependency: Display is the serialized form.
        format!("\"{}\"", id)
    }
}
