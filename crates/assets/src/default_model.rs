//! The model SubCraft starts with: `assets/models/SubCraft_base.fbx`, compiled into the program
//! (like the starter-pack thumbnails) so a fresh install needs no extra files.

/// Name of the bundled model (also its mesh name).
pub const NAME: &str = "SubCraft_base";

/// The FBX file's bytes.
pub const FBX: &[u8] = include_bytes!("../../../assets/models/SubCraft_base.fbx");
