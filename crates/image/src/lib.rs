//! Texel storage and image file I/O.
//!
//! * [`TiledImage`] — copy-on-write tiled surface used for every layer, mask, composite and
//!   mesh map (the guide's `TextureSurface` abstraction; dense backing is never exposed).
//! * [`Bitmap`] — dense encoded source images (imported textures, alphas, environments).
//! * [`io`] — file decode/encode.
//! * [`dilate`] — UV padding for export and display.

pub mod bitmap;
pub mod dilate;
pub mod io;
pub mod tiled;

pub use bitmap::{Bitmap, BitmapData, Wrap};
pub use io::{FileFormat, IMAGE_EXTENSIONS, ImageIoError, guess_extension, load_bitmap, save_image};
pub use tiled::{TILE_SIZE, TILE_TEXELS, Tile, TileSlot, TiledImage, slot_data_mut, slot_read};
