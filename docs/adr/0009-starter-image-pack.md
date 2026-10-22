# ADR-0009 — Starter image distribution and adjustments

Status: Accepted (2026-10-15)

The owner's CC0 pack contains 30 normal maps and 30 alpha tips, each 1024px square. Full PNGs
are losslessly optimized and accompanied by a manifest and 96px thumbnails. The catalog uses
stable UUIDs derived from each pack filename. Desktop embeds compressed PNGs so portable
updates need only replace the executable. Decoding happens only when an asset is selected.

Full decoded sources are retained for the active preview, rather than decoding every image at
startup. (The browser build that fetched images on demand was removed: ADR-0038.)

`assets::preview` owns image adjustments and CPU examples. Normals are data, never gamma
decoded; green flip, vector normalization, rotation and mirror derivatives preserve their
tangent interpretation. Alpha brightness/contrast affect coverage. Edge blending softens
boundary details and pairs final boundary texels. Preview and apply use the same math with
bounded pixel jobs. Applying produces a copy, leaving the catalog files untouched.

`app::asset_apply` owns captured scene/texture-set/layer destinations, resource UUIDs,
deduplication and history. Adjusted image resources keep lossless encoded bytes so existing
project schema 4 remains portable. Save/export, project changes and update restart wait for
pending application. A saved desktop brush preset stores its adjusted starter tip in the user
library only when needed, without eagerly loading the entire starter pack.

The UI owns asynchronous fetching, thumbnail handles, controls and progress presentation.
Wrap/scale/rotation are baked into one adjusted tile; the fill layer then repeats that tile
through its normal projection controls. Imported image previews use a square output tile.
