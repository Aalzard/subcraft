# Prompt for Claude — unresolved Edge Wear

Work in `E:\Applications\ArtCraft\SubCraft` on `feature/brush-roadmap`. Do not switch branches,
push or release. Read CLAUDE.md, docs/progress.md, docs/HANDOFF-codex.md and ADRs 0026–0029.

The owner still rejects Edge Wear after commits a22eb6f/4f6d8ea. Treat it as unresolved despite
passing tests and screenshots. Reproduce on the default crate after baking, with Natural edge
wear both disabled and enabled. Check corner gaps, overly straight/uniform boundaries at
Breakup 0, and whether Level 0.065 with Spread 12.5 gives useful, natural outward growth.
Verify the executable the owner actually runs. Investigate bake, generator, sampling and
preview paths; do not hard-code the cube, weaken acceptance, or hide defects behind a mode.

Make Level, Spread, Softness, Variation and Breakup predictable and independently useful.
Prevent gaps across corners/UV seams and avoid flooding flat faces. Test hard, rounded, thin
and overlapping geometry; preserve legitimate concavity and old project compatibility.
You may replace the current approach where evidence shows it is wrong.

Add numeric regressions for the actual failure, test undo/save/reopen/export convergence, and
inspect the real app. Profile 4K responsiveness and worker convergence (recent CPU runs had
1.6–2.0 s convergence despite short UI polling). Do not claim success from tests alone.
Do not drive the physical mouse without the owner's permission.

Filter work is being added separately. Inspect HEAD and working changes before editing shared
files, preserve those changes, and commit only your own changes. Coordinate before concurrent
edits in this checkout. Preserve unrelated artwork/dependencies and protected zips, output/,
generate_textures.py and __pycache__/. Do not bypass the earlier cache-deletion rejection.

Run `cargo test --workspace` and `cargo clippy --workspace --all-targets` with
`CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=line-tables-only`, `-j 1` before each commit.
Make small commits, update progress/roadmaps/getting-started/ADR/handoff docs, save real-app
captures in docs/screenshots, and state exactly what was not verified.
