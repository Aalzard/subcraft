# ADR-0048: Graph materials (`.sbsar` and other material graphs)

Status: accepted (2026-11-24)

## Context

Issue #682: [open-sbsar](https://github.com/wtfmydarling/open-sbsar) is an independent,
clean-room Rust engine for compiled Substance archives (`.sbsar`), MIT/Apache-2.0 like SubCraft.
It lists a graph's inputs (with labels, groups, ranges, widgets) and renders its outputs on the
CPU or on wgpu. `docs/spp-procedurals.md` had ruled out writing our own interpreter.

At the same time MaterialCraft materials (`.mcmat`, `.mcraft`, `.sbs` source graphs) are being
added through MaterialCraft's own engine. Both are "a material graph with parameters that
renders channel images", so they share one layer model and one UI.

Checked before adopting open-sbsar (2026-11-24): three freely licensed real archives
(`CardBoard.sbsar` and `natural_lambskin_leather.sbsar` from Adobe's Apache-2.0
USD-Fileformat-plugins tests, `fabric.sbsar` from O3DE) render every output, and parameters
(colour, tearing, wrinkles, grain scale) change them as expected. No reference renders from
Substance were available here, so pixel accuracy rests on open-sbsar's published comparison.
The CPU path takes about 10 s per render at 512 px on 4 cores; its GPU path is far faster.

## Decision

* `crates/sbsar` (`subcraft-sbsar`) wraps open-sbsar with `default-features = false` (CPU only),
  because crates below `render` must not depend on wgpu. It maps outputs to channels by usage,
  hides `$outputsize`, the "Channels" output switches and the normal-format combo, sets OpenGL
  normals (or flips green when a graph has no normal-format input; Substance's default is
  DirectX), and keeps encoded values (Base Color sRGB, like an imported PNG).
* `subcraft-doc::graph_material` is engine-neutral: `GraphMaterial { engine, package, graph,
  values, size_log2 }` on `Layer::graph`, the package as a `ResourceKind::GraphPackage` resource
  (original file bytes and name), and `GraphParam` / `GraphParamKind` descriptions read from the
  package at run time (never saved). `channel_for_usage` is the one usage-to-channel table.
* The layer stays an ordinary fill layer: each filled channel is a `FillSource::Bitmap` of a
  rendered image resource, so compositing, export and every tool are unchanged (the same idea as
  ADR-0047). Saved images let a project open without re-rendering.
* `subcraft-app::graph_layer` runs renders on worker threads, one at a time per layer, keeping
  only the newest waiting request; a finished render replaces the layer's images in one undo
  step (`ReplaceLayer`). Engines plug in through the `GraphBackend` trait and `open_backend`.
* The Properties panel shows the parameters with their groups; edits are sent a moment after the
  last change with no button held, so a slider drag is one render.
* Project schema 19 (`graph` on layers, `graph_package` resources).

## Consequences

* `.sbsar` files load by File > Import resources > Import material graph… or by dropping them on
  the window or the shelf. The render size is a per-layer setting (default 512 px).
* Not yet: the GPU path (needs to live in `render`/`ui` and share the app's device), image inputs
  (mesh maps such as curvature for `.sbsar` filters), `.sbsar` layers inside `.spp` imports, and
  `.sbsar` in the smart-material library.
* A hostile archive can make a render slow or memory-hungry (open-sbsar says so); it runs on a
  worker thread and cannot block the interface, but it is not time-limited.
