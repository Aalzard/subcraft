# ADR-0040 — Edit in PhotoCraft (PSD round trip of the layer stack)

Status: Accepted (2026-10-29)

## Context

Painters want to retouch a texture set in a full image editor and come back without losing the
layer structure. PhotoCraft (sibling ArtCraft app, `storytold/photocraft`) reads and writes PSD,
opens files passed on its command line, and keeps PSD layer ids (`lyid`) stable across a save.
Its `photocraft-psd` crate is standalone (thiserror + flate2, MIT/Apache) with a builder and a
reader. SubCraft had no PSD support.

## Decision

**Scope.** "Edit in PhotoCraft" sends the whole layer stack of the selected texture set. Only
pixel data travels: folders become PSD groups, paint layers become raster layers with their
painted mask as a user mask. Fill/procedural layers and effects are not sent (they are not
pixels) and are never touched by the return trip.

**Files.** One PSD per channel of the set (`<Set>_<Channel>.psd`) in a session folder under the
OS temp dir, opened together in one PhotoCraft launch. Color channels are linear → sRGB; data
channels are written raw; Height is remapped `h · 0.5 + 0.5` like integer export. Pixels are
16-bit (Gray+A for scalar channels, RGBA for 3-component ones); masks are 8-bit (the builder's
mask format). Alpha is the layer's coverage.

**Identity.** Each PSD layer carries an `lyid`; a session manifest maps `lyid` → `LayerId`
(UUIDs stay the persistent identity; files are disposable).

**Return trip.** The app polls the files' modification time and size; once a save is stable it
reads the PSD and applies it to the stack as one undo step:

* Only tiles whose 16-bit/8-bit data differ from what was exported (per-tile hashes kept in the
  session) are written back, so untouched texels stay bit-exact despite the lossy encoding.
* Name and visibility changes are applied to known layers.
* Raster layers with an unknown `lyid` (new or duplicated in PhotoCraft) become new paint layers
  at the same place in the hierarchy, painting only that channel.
* Layers deleted in PhotoCraft are not deleted in SubCraft; the status line says so.
* If the same tile also changed in SubCraft meanwhile, the PhotoCraft version wins.

**Layering.** PSD mapping (pure, doc ↔ PSD) lives in `subcraft-export::psd`; the session,
process launch, file polling and command dispatch live in `subcraft-app::photocraft`; the UI only
dispatches. A new `Command::ReplaceTextureSetLayers` swaps a set's layer list in one history entry.

**Dependency.** `photocraft-psd` is a git dependency pinned to a commit so CI can build it. A
local checkout can override it with a `[patch]` in an uncommitted `.cargo/config.toml`.

**Executable.** `SUBCRAFT_PHOTOCRAFT` env var, then `photocraft` on `PATH`, then a sibling
development build (`../photocraft/target/{release,debug}/photocraft`). Missing → a clear error.

## Consequences

Retouching works with a real layered editor and comes back non-destructively. Fill layers are
visible in SubCraft but absent from the PSD, so PhotoCraft's composite of a channel is not the
material's full composite. Deletions must be repeated in SubCraft. 16-bit integer storage limits
precision of *edited* texels only. Other PSD editors would work the same way but are not tested.
