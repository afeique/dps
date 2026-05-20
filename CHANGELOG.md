# Changelog — Dark Prism Solid

All notable changes to **Dark Prism Solid** (`dps`) are documented here. The
format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project adheres to [Semantic Versioning](https://semver.org/). DPS stays in `0.x`
while pre-1.0; it promotes to `1.0.0` when the solo port is feature-complete.

## [Unreleased]

### Added

- **Phase 1 exit gate closed** — the slice now has the two clauses 0.2.0
  deferred: *take damage* and *die*. The go/no-go vertical slice (fly → shoot →
  kill → take damage → die) is complete.
  - **Enemy firing** (`systems/enemy_fire`): the drifter ticks a `FireCooldown`
    and fires aimed shots at the player. Generic stand-in for the per-kind
    patterns ported in Phase 3.
  - **Player damage intake:** new collision pairs
    `enemy_bullet_hits_player` and `enemy_contact_player` (hull overlap, 20 dmg,
    drifter survives). `Fire` now carries a `faction` so one `spawn_bullets`
    path renders player (gold) and enemy (magenta) bullets.
  - **Invulnerability i-frames** (`Invulnerable` component +
    `tick_invulnerability`): a 0.6 s grace window after each hit so rapid
    contact/fire can't melt the ship in a single tick burst.
  - **Death → `GameOver` → restart:** `apply_damage` now branches player vs.
    enemy — enemy death bumps `Score::kills`; player death emits `Death`, flips
    state to `GameOver`, and despawns the ship. `OnEnter(GameOver)` clears the
    field; **R** / **Enter** restarts the slice.
- **Headless gate tests** (`src/gate_tests.rs`, the first cut of the
  `docs/port-plan.md` §9 strategy): spin a minimal `World`, drive the sim
  systems through an ad-hoc `Schedule`, and assert on the result — no rendering.
  Covers contact damage + i-frame gating, i-frame expiry, player death →
  `GameOver`, and the enemy-kill/score path.
- **Phase 2 renderer — first silhouettes (lyon).** Added `bevy_prototype_lyon`
  and a `render::shapes` module that ports the Canvas2D vector art from
  `js/modules/render/shapes.js`: the player ship hull (16-vertex silhouette +
  cockpit highlight) and the Drifter's 10-point electric star (+ white-hot
  core). Colors are HDR-emissive so the camera's `Bloom` does the glow (no
  per-shape blur); `ShapePlugin` is registered in `GamePlugin`. Replaces the
  placeholder `RegularPolygon` meshes. Particles (`bevy_hanabi`), the bullet
  layer, and the starfield are the next Phase-2 increments.

### Notes

- Collision is still naive O(n × m); the spatial-grid broadphase remains a
  deliberate Phase-3 port (premature with one enemy). This was the one Phase-1
  scope item intentionally carried forward.

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
