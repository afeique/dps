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
  placeholder `RegularPolygon` meshes.
- **Phase 2 renderer — GPU explosions (`bevy_hanabi`).** Added `bevy_hanabi`
  (2D-only) and a `render::explosion` module: a one-shot 120-particle burst
  `EffectAsset` (HDR white-yellow → orange → red → transparent over a 0.35–0.7 s
  life, shrinking, with linear drag) built once at startup and spawned at each
  `Death`. The `Death` message now carries the death `position` (captured in
  `apply_damage` before despawn) so the burst lands where the entity died;
  spent effect entities are reaped by a lifetime timer.
- **Phase 2 renderer — bullet layer + parallax starfield.** Bullets now draw
  from a shared `BulletAssets` cache (one unit-circle mesh + per-team materials,
  scaled per shot — no more per-shot mesh/material allocation): player shots are
  bright `#FFFF00` with a white-hot core child, enemy shots hot magenta. Added
  `render::starfield` — three parallax depth layers of stars (far/dim/slow →
  near/bright/fast) drifting downward and wrapping, on a far background z behind
  all gameplay (dependency-free deterministic scatter).
- **Screenshot hook + ship-scale tuning.** `render::screenshot` adds an
  env-gated (`DPS_SCREENSHOT=<path>`) in-app framebuffer capture via Bevy's
  `Screenshot` API — for visual checks without OS screen-recording permission,
  and the `docs/port-plan.md` §9 web-vs-native diff hook. Bumped the player
  ship's authored radius (`SHIP_R` 15 → 22) and collider (16 → 20) so it reads
  larger than the enemies at the native window scale.
- **Phase 2 renderer — remaining items completed.**
  - **Starfield diversified:** 4 depth layers, 6 shared color tiers
    (white / blue-white / amber / HDR-cyan), size jitter, and per-star twinkle
    (scale pulse, no per-frame material writes).
  - **Procedural nebula** (`render::nebula` + embedded `nebula.wgsl`): a
    full-field background quad with a custom `Material2d` whose WGSL shader
    builds domain-warped fractal-noise gas clouds in a JWST-style teal/gold
    palette — wispy filaments, dark dust lanes, and HDR emission cores that the
    bloom lights. Animated via the global time uniform. (Shader embedded in the
    binary via `embedded_asset!`, so it works regardless of the working dir.)
- **Phase 3 — enemy roster + waves (increment 1).** Added a per-enemy module
  pattern (`systems/enemy/<kind>.rs`, each exposing `shape()` / `stats()` /
  `ai()`) plus a spawn dispatch (`systems::enemy::spawn`). Ported three new
  enemies from `js/modules/enemy/*` as lyon silhouettes + simplified-faithful
  movement: **Hunter** (red, orbital `hunter_arc`), **Guardian** (green,
  `square` patrol), **Wasp** (yellow, fast `wasp_zigzag`); added
  `EnemyKind::Hunter`. A basic wave spawner (`systems::wave`) drops enemies on a
  timer at the top edge, cycling the ported roster — replacing the single
  hardcoded drifter. Enemies with a `FireCooldown` shoot via the generic aimed
  `enemy_fire`.
