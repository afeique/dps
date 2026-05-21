# Dark Prism Solid (`dps`)

A native **Rust + [Bevy](https://bevyengine.org)** port of the single-player
[Rainboids](https://rainboids.cat.computer) game — rebuilt for best-in-class
graphics: HDR bloom, emissive glow, and GPU-compute particles (10k–100k) well
beyond what the Canvas2D/WebGL2 web build can do.

> **DPS** = Dark Prism Solid. (Also, fittingly, *damage per second*.)

## Status

Actively in development. The playable slice works today: fly, shoot, cycle
weapons, fight the full enemy roster across waves, take damage, die, restart.
Phase 1 (core loop) and Phase 2 (renderer) are complete; Phase 3 (combat
systems) is in progress. See [`CHANGELOG.md`](CHANGELOG.md) for what landed and
[`docs/port-plan.md`](docs/port-plan.md) for the staged roadmap.

---

## Quick start

If you already have a recent Rust toolchain and a working GPU driver:

```sh
git clone https://github.com/afeique/dps.git
cd dps
cargo run --release
```

`cargo run` drops you straight into gameplay (there's no title screen yet). The
**first** build downloads and compiles Bevy and its dependencies — expect
several minutes and a few GB of `target/`. Subsequent builds are incremental and
fast.

New to Rust, or hitting build errors? Read the per-platform setup below first.

---

## Prerequisites (all platforms)

| Requirement | Notes |
|-------------|-------|
| **Rust toolchain** | **1.85 or newer** is required (the crate uses Rust **edition 2024**). Built and tested with **1.95**. Install via [rustup](https://rustup.rs). |
| **A C/C++ linker & build tools** | Some dependencies build native code. Provided by Xcode CLT (macOS), `build-essential`/`base-devel` (Linux), or the MSVC build tools (Windows). See your platform section. |
| **A GPU with a modern graphics backend** | The particle system (`bevy_hanabi`) uses **compute shaders**, so you need **Vulkan** (Linux/Windows), **DX12** (Windows), or **Metal** (macOS). Virtually any GPU from the last ~10 years qualifies; ensure drivers are installed. Software/`llvmpipe` rendering will *not* run the particles. |
| **Disk & network** | The first build pulls hundreds of crates. Budget ~5 GB of free disk for `target/` and registry caches. |

> **Why no `assets/` folder?** Shaders are embedded in the binary and all art is
> generated procedurally (vector silhouettes via `lyon`), so there is nothing to
> copy alongside the executable. Just build and run.

### Install Rust (every platform)

Use [rustup](https://rustup.rs) — it installs `rustc`, `cargo`, and keeps the
toolchain updatable.

- **macOS / Linux:**
  ```sh
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```
  Then restart your shell (or `source "$HOME/.cargo/env"`).

- **Windows:** download and run [`rustup-init.exe`](https://rustup.rs) (see the
  Windows section for the prerequisite C++ build tools).

Verify:

```sh
rustc --version   # should print 1.85.0 or newer
cargo --version
```

---

## Platform setup

### macOS

1. **Install the Xcode Command Line Tools** (provides the linker and headers):
   ```sh
   xcode-select --install
   ```
   A full Xcode install also works but is not required.

2. **Install Rust** (see above).

3. **Build & run:**
   ```sh
   cargo run --release
   ```

Notes:
- Works on both **Apple Silicon** (arm64) and **Intel** (x86_64) Macs; Bevy uses
  the **Metal** backend automatically.
- No extra system libraries are needed — audio/windowing/graphics all come from
  system frameworks.
- macOS blocks programmatic OS screenshots without Screen Recording permission;
  the app's own `DPS_SCREENSHOT` capture (below) reads the GPU framebuffer
  directly and sidesteps that.

### Linux

Bevy needs a handful of development packages (ALSA for audio, `udev` for
gamepad/device enumeration, and X11/Wayland + windowing libraries), plus a
working **Vulkan** driver for your GPU.

**Debian / Ubuntu / Pop!\_OS / Mint:**
```sh
sudo apt update
sudo apt install -y \
  build-essential pkg-config \
  libasound2-dev libudev-dev \
  libx11-dev libxkbcommon-dev libwayland-dev \
  mesa-vulkan-drivers vulkan-tools
```

**Fedora / RHEL:**
```sh
sudo dnf install -y \
  gcc gcc-c++ pkgconf-pkg-config \
  alsa-lib-devel systemd-devel \
  libX11-devel libxkbcommon-devel wayland-devel \
  mesa-vulkan-drivers vulkan-tools
```

**Arch / Manjaro:**
```sh
sudo pacman -S --needed \
  base-devel pkgconf \
  alsa-lib \
  libx11 libxkbcommon wayland \
  vulkan-icd-loader vulkan-tools
# plus the driver for your GPU, e.g.:
#   Intel:   vulkan-intel
#   AMD:     vulkan-radeon
#   NVIDIA:  nvidia-utils (proprietary) or vulkan-nouveau
```

Then **install Rust** (see above) and:
```sh
cargo run --release
```

Notes:
- Verify Vulkan works first with `vulkaninfo | head` or `vkcube`. If those fail,
  fix your GPU driver before building — Bevy + `bevy_hanabi` won't run on
  software rendering.
- **Wayland vs X11:** both are supported. If you hit a window-creation issue on
  Wayland, try forcing X11/XWayland: `WINIT_UNIX_BACKEND=x11 cargo run`.
- **WSL2** works if your distro has GPU passthrough (recent Windows 11 + updated
  drivers expose `/dev/dxg`). Plain WSL2 without GPU acceleration will fail to
  create a rendering surface — run natively on Windows in that case.

### Windows

1. **Install the Microsoft C++ build tools.** Rust's default toolchain on
   Windows is `*-msvc`, which needs a linker. Install **Visual Studio 2022** (or
   the standalone **Build Tools for Visual Studio**) and select the
   **“Desktop development with C++”** workload. Download:
   <https://visualstudio.microsoft.com/downloads/> → *Tools for Visual Studio* →
   *Build Tools for Visual Studio*.

2. **Install Rust** via [`rustup-init.exe`](https://rustup.rs). Accept the
   default `stable-x86_64-pc-windows-msvc` toolchain.

3. **Build & run** from PowerShell or the *x64 Native Tools Command Prompt*:
   ```powershell
   git clone https://github.com/afeique/dps.git
   cd dps
   cargo run --release
   ```

Notes:
- Bevy uses **DX12** by default on Windows (Vulkan also works). Update your GPU
  drivers if window/device creation fails.
- Prefer the MSVC toolchain. The GNU toolchain (`*-gnu`) can work but is not the
  tested path.

---

## Building & running

From the project root:

```sh
cargo run              # debug build — fast to compile, slower at runtime
cargo run --release    # optimized build — recommended for actually playing
cargo build --release  # build without launching; binary at target/release/dps
```

**Build profiles** are pre-tuned in [`Cargo.toml`](Cargo.toml): your own code
compiles with light optimization while *all dependencies* (Bevy & friends) are
fully optimized, so debug runs feel smooth without slowing incremental rebuilds.
The release profile adds thin LTO, a single codegen unit, `panic = abort`, and
strips symbols for a lean binary.

### Controls

| Action | Keys |
|--------|------|
| Move / thrust | **W A S D** or **Arrow keys** |
| Fire | **Space** |
| Cycle weapon | **Tab** or **Q** |
| Select weapon directly | **1** – **5** (Single, Twin, Triple-Spread, Rapid, Wide-Arc) |
| Restart after death | **R** or **Enter** |

### Running the tests

The Phase-1 exit-gate scenario tests are **headless** (no window/GPU) — the
simulation is decoupled from rendering, so they run anywhere Rust does:

```sh
cargo test
```

### Capturing a screenshot (dev/CI)

Set `DPS_SCREENSHOT` to a file path. The app renders a warmed-up mid-game frame,
writes a PNG there, and exits — no OS screen-recording permission needed:

```sh
# macOS / Linux
DPS_SCREENSHOT=/tmp/dps.png cargo run --release

# Windows (PowerShell)
$env:DPS_SCREENSHOT="dps.png"; cargo run --release
```

---

## Troubleshooting

- **First build is very slow / looks stuck.** It isn't — compiling Bevy from
  scratch takes minutes. Watch progress with `cargo build -v`. Later builds are
  incremental.
- **`error: linker not found` / `cc not found` / `link.exe not found`.** The
  C/C++ build tools aren't installed. Re-do the “build tools” step for your
  platform (Xcode CLT / `build-essential` / MSVC “Desktop development with C++”).
- **Linux: `failed to find a Vulkan ICD` or a blank/black window.** Your Vulkan
  driver is missing or broken. Install the `mesa-vulkan-drivers` (or vendor)
  package above and confirm with `vulkaninfo`.
- **`error: package requires a newer Rust`** or edition-2024 errors. Update the
  toolchain: `rustup update stable`.
- **Window opens but particles/explosions don't render.** You're likely on a
  software/`llvmpipe` GPU with no compute-shader support — `bevy_hanabi`
  requires a real GPU backend. Enable hardware acceleration / fix drivers.

---

## Why this exists

The existing Electron desktop wrapper ships the web build unchanged — same
renderer, same ceiling. This port exists to **beat the web build graphically**,
which requires a modern GPU pipeline (wgpu via Bevy) the browser/Electron path
can't reach. See the design docs for the full rationale.

## Design docs

- [`docs/comparison.md`](docs/comparison.md) — why Rust + Bevy over
  Kotlin/C#/C++ and the alternative engines (performance and graphics-ceiling
  analysis).
- [`docs/port-plan.md`](docs/port-plan.md) — the staged implementation plan
  (OOP-JS → ECS, renderer, audio, UI, phases, risks).
- [`docs/lyon-vs-vello.md`](docs/lyon-vs-vello.md) — the vector-renderer decision
  (why DPS stays on `lyon`).

## Project layout

```
src/
  main.rs            app entry — DefaultPlugins + GamePlugin
  app.rs             GamePlugin: wires states, resources, messages, schedules
  states.rs          GameState (Playing / Paused / GameOver)
  components/        ECS components (player, enemy, projectile, common)
  systems/           simulation systems (FixedUpdate) + input (Update)
    enemy/           per-kind enemy modules (shape / stats / AI / firing)
  render/            HDR camera + bloom, starfield, nebula, bullets, explosions
  gate_tests.rs      headless exit-gate scenario tests
```

## Relationship to Rainboids

`dps` is a **separate, independent** project. It ports the **solo** game only;
the JavaScript source in the Rainboids repo (`js/modules/*`) is the behavioral
source of truth. Multiplayer and the Rust `sim/` crate are out of scope.

## License

Copyright (c) Afeique Sheikh, All Rights Reserved.
