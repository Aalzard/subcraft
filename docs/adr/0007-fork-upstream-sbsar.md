# ADR 0007: Fork vs. Upstream (SubCraft + SBSAR)

## Status

Accepted — local fork of [SubCraft](https://github.com/Aalzard/subcraft) with **optional** Substance `.sbsar` support.

## Goal

Pull upstream SubCraft improvements regularly **without** re-implementing SBSAR or fighting giant conflicts.

## Boundary

| Layer | Upstream-owned | Fork-owned (do not upstream without agreement) |
|--------|----------------|--------------------------------------------------|
| Core product | `crates/*` except hooks below, `apps/subcraft`, most of `ui` | — |
| SBSAR engine | — | `crates/sbsar/`, `native/sbsar-bake/`, `scripts/build-sbsar-host.ps1`, `bin/sbsar-bake/` (gitignored build) |
| SBSAR app API | — | `crates/app/src/sbsar.rs` |
| SBSAR UI | — | `crates/ui/src/panels/sbsar_materials.rs` (toolbar + grid + apply), dialog helpers in `layers.rs` |
| Shelf shared | upstream may change | `shelf_tile.rs`, `shelf_drag.rs` (SBSAR variant is fork) |
| Local SDK | — | `_sdk_extract/` (gitignored) |
| Fork tweaks | Prefer upstream PRs | e.g. GLB auto-orient in `crates/mesh/src/import.rs` — keep minimal, document in merge notes |

**Rule:** New SBSAR logic lives in fork-owned files. Upstream files get only **thin hooks** (menu item, shelf branch, `UserAssetKind` variant, workspace member).

## Integration touchpoints (expect merge conflicts here)

When updating from upstream, re-apply or reconcile these files first:

1. `Cargo.toml` — workspace member `subcraft-sbsar`, `tools/preview` only (not `tools/*` glob).
2. `crates/app/Cargo.toml` + `crates/app/src/lib.rs` — `sbsar` module.
3. `crates/assets/src/user_library.rs` — `UserAssetKind::Sbsar` (always **append** new enum variants at the end).
4. `crates/ui/src/state.rs` — `sbsar_materials`, `remember_sbsar`, `with_user_library`.
5. `crates/ui/src/panels/shelf.rs` — three hooks: `materials_toolbar`, `paint_sbsar_grid`, `apply_shelf_item` → `Sbsar` arm; `shelf_drag.rs` enum variant `Sbsar`.
6. `crates/ui/src/app.rs` — File/Layer menu, `sbsar_materials.poll`.
7. `crates/ui/src/panels/layers.rs` — import dialogs.
8. `.gitignore` — `_sdk_extract`, `bin/sbsar-bake`, etc.

Everything else should merge cleanly if upstream did not edit the same lines.

## Git workflow

```powershell
# Once:
git remote add upstream https://github.com/Aalzard/subcraft.git

# When you want updates:
git fetch upstream
git checkout main
git merge upstream/main
# or: git rebase upstream/main   (if you prefer linear history)
```

After merge:

1. `cargo test --workspace`
2. `cargo build -p subcraft --release`
3. Rebuild SBSAR host if SDK path unchanged: `.\scripts\build-sbsar-host.ps1`
4. Smoke: Shelf → Materials → Import SBSAR, apply one material.

Resolve conflicts by **keeping upstream behavior** everywhere except the touchpoints above; re-apply SBSAR hooks from this branch or from `docs/sbsar-integration.md`.

Helper: `.\scripts\sync-upstream.ps1` (fetch + instructions).

## Optional hardening (later)

- Cargo feature `sbsar` on `subcraft-app` / `subcraft-ui` to compile without Adobe tooling (closer to upstream binary).
- Done: Materials SBSAR UI is in `sbsar_materials.rs`; `shelf.rs` only calls `materials_toolbar` / `paint_sbsar_grid`.

## Non-goals

- Shipping Adobe SDK binaries in git.
- Replacing SubCraft’s material preset system — SBSAR only adds library entries + fill-layer import.
