# ADR-0010 — Desktop graphics API selection

Status: Accepted (2026-10-15)

Graphics device creation happens before eframe loads its normal widget preferences. Store a
small version-1 `graphics.json` in the app data folder, read it before creating the window, and
atomically persist each selection. `app::graphics` owns the serialized API choice and file IO;
`ui::graphics` maps it to wgpu backends and renders settings. No GPU types enter app.

Explicit DirectX 12 or OpenGL selects only that backend. Automatic retains Windows' existing
DX12-first behavior to avoid loading problematic Vulkan drivers unnecessarily. `WGPU_BACKEND`
continues to override preferences for diagnostics, and Preferences says when it is present.
On a returned startup error, retry Automatic once and open Preferences with the explanation.
Corrupt/future settings also use Automatic with a visible warning. Preferences always reports
the actual adapter backend separately from the saved next-launch choice.

Switching a live wgpu device would require rebuilding all surfaces, texture handles and renderer
resources. Apply on the next launch and clearly ask users to save/restart. Selecting a choice
does not close the app or discard work. Browser builds report their browser-selected API.

Keep eframe's adapter-specific device limits, including its downlevel OpenGL defaults; see the
[wgpu limits documentation](https://docs.rs/wgpu/30.0.1/wgpu/struct.Limits.html). Test both real
Windows backends, including offscreen viewport display, previews and screenshot readback.
