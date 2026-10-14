# Contributing to SubCraft

Thanks for helping! This guide covers how to get set up, the rules the codebase follows, and how
to get a change merged.

## Ways to help

- **Report bugs**: open an issue with steps to reproduce, what you expected, what happened, your OS
  and GPU, and a screenshot if it's visual.
- **Suggest features**: open an issue describing the workflow you want (Substance Painter
  comparisons are welcome).
- **Write code or docs**: issues labelled `good first issue` are a good start. The ordered roadmap
  is in [`docs/progress.md`](docs/progress.md) under "Next steps". For anything big, open an issue
  first so we can agree on the approach before you spend time on it.

## Setup

New to Rust on Windows? [`docs/development.md`](docs/development.md) walks through installing
everything, the branch / pull request loop and building the `.exe`.

```sh
git clone https://github.com/Aalzard/subcraft.git
cd subcraft
cargo build
cargo run --release              # the app
cargo run --release -- --demo    # with a demo layer stack
cargo test --workspace           # all tests
```

Rendering tests run on whatever GPU adapter exists and skip themselves (pass with a message) when
there is none. On Linux without a GPU, install Mesa's software Vulkan driver (`mesa-vulkan-drivers`)
to run them anyway.

## Architecture rules

These keep the codebase healthy; reviews check them.

- **Dependency direction**: `core ← image ← mesh/doc ← materials ← compositor/paint/bake ←
  project/export/assets ← app ← ui ← apps`. No crate reaches upward.
- **No GPU or UI in the engine**: `core`, `image`, `mesh`, `doc`, `materials`, `compositor`,
  `paint`, `bake`, `project`, `export` and `app` must not depend on egui or wgpu. Only `render`, `ui`
  and `apps/*` may.
- **UI dispatches commands**: widgets send `Command`s to `app`; no project logic lives in widgets.
- **Identity is UUIDs, never file paths.** Project files use a versioned schema, never raw struct dumps.
- **Texels go through `TiledImage`**: never assume one dense RGBA bitmap per layer.
- **Color is linear internally**: never color-transform data channels (roughness, normal, height…).
- **Every change needs tests**: image-producing code needs numeric or golden checks.

Background on these decisions is in [`docs/adr`](docs/adr).

## Making a change

1. Fork the repo and create a branch from `main`.
2. Keep the change focused: one fix or feature per pull request.
3. Add or update tests, then run:
   ```sh
   cargo test --workspace
   cargo clippy --workspace --all-targets
   ```
   Don't add new clippy warnings in the code you touch.
4. Match the surrounding style: naming, comment density and idioms of the file you're editing.
5. For UI changes, include before / after screenshots in the pull request.
6. If you finish a roadmap item or change behavior, update [`docs/progress.md`](docs/progress.md).
7. Open a pull request describing what changed and how you tested it. CI must be green before merge.

Commit messages: a short imperative summary line ("Fix seam in curvature bake"), then a blank line
and a body explaining why, when it isn't obvious.

## Licensing of contributions

SubCraft is dual-licensed under MIT and Apache-2.0. Unless you explicitly state otherwise, any
contribution you intentionally submit for inclusion is dual licensed the same way, without any
additional terms or conditions.

## Code of Conduct

Be kind and constructive. Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).
