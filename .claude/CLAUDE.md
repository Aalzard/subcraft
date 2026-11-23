# SubCraft

Independent Painter-class 3D texturing application. Rust workspace (see `docs/adr/0001-technology-stack.md`).

Build: `cargo build` · Run: `cargo run --release` · Test: `cargo test --workspace`

Read `docs/progress.md` first — it lists what is done and the ordered next steps.

## Architecture rules

- Dependency direction: core ← image ← mesh/doc ← materials ← compositor/paint/bake ← project/export/assets ← app ← ui ← apps. No crate reaches upward.
- `core`, `image`, `mesh`, `doc`, `materials`, `compositor`, `paint`, `bake`, `project`, `export`, `app` must not depend on egui or wgpu. Only `render`, `ui`, `apps/*` may.
- UI dispatches commands to `app`; no project logic in widgets.
- Persistent identity is UUIDs, never file paths. Project files use a versioned schema, never raw struct dumps.
- Texel storage goes through `TiledImage`; never assume one dense RGBA bitmap per layer.
- Color channels are linear internally; never color-transform data channels.
- Every subsystem change needs tests; image-producing code needs numeric/golden checks.
- Never claim a feature works without running it; record results in `docs/progress.md`.
