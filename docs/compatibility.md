# Compatibility and robustness audit

Goal: SubCraft starts, or explains clearly why it cannot, on machines we do not own.
Each entry: **can fail** → **now** → **changed** → **open**. "Tested" means an automated test or a
run on the development PC; everything else was reasoned from the code and is marked *reasoned*.

Files live in `%LOCALAPPDATA%\SubCraft` (Windows), `~/.local/share/SubCraft` (Linux),
`~/Library/Application Support/SubCraft` (macOS), or `SUBCRAFT_DATA_DIR`. Useful files:
`startup.log` (every start, adapter info, breadcrumbs), `gpu-probe.json`, `graphics.json`,
`logs/subcraft.log`, `crashes/`.

## 1. Startup

| Topic | Notes |
|---|---|
| Silent exit with no window | **Can fail:** a packaged Windows build has no console, so any `Err` returned from `main` vanished (bad command line, settings error). **Changed:** `main` now catches errors *and panics* and always shows a native dialog (`subcraft_ui::show_fatal`), also written to `startup.log`. The GPU-failure dialog already existed; it is shown once, not twice. **Open:** a failure of the OS loader itself (missing DLL) happens before our code runs, see §3. |
| Adapter selection order | Already in place before this work (0.2.1): a probe child process tries DX12, Vulkan, GL per adapter, discrete before integrated before software (`startup_probe.rs`, `gpu_probe.rs`), remembers verdicts in `gpu-probe.json`, avoids the adapter that was in use when the last start died. Windows software rasteriser (WARP) and Linux lavapipe come last through the `Cpu` kind. **Open:** WARP is only used when wgpu enumerates it; not tested on a machine without any GPU. |
| `startup.log` always written | Written before any graphics call, flushed and fsynced per line. **Changed:** if the data folder is read-only/unusable the log falls back to `%TEMP%\SubCraft-startup.log` (previously: no log at all). The adapter capability line (`adapter caps: …`) is now logged. |
| "Startup complete" marker | **Was:** written as soon as the UI state was built, i.e. *before the first frame was presented*, so a crash in the first present/surface configure looked like a successful start. **Changed:** written after 5 presented frames. |
| Safe mode (self-healing) | **New** (`app::safe_mode`). If the previous start never reached the marker, or the user passes `--safe-mode` / sets `SUBCRAFT_SAFE_MODE=1`, SubCraft starts with: GPU compositing off, GPU baking not registered, MSAA off, path tracing off, remembered window position/size ignored. A window explains it and offers *Restart normally* (starts `--normal-mode`), *Copy diagnostic info*, *Continue*. The next plain start is normal again automatically once a run completes. `--normal-mode` overrides. Tested: decision logic unit test. *Reasoned:* the window and restart button (needs a GUI session). **Open:** safe mode does not yet force a specific backend; the existing probe's avoid-list does that. |
| Missing features / limits | **New** `render::caps` probes the adapter once: max texture size, 4x MSAA on the viewport colour+depth formats, compute shaders, storage textures, RGBA16F filter/render. Before: MSAA pipelines were created unconditionally, so an adapter without 4x on those formats would hit a wgpu validation error (a panic by default). **Changed:** MSAA is dropped (single-sample pipeline) when unsupported and the reason is logged. Path tracing already had `check_support` (storage buffers/textures, workgroup size); it now also refuses in safe mode with the reason. Tested: caps reasons unit test; headless render tests run on whatever adapter exists. *Reasoned:* behaviour on a GPU that really lacks MSAA. |
| Device lost / uncaptured errors | **Was:** wgpu's default handler panics on any uncaptured error (validation, out of memory) and nothing handled device loss, so the app froze or vanished. **Changed:** the error handler logs and records the error (shown in diagnostics) instead of panicking; a device-lost callback logs, then shows a native dialog (egui cannot draw without the device) telling the user their work is in the recovery copy. **Open:** no in-process device re-creation (egui_wgpu does not support it); recovery is restart + the existing recovery prompt. *Reasoned*, not reproducible without forcing a TDR. |
| Surface lost / resize to zero / failed configure | eframe/egui_wgpu skip rendering for zero-size surfaces and reconfigure on `Lost/Outdated`. Not changed; *reasoned* from egui_wgpu 0.36 source. **Open:** a surface that cannot be configured at all ends in the existing "cannot start" dialog (the window creation fails). |
| sRGB surface format / present modes | egui_wgpu picks the first non-sRGB-suffixed format the surface supports and `AutoVsync`, which falls back to FIFO. Not changed; *reasoned*. |
| Shader / pipeline creation failure | Shaders are compile-time WGSL checked on the development GPU; a driver-specific compile failure is a validation error. Now logged instead of panicking (see above), the affected pass renders nothing. **Open:** no per-pass fallback pipeline; would need pipeline creation inside error scopes. |

