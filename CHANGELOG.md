# Changelog — Dark Prism Solid

All notable changes to **Dark Prism Solid** (`dps`) are documented here. The
format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project adheres to [Semantic Versioning](https://semver.org/). DPS stays in `0.x`
while pre-1.0; it promotes to `1.0.0` when the solo port is feature-complete.

## [0.2.0] - 2026-05-20

### Added

- **Phase 1 ECS architecture scaffold** implementing the OOP-JS → ECS mapping
  from `docs/port-plan.md`. Module layout: `states`, `resources`, `messages`,
  `components/`, `systems/`, `render/`, `app.rs`.
  - `GameState` states; `PlayBounds` / `Score` resources; `Collision` /
    `Damage` / `Death` / `Fire` events (Bevy 0.18 buffered-message API:
    `#[derive(Message)]` + `MessageWriter`/`MessageReader`).
  - Components: `Ship` / `Intent` / `Weapon`, `Enemy` / `EnemyKind` /
    `AiState`, `Bullet`, plus shared `Velocity` / `Collider` / `Health` /
    `Faction` / `Lifetime`.
  - Systems wired into a `FixedUpdate` simulation pipeline (`.chain()`) plus an
    `Update` input pass that writes only `Intent`.
- **Playable vertical slice:** fly the ship (WASD / arrows), fire (Space), and
  destroy a drifter through the bullet → `Damage` → `Death` message flow. The
  HDR camera + bloom moved into `render` so the Phase-0 glow aesthetic carries
  over to the ship/enemy/bullets.

### Notes

- Remaining Phase-1 work: the player can't yet take damage or die (enemy
  contact + enemy bullets are Phase 3). Collision is naive O(bullets × enemies);
  the spatial-grid broadphase is a Phase-3 port.

## [0.1.0] - 2026-05-20

### Added

- Initial project scaffold: Cargo binary crate targeting **Bevy 0.18**, with
  Bevy's recommended dev-build-speed profiles and a thin release profile.
- **Phase 0 graphics + toolchain spike** (`src/main.rs`): an HDR `Camera2d` with
  bloom rendering two over-bright emissive silhouettes that drift, proving the
  Rust + Bevy stack and the glow/bloom direction.
- Design docs under `docs/`: the language/engine comparison and the staged
  Rust + Bevy port plan.
