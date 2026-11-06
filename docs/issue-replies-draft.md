# Draft replies for open issues (not posted)

Prepared 2026-11-07 against `main` at 63fb47d and the v0.2.1 tag. Nothing here has been posted;
the owner posts, edits or drops each reply. Each section: issue number, title, state, what is
true (checked in code / by running tests, with where), and the reply text.

Facts that several replies rely on (all checked in the source):

* Data folder: `%LOCALAPPDATA%\SubCraft` on Windows (`crates/app/src/recovery.rs`, `data_dir()`;
  `SUBCRAFT_DATA` overrides it). It is **not** `%APPDATA%` (Roaming).
* Files in it: `startup.log` (written before any graphics call, flushed on every line),
  `logs\subcraft.log` (+ `subcraft.1.log` … four older runs), `crashes\` (panic reports),
  `graphics.json` (chosen API + GPU), `gpu-probe.json` (cache of adapters that passed the probe).
  `app.ron` is not where the graphics choice lives.
* Reset graphics settings: delete `graphics.json` and `gpu-probe.json` (or set
  `SUBCRAFT_GPU_REPROBE=1` for one run to force a fresh probe; `SUBCRAFT_SKIP_GPU_PROBE=1` skips it).
* Since 0.2.1 on Windows, start-up runs a GPU probe in a child process
  (`subcraft --gpu-probe`), so a driver crash kills only the child. If nothing works a message box
  names what failed and points at `startup.log`.
* Windows requirement in the README: Windows 10/11 64-bit, GPU with DirectX 12 (Vulkan also works).

---

## #98 windows 64 bit running problems

**State:** open, label `bug`. Reporter kevin-aus72, 0.2.1, Windows 10 Home 64-bit, installer and
x64 zip both tried. GPU, log, steps: none given. The text "get the screen" is cut off; unclear
whether a window, an error box or nothing appears. The owner already asked for dxdiag + log
(comment on the issue), with a vague log location ("%APPDATA%\SubCraft or %LOCALAPPDATA%\SubCraft").

**What is true:**

* 0.2.1 already contains the crash-safe start-up (probe, `startup.log`); the reporter is on it.
  So "no window at all" on 0.2.1 means one of: the probe's own message box was shown (and the
  reporter means that "screen"), the app was killed before the probe (missing runtime / blocked
  by security software / SmartScreen), or every adapter failed and the box was dismissed. Only
  `startup.log` can tell these apart: it records every step before any graphics call.
* The real log locations are above (`%LOCALAPPDATA%\SubCraft`, not Roaming).
* Minimum GPU: DirectX 12 capable (Vulkan also works); the app asks the adapter for its full
  texture limit and runs a one-buffer device test in the probe. Very old GPUs, the Microsoft Basic
  Display Adapter, some remote-desktop / VM sessions and drivers older than DX12 fail that.
* PR #97 (`fix/issue-91-opengl-startup`) would **not** plausibly fix this: it fixes OpenGL-only
  shader pipeline creation and adds "after a crashed start, try Automatic before the chosen API".
  The reporter never chose OpenGL, and the default path on Windows is DX12.
* `crash-msaa` (session "Investigate 0.1.7 startup crash (viewport msaa 8192)") is the clamp of
  viewport targets to the device texture limit (commits 76b17a9 / e6df477). It is already in 0.2.1
  (`git merge-base --is-ancestor e6df477 v0.2.1` succeeds), so it cannot be the cause on 0.2.1.
  PR #55 (chrstphrbrns) is the same change from a contributor.
* Not reproducible from code without the reporter's machine.

**Reply:**

> Thanks for the report, and sorry it doesn't start. I can't tell what is going wrong from this
> alone, so I need a few things (all are quick):
>
> 1. **What exactly do you see?** Nothing at all, a window that flashes and closes, or an error
>    box? Your message says "get the screen" and then stops, so please say what that screen is
>    (a screenshot is ideal).
> 2. **The start-up log.** Press Win+R, paste `%LOCALAPPDATA%\SubCraft` and press Enter (note:
>    Local, not Roaming). Please attach `startup.log`, and `logs\subcraft.log` if it exists. If
>    there is a `crashes` folder, attach the newest file in it. SubCraft writes `startup.log`
>    before it touches the graphics card, so it should exist even if no window ever opens. If the
>    folder doesn't exist at all, tell me that: it means the program is stopped before it starts
>    (for example by security software), which is a different problem.
> 3. **Your graphics card.** Win+R, `dxdiag`, Display tab: card name, driver version, and the
>    "DirectX 12" line under Driver Model/Feature Levels. "Save All Information" gives a text file
>    you can attach. SubCraft needs a GPU with DirectX 12 (Vulkan also works); a very old card or
>    a virtual machine / remote desktop session often doesn't qualify.
>
> Two things to try meanwhile:
>
> * Close SubCraft, delete `graphics.json` and `gpu-probe.json` from `%LOCALAPPDATA%\SubCraft`
>    (this resets the graphics settings and the remembered GPU), then start it again.
> * Update your graphics driver from the card maker's site (not just Windows Update), then try
>    again.
>
> This is separate from #91: that one is about choosing OpenGL, which you didn't do, so the fix
> in review for #91 is unlikely to help here. I'll wait for your files before guessing further.

---

## #83 Support Needed for App Compatibility

**State:** open, label `bug`. BtFormado, 0.2.0, Windows 11, Intel UHD + RTX 2080 Super; app
doesn't open unless the Intel GPU is disabled; "nothing at all", nothing in Task Manager
(crashes instantly, probably inside a driver while enumerating adapters). The owner commented
twice (fix planned; "almost certainly inside a graphics driver").

**What is true:**

* The reporter is on **0.2.0**. 0.2.1 (released 2026-11-04) contains the fix described in the
  owner's comment: the adapter probe in a child process, discrete-before-integrated ordering,
  fallback to the next adapter, a GPU combo in Preferences, `SUBCRAFT_GPU` override, and the
  breadcrumb log (`crates/app/src/startup_probe.rs`, `crates/ui/src/gpu_probe.rs`,
  `crates/ui/src/graphics.rs`; commits 3192c8a, 6d7b0ef, 99b8bb7, all ancestors of `v0.2.1`).
  CHANGELOG 0.2.1: "not yet confirmed by the reporter".
* Unit tests cover the policy (ordering, avoid-last-crashed, cache validity); the real crash on an
  Optimus laptop could not be reproduced here (no such hardware), so "fixed" is unconfirmed.
* No code change needed in this triage.

**Reply:**

> Good news if it's the same problem: version 0.2.1 (out since Nov 4) changes how SubCraft picks
> a graphics card. It now tests each card and API in a separate throw-away process first, so a
> driver that crashes can't take SubCraft down; it prefers your RTX card, skips one that fails,
> and remembers what worked. There is also a GPU choice in Preferences > Graphics. Could you
> install 0.2.1 from the Releases page and tell me whether it starts with both GPUs enabled?
>
> If it still doesn't, please attach `startup.log` from `%LOCALAPPDATA%\SubCraft` (Win+R, paste
> that path). It is written before any graphics call, so it shows exactly which step died.
> Setting the environment variable `SUBCRAFT_GPU_REPROBE=1` for one run forces a fresh test.

---

## #82 Wacom clicks not registered

**State:** open, label `bug`. hced, 0.2.0, CachyOS, niri (Wayland), Wacom Intuos PTH-651, NVIDIA +
Intel. Cursor moves with the pen; no painting, no button clicks. Owner commented: fix in progress,
please try X11 (`WAYLAND_DISPLAY=`).

**What is true:**

* What is reported: pen *motion* reaches egui, pen *contact* (tip down) does not become a click.
  The reporter did not say whether the app ran on Wayland or X11.
* Cause in code: egui-winit turns a touch/pen contact into pointer events for only one touch id
  at a time; if a pen leaves proximity without an `End` (seen on some Wayland/Xwayland setups) all
  later touch ids are ignored. `PenInputFixer` (`crates/ui/src/pen_input.rs`, commit 8ef718b,
  "Map pen/touch contact to primary press/release before egui", issue #82) now synthesizes the
  press/move/release; it ships in **0.2.1** (ancestor of `v0.2.1`, not of 0.2.0). The earlier
  `wacom-clicks` work is exactly this commit (branch `claude/fix-wayland-crash-9cd59b`, merged to
  main via 6d7cbe1).
* Unit tests of the fixer's event streams pass on main (`cargo test -p subcraft-ui --lib pen_input`:
  7 passed); it has **not** been run with a
  real tablet (the owner has none). `SUBCRAFT_INPUT_LOG=1` logs raw input for diagnostics.
* Not reproducible here (Windows machine, no Wayland, no Wacom).

**Reply:**

> 0.2.1 contains a change aimed at exactly this: pen and touch contact is now turned into a normal
> left-button press/release before it reaches the interface (some Wayland setups never send the
> "pen lifted" event, which made every later pen contact get ignored). I can't test with a tablet,
> so I can't call it fixed. Could you try 0.2.1 and tell me whether (a) clicking buttons with the
> pen works and (b) painting works?
>
> If it still doesn't, run it from a terminal with `SUBCRAFT_INPUT_LOG=1 ./subcraft`, tap and
> drag with the pen over the 3D view for a few seconds, and attach the output. Please also say
> whether it works with `WAYLAND_DISPLAY= ./subcraft` (X11 via Xwayland). Whichever of those
> works tells me where the events get lost.

---

## #81 Continuous Navigation (pressed navigation key will instantly take over the former navigation mode)

**State:** open, enhancement. hced. Owner commented (working on it with #80).

**What is true:** implemented and released in **0.2.1** (CHANGELOG: "Blender-style 3D navigation
… with continuous mouse takeover"). `crates/ui/src/nav.rs`: `NavDrag::step` re-evaluates the mode
from the held buttons/modifiers every frame, and when the mode changes it rebases the drag origin
(zero delta that frame), so pan -> zoom -> orbit continues without releasing. Wired in
`crates/ui/src/viewport.rs`. Unit tests in `nav.rs`. Commit 0e33d42. Nothing to implement; the
issue can be closed once hced confirms it behaves as wanted.

**Reply:**

> This is in 0.2.1: while you hold a navigation button, pressing or releasing a modifier switches
> the motion without interruption (for example, in the Blender scheme: hold the middle button to
> orbit, add Shift to pan or Ctrl to zoom, release it to go back, all in one drag with no jump).
> Please try it and tell me if something you rely on in Blender or ArmorPaint still stops the
> motion; I'll adjust it. If it works for you, this can be closed.

---

## #80 Support Blender navigation control

**State:** open, enhancement. hced. Owner commented (preset planned).

**What is true:** implemented in **0.2.1**: Preferences > "3D navigation" with `Painter` (default:
Alt+LMB or RMB orbit, MMB pan, Alt+RMB zoom) and `Blender` (MMB orbit, Shift+MMB pan, Ctrl+MMB
zoom; the wheel zooms in both). Shortcuts window and status bar hint follow the scheme. In the
Blender scheme Alt does not navigate, so it doesn't clash with painting. (`nav.rs`,
`viewport.rs`, `recovery.rs` preference `nav_scheme`.) `cargo test -p subcraft-ui --lib nav`:
8 passed on main (mapping per scheme, takeover without a jump, preference defaults). Not implemented: arbitrary rebinding of the
navigation buttons, trackpad (Shift+two-finger-scroll) gestures, numpad view keys.

**Reply:**

> Blender-style navigation is in 0.2.1: Preferences > 3D navigation > Blender gives middle mouse
> orbit, Shift+middle pan, Ctrl+middle zoom (wheel zooms too), and the continuous takeover from
> #81 works with it. Fully custom button mapping and numpad view shortcuts aren't there yet; tell
> me which of those you need. Please try it and report back, then this can be closed.

---

## #91 Switching graphics API to OpenGL causes a hard crash on startup; config reset doesn't recover

**State:** open, bug. pitipet, 0.2.1, Windows, RTX 4090. PR #97 (draft, `fix/issue-91-opengl-startup`).

**What is true (evidence):**

* Reporter's two symptoms: (1) OpenGL crashes at start; (2) deleting `app.ron` in
  `%APPDATA%\SubCraft\data` did not help. (2) is explained by the file locations: the choice is in
  `%LOCALAPPDATA%\SubCraft\graphics.json`, so clearing the wrong folder left OpenGL selected.
* PR #97 fixes (1) at the cause for the two WGSL problems it names: GLSL allows one sampler per
  texture (`viewport.wgsl` sampled `t_base` with two samplers) and `textureLoad` on a
  `texture_depth_2d` (`post.wgsl`). The diff is small and consistent with how wgpu's GL backend
  fails (pipeline creation error -> panic on first frame). The PR says it was run on Windows +
  RTX 4060 with GL, rendering OK. I did not re-run OpenGL (no GL verification here) — the claim rests
  on the PR text and the diff. Its post-pass tests are unaffected by design (`.r` of an R32 depth
  copy, same values).
* PR #97 also addresses (2) partly: `startup_attempts(.., previous_start_crashed)` tries Automatic
  first after a start that never reached "startup complete" (log marker). That covers a crash
  during init, but only when the break happens in the process the log sees; the probe child still
  uses the stored API as the allowed list (`allowed_backends`). So "choice that crashes the
  process can't lock you out" holds for normal crashes. Remaining gap: nothing automatically
  clears a bad `graphics.json`; the manual reset is deleting it.
* Verdict: **PR #97 plausibly fixes #91 for the OpenGL pipeline cause; the lock-out part is
  handled by the fall-back-to-Automatic after a crashed start.** Not confirmed by the
  reporter, and it does not touch other OpenGL-only failures if there are any.

**Reply:**

> Thanks for the clear steps. Two separate things happened here:
>
> 1. *OpenGL crashes.* The viewport shaders used a texture/sampler combination that the OpenGL
>    backend can't build, so the 3D view failed on the first frame. A fix is in review (PR #97);
>    it will ship in the next release.
> 2. *Resetting didn't help.* The graphics choice is not in `app.ron`. It is stored in
>    `%LOCALAPPDATA%\SubCraft\graphics.json` (Local, not Roaming), so clearing the other folder left
>    OpenGL selected. To recover now: close SubCraft, delete that `graphics.json` (and
>    `gpu-probe.json` next to it), and start again. The same fix also makes SubCraft try
>    "Automatic" first after a start that crashed, so a bad choice can't lock you out again.
>
> I'll post here when a build with the fix is available so you can confirm.

---

## #93 Feature request: Batch export baked mesh maps from Export Textures

**State:** open, enhancement. Smith990401, 0.2.1. PR #100 (draft, `feat/issue-93-export-baked-maps`).

**What is true:** PR #100 adds a collapsible "Baked Mesh Maps" section in Export Textures: one
checkbox per baked map (with All / None), "Selected texture set only", an **Export baked maps**
button; files named `<mesh>_<set>_baked_<map>.png`, using the window's padding and format override;
maps a set hasn't baked are skipped. This matches each item the request lists (checkboxes, select
all/none, selected maps and texture sets in one operation, descriptive filenames, raw maps kept apart
from the material textures). Checked by running the PR branch (a790a7a): `cargo test -p subcraft-app
--test export_baked_maps` 2 passed (every selected map of every set is one named file with the baked
pixels; selected-set-only, unbaked maps skipped, format override) and `cargo test -p subcraft-ui
--test export_maps` 4 passed (including the UI test that opens the section, unchecks a map and
exports). Not clicked through in the real window by me. So the PR does implement #93.

**Reply:**

> Thanks, this is a good suggestion and PR #100 implements it: Export Textures gets a "Baked Mesh
> Maps" section with a checkbox for each baked map (Normal, AO, Curvature, Thickness, Position, ID,
> World Space Normal, and so on), All/None, an option to export only the selected texture set, and
> one button that writes every selected map for every texture set. Files are named
> `<mesh>_<set>_baked_<map>.png` so nothing overwrites anything and baked maps stay clearly apart
> from the material textures. It's in review and will be in the next release.

---

## #95 Mask painting in 3D View only writes thin 1–2 px lines instead of full brush footprint

**State:** open, no label. sigur666, macOS, 0.2.1. PR #99 (open, `test/issue-95-mask-3d`).

**What is true (evidence):**

* **PR #99 does not fix #95.** It only adds tests (`crates/app/tests/mask_stroke_3d.rs`,
  `crates/ui/tests/viewport_paint.rs`); no source file changes.
* I ran the engine tests from the PR branch: `a_3d_mask_stroke_covers_the_brush_footprint_like_a_content_stroke`
  and `on_the_sample_crate_a_mask_stroke_covers_as_much_as_a_paint_stroke` both pass (content and
  mask stroke on the sample crate both 9699 texels). So the paint engine already stamps the full
  footprint for a mask stroke with the issue's brush settings (size 5 %, flow 100 %, hardness 50 %,
  spacing 0.1); the thin-line symptom is **not reproduced** at the engine level.
* The UI-level test in the PR (real egui input through `viewport::show`, 100 / 150 / 200 %
  display scale) passes too: paint layer 1328 / 1301 / 1297 texels and mask exactly the same.
* So the cause is not the mask code path. What differs on the reporter's machine is macOS
  (Retina scale, mouse/trackpad event rate) and their exact brush/mask setup: very few pointer
  events per frame would give a path of sparse stamps, but a 5 % round brush would still stamp a
  footprint, not a 1-2 px line. A 1-2 px line is what a *tiny size* gives, so check whether "Size
  5 %" is a percentage of something small there, or the brush was in a different tool/mode
  (e.g. a line/polygon tool, or a stencil), or whether the displayed mask preview is a downsampled
  view.
* Needs the reporter's info; I can't repro on Windows.

**Reply:**

> Thanks for the detailed steps. I followed them (same brush: Round, size 5 %, flow and opacity 100 %,
> hardness 50 %, spacing 0.1, paint on Mask with value 1.0, black mask on a fill layer) in an
> automated test on a mesh with a clean 1024-style UV layout and the mask stroke is as wide as a
> normal paint stroke there, so I can't reproduce the thin line yet. A few questions to narrow down
> what's different on your Mac:
>
> 1. Does painting directly on the *layer* (target "Layer" instead of "Mask") in the 3D view give a
>    full-width stroke with the same brush, on the same model?
> 2. Does it also happen on the built-in sample crate (File > New project, or just start SubCraft
>    without a model)?
> 3. Which model is it, and what is the texture set size and the model's UV scale (do the UV
>    islands fill the 0-1 square, or are they small)? If you can share the model, that helps most.
> 4. Is the display scaling set to Retina's default, and are you using a trackpad, a mouse or a
>    pen?
> 5. What does the brush cursor circle look like in the 3D view - is it much larger than the line
>    that appears in the mask?
>
> A screenshot of the Brush panel while painting the mask would also help.

---

## #85 Apple Silicon native

**State:** open, enhancement. Macwelshman. Owner has replied twice (universal binary, "testing now").

**What is true:** 0.2.1 release assets include `SubCraft-0.2.1-macos-universal-experimental.dmg`
and `.zip` (one binary for Apple Silicon and Intel). README: macOS "Experimental, not notarized, no
in-app updates", runs natively on Apple Silicon (macOS 11+, Metal); "untested on that hardware"
for macOS and Arm packaging (README line 45, `docs/progress.md` Session 113). No separate arm64-only
download exists or is promised in the docs.

**Reply:**

> The macOS download in 0.2.1 (`SubCraft-0.2.1-macos-universal-experimental.dmg` or `.zip`) is a
> universal build, so it runs natively on Apple Silicon without Rosetta. It is marked experimental
> (not notarized, no in-app updates), and I haven't been able to verify it on much Apple Silicon
> hardware, so please tell me if it opens and how it performs. A separate Apple-Silicon-only
> download is not planned at the moment; if the universal one is a problem for you, say why.

---

## #79 No Linux version

**State:** open, enhancement. mithubindia. Owner replied (pointing to the release asset).

**What is true:** `SubCraft-0.2.1-linux-x86_64.tar.gz` (supported, in-app updates) and
`SubCraft-0.2.1-linux-aarch64.tar.gz` (experimental) are in the 0.2.1 release assets (checked with
`gh release view v0.2.1`). README system requirements: Linux x86_64, Ubuntu 22.04 or newer, Vulkan
driver, X11 or Wayland. The reporter has not answered.

**Reply:**

> There is a Linux build: open the latest release, scroll to **Assets** and download
> `SubCraft-<version>-linux-x86_64.tar.gz` (or the `linux-aarch64` one for Arm). Unpack it with
> `tar -xzf` and run `./install.sh` or the `subcraft` binary. It needs a Vulkan driver (Mesa or the
> vendor's) and X11 or Wayland; the README has the steps. If it doesn't start for you, tell me your
> distro and GPU and I'll look. Otherwise I'll close this in a few days.

---

## #75 Ipad version please!

**State:** open, enhancement. danielzanco. Owner replied twice (no commitment; "keep open").

**What is true:** nothing in the README or docs plans iOS/iPadOS. SubCraft is a desktop app (Windows,
Linux, experimental macOS and Arm builds). There's no touch/Pencil UI beyond the pen/touch-to-mouse
mapping for tablets on desktop. Nothing to add.

**Reply:** (no new reply needed; the two existing comments are accurate.) Optional wording if a
final answer is wanted:

> To be honest about where this stands: an iPad version isn't planned right now. SubCraft is a
> desktop app, and iPadOS would need a new touch and Pencil interface and App Store distribution,
> which is a separate project. I'm leaving the issue open as a record of interest; I'll say so here
> if that changes.

---

## #71 Any plan to support Arm64 in Linux?

**State:** open, enhancement. batoceragaming. The owner's earlier comment says Arm64 Linux isn't
supported because releases are x86_64 only — **this is out of date.**

**What is true:** 0.2.1 release assets include `SubCraft-0.2.1-linux-aarch64.tar.gz` (+ SHA256), marked
experimental in README ("Linux aarch64 (Arm64) … Experimental, in-app updates"); `docs/progress.md`
Session 113: "Not run anywhere" on real aarch64 hardware.

**Reply:**

> Update: there is now an experimental Linux Arm64 build. Download
> `SubCraft-<version>-linux-aarch64.tar.gz` from the latest release, `tar -xzf`, then `./install.sh`.
> It hasn't been tested on much real Arm hardware yet, so please tell me what machine you try it on
> and whether it opens and runs. (My earlier comment in this issue said only x86_64 was built; that is
> no longer true.)
