# Draft reply for issue #83

Thanks for the detailed report — the combination (Intel UHD + RTX 2080 Super, works only with the
iGPU disabled) was enough to find the cause.

**What was wrong:** at startup SubCraft only chose a graphics *API* (DirectX 12 / Vulkan / OpenGL) and
left the choice of *graphics card* to the default. On a laptop with two GPUs that could land on the
Intel one, or on an adapter that cannot draw into the window, and there was no second attempt — so the
window never appeared. Disabling the Intel GPU "fixed" it by leaving only one candidate.

**What changed on branch `cloud/gpu-adapter`:**

* SubCraft now enumerates all adapters and tries them in order — discrete card first, then integrated,
  then software renderers — and *skips* any adapter that cannot present to the window or cannot create
  a device, instead of giving up on the first failure.
* The full adapter list (name, type, backend, driver version) and the adapter actually chosen are
  written to the log (`logs/subcraft.log`), so a startup problem can be diagnosed from the log alone.
* Preferences > Graphics has a new **GPU** setting: *Automatic*, *High performance*, or a specific card
  by name. It is saved and applied on the next start.
* For a quick test without touching Preferences there is an environment variable:
  `SUBCRAFT_GPU=NVIDIA` (any part of the adapter name, case-insensitive) forces that card for one run.

**What I need from you:** I don't have a machine with this hardware, so the fallback logic is covered by
unit tests but the real Optimus combination is untested. Could you try the next build with the Intel GPU
**enabled** again and, whether it works or not, attach `logs/subcraft.log`? The log now lists every
adapter it saw and which one it picked, which tells us immediately whether the ordering did the right
thing on your laptop.
