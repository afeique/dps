# Dark Prism Solid (`dps`)

A native **Rust + [Bevy](https://bevyengine.org)** port of the single-player
[Rainboids](https://rainboids.cat.computer) game — rebuilt for best-in-class
graphics: HDR bloom, emissive glow, and GPU-compute particles (10k–100k) well
beyond what the Canvas2D/WebGL2 web build can do.

> **DPS** = Dark Prism Solid. (Also, fittingly, *damage per second*.)

## Status

**Phase 0 — graphics + toolchain spike.** A window with an HDR `Camera2d` +
bloom rendering bright emissive silhouettes. The full gameplay port (ECS, vector
silhouettes, particles, audio, UI) is staged in the design docs below.

## Why this exists

The existing Electron desktop wrapper ships the web build unchanged — same
renderer, same ceiling. This port exists to **beat the web build graphically**,
which requires a modern GPU pipeline (wgpu via Bevy) the browser/Electron path
can't reach. See the design docs for the full rationale.

## Design docs

- `docs/comparison.md` — why Rust + Bevy over Kotlin/C#/C++ and the alternative
  engines (with the performance and graphics-ceiling analysis).
- `docs/port-plan.md` — the staged implementation plan (OOP-JS → ECS, renderer,
  audio, UI, phases, risks).

## Build & run

Requires a recent Rust toolchain (built with 1.95).

```sh
cargo run            # debug (fast iteration)
cargo run --release  # optimized
```

The first build downloads and compiles Bevy and is slow; subsequent builds are
incremental.

## Relationship to Rainboids

`dps` is a **separate, independent** project. It ports the **solo** game only;
the JavaScript source in the Rainboids repo (`js/modules/*`) is the behavioral
source of truth. Multiplayer and the Rust `sim/` crate are out of scope.

## License

Copyright (c) Afeique Sheikh, All Rights Reserved.
