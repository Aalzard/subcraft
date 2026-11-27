# Vendored crates

Third-party crates SubCraft carries with small patches. They are wired in with
`[patch.crates-io]` in the root `Cargo.toml` and excluded from the workspace.

## egui_dock 0.21.1 (MIT, see `egui_dock/LICENSE`)

Upstream: https://github.com/anhosh/egui_dock, release 0.21.1 from crates.io.
The first commit that adds this folder is the unmodified crate (dev-dependencies and
Cargo.lock removed). SubCraft's changes:

- `TabBarStyle::wrap` (issue #637): when the tabs of a panel don't fit its width, they
  wrap onto more rows ("stacked tabs") instead of scrolling. The tab bar grows by one
  row height per extra row. Code: `widgets/dock_area/show/leaf.rs` (`tab_bar`,
  `wrap_rows`).

To update egui_dock, copy the new release over this folder and re-apply the changes
above (search for `SubCraft:` in the sources).
