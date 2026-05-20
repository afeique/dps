# Changelog — Dark Prism Solid

All notable changes to **Dark Prism Solid** (`dps`) are documented here. The
format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project adheres to [Semantic Versioning](https://semver.org/). DPS stays in `0.x`
while pre-1.0; it promotes to `1.0.0` when the solo port is feature-complete.

## [0.1.0] - 2026-05-20

### Added

- Initial project scaffold: Cargo binary crate targeting **Bevy 0.18**, with
  Bevy's recommended dev-build-speed profiles and a thin release profile.
- **Phase 0 graphics + toolchain spike** (`src/main.rs`): an HDR `Camera2d` with
  bloom rendering two over-bright emissive silhouettes that drift, proving the
  Rust + Bevy stack and the glow/bloom direction.
- Design docs under `docs/`: the language/engine comparison and the staged
  Rust + Bevy port plan.