- **Phase 3 — full enemy roster (increment 2).** Ported the remaining six kinds
  (in parallel, same module pattern): **Stalker** (cyan sword, `arc` swoop),
  **Prowler** (magenta, slow `keep_distance` missile turret), **Weaver** (yellow
  `weaver_spinup`), **Sentinel** (green `weaver_spinup`), **Tangerine/Bomber**
  (orange `spiked_circle`, `chase`), **Titan** (big pink `boulder` tank). The
  wave spawner now cycles the full 10-kind roster, and `despawn_offscreen_enemies`
  caps stray counts. FixedUpdate's per-kind AI is nested as a chained sub-group
  (keeps the system tuple under Bevy's 20-element limit). Subsequent increments:
  per-kind fire patterns (spread / machinegun / laser / missile / mine), the
  data-driven wave tables (`wave-data.js`), weapons + defense skills, drops,
  asteroids.
- **Phase 3 — combat systems (increment 3).** A large batch built by parallel
  subagents on disjoint files:
  - **Per-kind enemy fire patterns** (`systems/enemy/firing.rs`) — aimed /
    spread / machinegun / charged / wide-arc / rotating-spiral / sweeping /
    slow-mine, dispatched by `EnemyKind`, replacing the generic aimed shot.
  - **Splitting asteroids** (`systems/asteroids.rs`) — rocky lyon polygons spawn
    on a timer and split into smaller tiers when shot; offscreen-culled.
  - **Drops** (`systems/drops.rs`) — gold/point orbs drop on enemy death, drift
    magnetically toward the player, and add to `Score` on pickup.
  - **Data-driven wave tables** (`systems/wave.rs`) — a 12-wave escalating table
    ported from `wave-data.js` (enemy-type introduction order, boss waves,
    endless loop with scaling), replacing the basic timed cycle.
  - **Player weapon variety** (`systems/weapons.rs`) — switchable primary
    weapons (Single / Twin / TripleSpread / Rapid / WideArc), cycled with
    **Tab** or **1**–**5**.
  - **Shields + spare ships** — the player now has a `Shield` (absorbs damage
    before `Health`) and `Lives` (respawn in place on death until exhausted,
    then `GameOver`).
  FixedUpdate is organized into chained sub-groups (AI / spawners / fire /
  collisions / drops / cleanup). Lasers, missiles, and mines are bullet
  approximations for now; homing / piercing power weapons remain for a later
  pass. The screenshot tool gained env-gated `keep_player_alive` + `force_fire`
  so captures show a populated combat scene.
- **Phase 3 — power weapons, skills, powerups + scenario tests (increment 4).**
  - **Homing power weapon** (`systems/power_weapon.rs`) — key **E** fires a
    salvo of homing missiles (a `Homing` component + steering system; they're
    player bullets, so the existing collision handles damage).
  - **Active defense skills** (`systems/skills.rs`) — **LShift** dash (forward
    burst + brief i-frames), **C** shield-burst (refill shield + i-frames),
    **X** bomb (clear the field: `Death` + score + despawn), each cooldown-gated.
  - **Powerups** (`systems/powerups.rs`) — enemies rarely drop gem pickups
    (ShieldRestore / ExtraLife / Bomb), applied on contact.
  - **Headless Phase-3 scenario tests** (`src/wave_tests.rs`) — wave spawning
    grows the enemy count, a player bullet kills an enemy (+score, emits
    `Death`), asteroids split when shot, and a ready enemy emits a `Fire`. **8
    tests pass** total. This is a representative cut of the §9 gate, not full
    web-build E2E parity. Remaining polish: weapon-modifying powerups, true
    laser/mine/homing projectile fidelity, per-kind firing tuning.
- **Phase 4 — input (gamepad).** The first connected controller now drives the
  ship: left stick steers (thrust/strafe, mirroring WASD) and South /
  right-trigger fires, OR-combined with the keyboard into the same `Intent`
  (Bevy 0.18's entity-based `Gamepad`).
- **Phase 4 — native SFX.** A procedural sound-effects synth (`src/audio.rs`),
  the Rust equivalent of `sound-defs.js` (port-plan §4's documented fallback to
  baked samples): generates PCM in Rust (oscillators + envelopes + deterministic
  noise), encodes 16-bit mono WAV byte buffers, and plays them on events —
  player/enemy shots (on `Fire`), explosions (on `Death`), and player hits (on
  `Damage` to the ship). Enables Bevy's `wav` feature so rodio decodes the
  runtime-generated buffers.
- **Phase 4 — mouse-aim + full gamepad bindings.** Mouse-aim: the cursor is
  projected into world space (`Intent.aim` / `aim_active`); while active the
  ship turns to face the cursor (turn-rate-limited) instead of strafe-rotating,
  falling back to strafe when the cursor leaves the window. Gamepad now covers
  every action alongside keyboard/mouse — **West** fires the power weapon,
  **LT** dashes, **LB** shield-bursts, **North** bombs (plus left-stick steer +
  South/RT fire from before). Remaining Phase-4: a mouse crosshair sprite and
  music streaming (CDN + disk cache).
- **Perf: nebula baked to a texture — fixes a major framerate dip.** The
  procedural nebula was a full-screen fragment shader evaluating ~6 fbm (≈144
  `sin`) per pixel every frame; on the dev GPU (Radeon Pro 5500M, Retina) that
  ran at only **~15 fps / 66 ms**. It's now **baked once to a 512² texture at
  startup** (`render::nebula`, CPU fbm) and shown as a single screen-covering
  sprite with an HDR tint for bloom — same look, but the per-frame cost is one
  texture sample, restoring **~60 fps / 16.7 ms**. (Bake matched to 4 octaves so
  detail stays ≥ 1 texel and doesn't alias into grain.) Removed the live nebula
  shader + `Material2d`. Added opt-in `DPS_FPS=1` (frame-rate logging) and
  `DPS_NO_NEBULA=1` (isolate cost) dev toggles.
- **Nebula look fixed (jaggies / stretch).** The baked nebula was a *square*
  512² texture stretched to a wide screen quad → features stretched ~1.7× and
  edges stair-stepped. Now baked at **1024²** with the noise domain
  **aspect-corrected** so features are round on screen, **5 octaves** of detail,
  linear filtering, and softer alpha edges. Smooth, undistorted, vivid.

### Changed

- **Starfield reworked.** Removed the scrolling-shooter vertical drift; stars
  now **parallax off the player's position** (far layers barely move, near
  layers shift visibly → depth in the fixed-camera arena). Expanded to ~10 color
  tiers (white / blue / cyan / amber / red / purple + HDR accents), more size
  variance, and per-star twinkle. (`drift_stars` → `parallax_stars`.)
- **Controls → twin-stick (WASD + mouse).** WASD / gamepad left stick now set a
  screen-space MOVE direction independent of facing; the **mouse aims** — the
  ship instantly faces the cursor and fires toward it (`Intent.move_dir`;
  `ship_control` faces `aim`). Replaces the old thrust + rotate scheme.
- **Nebula dimmed to a backdrop.** It had become so bright/dense (tint 1.6,
  near screen-filling) that it drowned out the ship + enemies and overloaded
  bloom into rectangular block artifacts. Dropped the sprite tint to 0.5 and
  raised the density/region thresholds for more dark gaps, so gameplay reads
  clearly on top and bloom stays clean. (`DPS_SCREENSHOT` now captures *normal*
  play; `DPS_DEMO=1` re-enables the immortal/auto-fire demo capture.)
- **Nebula rebuilt as smooth cloud blobs.** Replaced the full-screen
  turbulent-fbm sprite (which read as jagged / torn) with a handful of soft
  gaussian cloud sprites in teal/gold/rose tints (port spec VII.5). Organic
  shape comes from **analytic sine lobes**, not value noise — noise modulation
  through `exp()` amplified its lattice creases into triangular facets. Bloom
  now uses a **prefilter threshold of 1.0** so only HDR-emissive gameplay glows
  and the dim clouds never feed bloom (that broad dim source caused both the
  rectangular blocks and the triangular bloom facets); `DebandDither::Enabled`
  on the camera.
- **Local music player** (`src/music.rs`, `MusicPlugin`): shuffles and
  auto-advances the ~73 `music/*.mp3` tracks (Fisher-Yates, loops), volume 0.5.
  Added bevy's `mp3` cargo feature; replaces the planned CDN streaming.
- **Recorded SFX** from `sfx/*.wav` (~530 jsfxr files) play by event name with
  a random variant + specific→generic fallback (`enemyDestroy_HUNTER` →
  `enemyDestroy`), a 30 ms per-event throttle, master volume 0.8. The in-house
  procedural **synth is retained** as the fallback when an event has no file.
  - **GPU bullet trails** (`bevy_hanabi`, continuous emission + global
    simulation space) stream behind player shots and fade out.
  - **Bloom** nudged up (`intensity` 0.2) for a harder glow.
  - **lyon vs vello evaluated** (`docs/lyon-vs-vello.md`): **stay on lyon** —
    `bevy_vello` rasterizes to an 8-bit SDR target that clamps emissive > 1.0,
    which would break the HDR → bloom glow the whole look depends on.

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
