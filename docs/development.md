# Developing SubCraft locally (Windows)

A step-by-step guide: set up once, then the daily loop of changing code, testing it, and getting it
into `main`, plus building the Windows `.exe` and publishing a download.

Commands are for **PowerShell**. They work the same on macOS / Linux with `/` instead of `\`.

## 1. One-time setup

1. **Git**: install from <https://git-scm.com/download/win> (defaults are fine).
2. **Rust**: download and run `rustup-init.exe` from <https://rustup.rs>.
   When it asks, let it install the **Visual Studio C++ Build Tools** (Rust needs Microsoft's
   linker and Windows SDK on Windows). Then close and reopen PowerShell.
3. Check both work:
   ```powershell
   git --version
   cargo --version
   ```
4. **Editor (optional)**: [VS Code](https://code.visualstudio.com) with the **rust-analyzer**
   extension gives autocomplete and inline errors.
5. **Get the code** (skip if you already have it):
   ```powershell
   cd E:\Applications\ArtCraft
   git clone https://github.com/Aalzard/subcraft.git SubCraft
   cd SubCraft
   ```

## 2. Run and test

```powershell
cargo run --release                # start the app (first build takes a few minutes)
cargo run --release -- --demo      # start with the demo layer stack
cargo test --workspace             # run every test
cargo clippy --workspace --all-targets   # lint
```

After a build you can also start it directly: `.\target\release\subcraft.exe`.

## 3. Daily workflow

`main` is protected: nobody (the owner included) can push to it directly or force push. Every
change goes through a pull request, and it can only be merged once the **Test (Linux)** and
**Build + engine tests (Windows)** checks pass.

```powershell
git checkout main
git pull                           # get the latest code
git checkout -b my-change          # a branch for this change (any name)

# ... edit code, run the app, run the tests ...

git add -A
git commit -m "Short description of the change"
git push -u origin my-change
```

Then open the link Git prints (or the repo page on GitHub) and click **Create pull request**.
Wait for the checks to go green, then click **Merge pull request**. Afterwards:

```powershell
git checkout main
git pull
git branch -d my-change            # delete the finished branch locally
```

Before starting new work, read [`docs/progress.md`](progress.md): it lists what is done and the
next steps in order. Code rules are in [`CONTRIBUTING.md`](../CONTRIBUTING.md).

## 4. Build the Windows .exe

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows.ps1
```

(`-ExecutionPolicy Bypass` is needed because Windows blocks running downloaded scripts by default.)

This builds an optimized, stripped program that opens without a console window, and creates:

```
dist\SubCraft-0.1.0-windows-x64\SubCraft.exe    <- double-click to run
dist\SubCraft-0.1.0-windows-x64.zip              <- share this
```

The zip is about 11 MB and contains the exe, the README and the licenses. Nothing else needs to
be installed to run it; any PC with a DirectX 12 or Vulkan GPU works. Windows SmartScreen may
warn about an unknown publisher the first time (the exe is not code-signed): click
**More info → Run anyway**.

## 5. Publish a download on GitHub

GitHub builds the zip for you on a clean Windows machine and attaches it to a release:

1. Bump the version in the root `Cargo.toml` (`version = "0.1.0"` → `"0.2.0"`) through a normal
   pull request, so the zip name matches.
2. On GitHub: **Releases → Draft a new release**.
3. **Choose a tag** → type `v0.2.0` → **Create new tag**. Give it a title and click
   **Generate release notes**.
4. Click **Publish release**.

About 15 minutes later `SubCraft-0.2.0-windows-x64.zip` appears under the release's **Assets**
(watch progress in the **Actions** tab, workflow "Release"). That page is the link to share.

## 6. Working with Claude Code locally (optional)

[Claude Code](https://code.claude.com/docs) can work in this folder on your PC the same way it
does in the cloud. Start it inside the SubCraft folder: it reads [`CLAUDE.md`](../CLAUDE.md)
(project rules) and [`docs/progress.md`](progress.md) (what to do next) automatically. Ask it to
work on a branch and open a pull request, so the protected `main` and the checks still apply.

## Troubleshooting

| Problem | Fix |
| --- | --- |
| `link.exe not found` or `linker not found` | The C++ Build Tools are missing: run the Visual Studio Installer and add **Desktop development with C++**. |
| `running scripts is disabled on this system` | Use the `powershell -ExecutionPolicy Bypass -File ...` form shown above. |
| Builds are very slow | Exclude the `target` folder from Windows Defender real-time scanning. |
| The app window is black or won't open | Update your GPU driver; SubCraft needs DirectX 12 or Vulkan. |
| `git push` to `main` is rejected | Expected: `main` is protected. Push a branch and open a pull request. |