## 2. Optional GPU capabilities

| Feature | Behaviour |
|---|---|
| Path tracing | Checked per device (`pathtrace::check_support`), message names the missing limit. Off in safe mode. Trace size is clamped to the buffer limit (existing). |
| Depth of field | Part of the path tracer / post pass; follows the same gate. |
| MSAA | Falls back to single sample with a log line when unsupported or in safe mode. The viewport "Anti-aliasing" toggle still shows; **open:** it does not yet grey itself out with the reason. |
| Big textures | Viewport target is clamped to `max_texture_dimension_2d` (existing `fit_target_size`). The device is created with the adapter's own max limit. CPU texel storage is tiled and independent of GPU limits. |
| GPU compositing / baking | Optional backends registered at start; the compositor falls back to the CPU path when none is registered or enabled. Both are off in safe mode. Preference: *Use GPU for compositing*. |

## 3. Windows

| Topic | Notes |
|---|---|
| Missing VC++ runtime (suspected cause of #98: nothing at all appears) | Rust MSVC builds link `vcruntime140.dll` dynamically, and the installer does not install the redistributable. On a clean Windows 10 the loader shows an error or, depending on the shell, nothing. **Changed:** the release workflow now builds with `-C target-feature=+crt-static` and fails the release if `dumpbin /dependents` still lists vcruntime/msvcp. **Not tested:** CI-only; the first release build must be checked by hand on a clean VM. |
| DPI / mixed-DPI monitors | winit declares per-monitor DPI awareness v2 and egui follows `native_pixels_per_point`, including when a window moves between monitors. `--scale` and the UI size preference override. The diagnostics report prints pixels-per-point. *Reasoned*, not tested on two monitors. |
| Installer | `installer/subcraft.iss`: x64 only (`ArchitecturesAllowed=x64compatible`), admin install into Program Files. The app never writes into its install folder: settings, logs, crash reports and recovery use `%LOCALAPPDATA%`, so a read-only install dir is fine. **Open:** no Windows-on-ARM native installer (x64 emulation works on Win11 ARM, *reasoned*). |
| Non-ASCII / spaces / OneDrive profile | Paths are `PathBuf`/`OsString` throughout; project files are addressed by UUID, not by path. Tested on this PC (a path with a space exists in the repo layout). `%LOCALAPPDATA%` redirected to OneDrive works but file locking by the sync client can fail a rename; `vfs::write` retries the rename on `PermissionDenied`. **Open:** no test with a real non-ASCII user name. |
| Long paths | Rust's std adds the `\\?\` prefix for absolute paths automatically when needed. Not tested beyond MAX_PATH. |
| Antivirus | Writes go temp-file-then-rename in the same folder (now also for exports, §5). No self-modifying exe except the existing updater (`--apply-update`), which only runs on user request. **Open:** the exe is signed in the release workflow only when secrets exist. |
| Tablets (Wacom Ink, Windows Ink, pen as click) | #82 is tracked by its own session; pen handling is in `ui` (`pen.rs` is the vector pen *tool*, not the digitiser). Not touched here. |
| Keyboard layouts / AltGr | Shortcuts are mapped by egui `Key` (layout-resolved logical key), not by scancode, so AZERTY/Dvorak users get the letter printed on the key. Shortcuts that need symbols (`[`, `]`, `+`) can be unreachable on layouts where those need AltGr (e.g. German). **Open:** audit the shortcut table for symbol-only bindings; AltGr arrives as Ctrl+Alt on Windows, so Ctrl+Alt combos must not be assigned. |

## 4. Linux and macOS

| Topic | Notes |
|---|---|
| X11 / Wayland | eframe/winit choose Wayland when `WAYLAND_DISPLAY` is set; a Wayland crash is covered by session *fix-wayland-crash*. `WAYLAND_DISPLAY`/`XDG_SESSION_TYPE`/`DISPLAY` are included in the diagnostics. winit 0.30 removed `WINIT_UNIX_BACKEND`; `SUBCRAFT_X11=1` or `--x11` forces X11 (XWayland) when `DISPLAY` is set (`ui::linux_backend`). |
| Pen tablets on Wayland (#82) | winit 0.30 has no `zwp_tablet_v2` support. Compositors that emulate a pointer for tablet tools (GNOME, KDE) deliver pen input as a mouse; Niri and some wlroots compositors deliver *nothing* (the cursor still moves because the compositor draws it). Workaround: `SUBCRAFT_X11=1 subcraft` runs under XWayland, where the pen is an XInput pointer. Logged at every Wayland start. Real support needs a winit with tablet-v2. |
| Mesa / lavapipe | The probe treats software Vulkan (lavapipe, llvmpipe) as `Cpu` adapters, tried last; render tests already run on llvmpipe in CI. Slow but should start. |
| Missing libraries | Loader errors (libxkbcommon, libwayland, libvulkan) happen before our code and print to the terminal only. **Open:** `packaging/linux/install.sh` should check for them and print apt/dnf package names. |
| macOS | Universal build (arm64 + x86_64, `lipo`) is produced by `release-platforms.yml`, experimental, **not signed or notarized**: users must right-click → Open (`packaging/macos/READ-ME-FIRST.txt`). Notarization needs an Apple Developer ID, `codesign --options runtime --timestamp`, `xcrun notarytool submit --wait`, `xcrun stapler staple`. Metal is the only backend; the probe's DX12/Vulkan/GL candidates are skipped. *Not tested here (Windows PC).* |
| aarch64 | Linux aarch64 and Windows arm64 packages are built experimentally in CI; never run by us. |

## 5. Large or broken inputs

| Input | Notes |
|---|---|
| Missing UVs | OBJ/FBX/glTF without UVs: a warning, then automatic unwrapping (existing `mesh::unwrap`). |
| NaN / infinite UVs, NaN normals, NaN positions, zero-area or overflowing triangles | **Was:** NaN UVs reached the rasteriser; a triangle whose area overflowed to infinity/NaN passed the degenerate check (`NaN <= eps` is false). **Changed:** `MeshBuilder::push_triangle` treats non-finite UVs as missing, and drops triangles whose area is not a finite number > eps. NaN normals were already regenerated. Test: `hostile_input_is_dropped_or_repaired_never_poisons_the_mesh`. |
| Overlapping UVs | Allowed; painting writes through the overlap. Auto-unwrap reports overlap statistics. Not changed. |
| Huge meshes | Stored as u32 indices, BVH is built in one pass. **Open:** no explicit triangle cap or memory estimate before import; a multi-hundred-million-triangle file would run out of memory and abort. |
| Corrupt or newer project files | `.subcraft` is a versioned zip: a newer schema gives "saved by a newer SubCraft", garbage gives `NotAProject`/`Container`, truncated rasters give `Invalid`. **Added test:** truncating the file at ~100 points and flipping 150 random bits never panics. The open path reports the error and leaves the session untouched (the existing project stays loaded). |
| Save / export interruption | Project save already wrote a temp file then renamed. **Changed:** `core::vfs::write`, used by image export, texture export, scene export and brush presets, is now atomic (temp file in the same folder, `sync_all`, rename with retry), and `write_obj` builds in memory then uses it. A full disk or a kill leaves the previous file intact and no partial file. Tests: replace, directory target, missing parent, no `.tmp` leftovers. **Open:** out-of-memory is still an abort (Rust default); texture export allocates a full bitmap per map. |
| Huge textures / unusual image formats | Decoding goes through the `image` crate; unsupported formats return an error shown in the status bar. 16-bit/float formats handled by `subcraft-image`. **Open:** no decode size cap, an extremely large PNG can exhaust memory. |

## 6. Diagnostics

* **Help → Report a bug → Copy diagnostic info** copies: version, OS, GPU, executable and data folder,
  safe-mode state, CPU threads, relevant environment variables, adapter limits and capabilities,
  device-lost/GPU errors, display scale and screen size, `graphics.json`, `gpu-probe.json`, the tail
  of `startup.log` (this and the previous start) and the log tail. Nothing is sent anywhere.

### Ready-to-paste checklist for users who cannot start SubCraft

```
SubCraft version / where you downloaded it (installer or zip):
Windows/Linux/macOS version and 32/64-bit/ARM:
Graphics card(s) and driver version (Windows: dxdiag → Display tab; Linux: `glxinfo -B` or `vulkaninfo --summary`):
Laptop with two GPUs (Intel/AMD + NVIDIA)?  Remote desktop / virtual machine?
What happens: no window / window closes / black window / error text (copy it exactly):
Does it start with `subcraft.exe --safe-mode`?
Does it start with `set SUBCRAFT_GPU=…` / `set WGPU_BACKEND=gl` (or dx12/vulkan)?
Files from %LOCALAPPDATA%\SubCraft (attach): startup.log, gpu-probe.json, graphics.json, newest file in logs\ and crashes\
Windows only: does Event Viewer → Windows Logs → Application show an error for subcraft.exe? (copy the "Faulting module")
Windows only: is "Microsoft Visual C++ 2015-2022 Redistributable (x64)" installed? (Apps list)
Antivirus / security software in use:
Tablet or special input devices:
```

## 7. Remaining risks, in order

1. Static CRT build is unverified until the first CI release run.
2. Device loss cannot be recovered in-process.
3. No symbol-key shortcut audit for non-US layouts.
4. No size caps for mesh / image import.
5. Linux missing-library errors are not caught by us.
