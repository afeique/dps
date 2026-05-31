# Enemy Revamp — Maneuver/AI Plan (dps)

## Context

dps already has all **20 enemy types** from rainboids (`enemy-data.js` standard +
elemental roster). The gap the player feels is **movement quality**: most dps
enemies move by lerping a target position, so they slide around without character.
Rainboids' `movement.js` + `ENEMY_AI_OVERHAUL.md` design is **acceleration (thrust)
based, with rotate-to-face/heading and per-kind state-machine maneuvers** — enemies
that bank into turns, dive, strafe, kite, flock, and blink. This plan ports that
movement *quality* per kind. (New named bosses — Sentinel Prime / Hive Tyrant /
Void Reaver — are a separate, larger effort, tracked at the end.)

Target: in the tower-defense context there is **no player ship**; the seek target
is the central **Core**. Every "toward the player" behavior retargets to the Core.

## Shared infrastructure (`systems::steering`, extend the existing module)

Reynolds-style pure helpers feeding `Velocity` (thrust), already started:
`seek`, `arrive`, `separation`, `approach` (force-limited). Add:
- `flee(pos, threat, max)` — desired velocity away from a point (kiting/evade).
- `pursue`/`evade` (lead a moving target) — optional; the Core is static so seek suffices.
- `wander(state)` — small steered jitter for idle drift.
- A `boid(pos, vel, neighbours)` combiner = separation + cohesion + alignment.

Facing: reuse `enemy::face_heading` + the `FaceTarget` component (already added) so
an enemy can rotate to its **heading** (flying nose-first) or to a **target** (the
Core, when about to fire). All movement is thrust → `approach(vel, desired, accel)`
then a speed cap, so turns ease in (banking) instead of snapping.

Telegraph: a brief wind-up before an attack — a `Telegraph { secs }` component that
scales/flashes the silhouette (reuse `render::telegraph_fx` / status auras) and
locks facing toward the Core, so attacks read.

## Per-enemy maneuvers (ported from `movement.js`)

| Enemy (dps) | rainboids base | Maneuver to implement |
|---|---|---|
| **Hunter** (+AshenDetonator, TeslaWraith) | `dive_bomber` | DONE: orbit→strafe(face Core, fire)→reposition. Upgrade to the full **APPROACH→HOVER→DIVE→RETREAT** state machine: hover at range telegraphing, then thrust hard *through* the Core (dive), overshoot, loop back. |
| **Stalker** (+FrostLance) | `zigzag` | **Zigzag-thrust**: every ~1s pick a new heading toward the Core ±0.6 rad, *accelerate* along it, smooth-rotate (`lerp_angle`, 0.2) toward it. Sharp, aggressive darts. |
| **Wasp** (+Cinder, LumenDrone) | `swarm` | **Boid flock**: separation×1.5 + cohesion×0.5 + alignment×0.3 + weak Core-seek×0.4. Fast, erratic swarm; face heading. |
| **Sentinel** (+FrostLance sniper) | `sniper` | **Kite**: hold a stand-off ring from the Core; `flee` (thrust away) if something gets close; slow-aim rotate; telegraph a charged beam then fire. |
| **Prowler** (+SporeCarrier, Warden) | `circler` | **Orbit-strafe**: orbit the Core at ~200r, thrust toward the moving orbit point, face the tangent; periodic radial volley. (Same family as Hunter's orbit; distinct radius/speed.) |
| **Tangerine** (+Plaguebearer, Hydra) | `teleporter` | **Blink-strike** state machine: DRIFT → CHARGE (telegraph flash) → BLINK (instant reposition near the Core) → STRIKE (fire) → COOLDOWN drift out. |
| **Titan** (+Glacier) | `tank` | **Heavy advance**: slow relentless thrust at the Core with high inertia (low accel/decel), slow rotate-to-face; heavy cannon volley. Feels weighty. |
| **Guardian** | `shielder` | Slow advance, rotate the frontal shield to face the Core; minimal thrust, steady rotation. |
| **Weaver** | `sine` | **Sine-weave**: thrust toward the Core with a sinusoidal lateral oscillation; bank (rotate) into the weave. |
| **Drifter** | `straight` | Gentle accel toward the Core (already has a light pull); add slight wander so it isn't a straight line. |

## Implementation status (2026-05-31)

**DONE** — thrust+rotate maneuvers shipped for the main archetypes (each covers its
elemental reskins): Stalker **zigzag-thrust** (+ sniper standoff/flee + face-Core),
Wasp **boid flock** (separation+cohesion+alignment snapshot + wander), Sentinel
**kite** (standoff sway + flee-when-close + aim), Prowler **orbit-strafe** (radius
spring + tangential thrust), Hunter **dive-bomber** with a real DIVE lunge phase
(`Diving` marker). Steering gained `flee`/`cohesion`/`alignment`/`wander` (+ tests).
**TODO** (lower priority): Tangerine blink-strike, Titan heavy-advance weight,
Weaver sine-weave, Guardian shield-facing, Drifter wander polish, explicit
firing-telegraph wind-ups, and the named bosses.

## Approach (original implementation order, one kind per step)

1. **Steering primitives** — add `flee`, `wander`, `boid` to `systems::steering` (+ tests).
2. **Stalker zigzag-thrust** — the clearest "thrust + rotate + maneuver" win.
3. **Wasp boid flock** — separation/cohesion/alignment (needs a neighbour snapshot; collect positions once per tick to avoid query aliasing).
4. **Sentinel kite** — flee-when-close + aim telegraph.
5. **Prowler orbit-strafe** — reuse the Hunter orbit shape with its own radius/cadence.
6. **Tangerine blink-strike** — state machine using `AiState.phase` as the state timer.
7. **Titan heavy advance** + **Weaver sine-weave** + **Guardian/Drifter** polish.
8. **Telegraphs** — wind-up before firing across the firing enemies.

Each kind keeps its `Enemy { kind }` + reuses `AiState { wander, phase }` (2 Vec2
floats + 1 float) for state; where more state is needed, add a tiny per-kind marker
component. Verify with headless steering tests + a demo screenshot per batch.

## Bosses (future, separate effort) — "Massive Maneuver-Around Bosses"

Three multi-phase, large, *moving* bosses (rainboids `BOSS_TYPES` + the 2026-05-30
boss doc), each with destructible weak-points + telegraphed phase attacks:
- **Sentinel Prime** — rotating fortress: radial spirals → sweeping beams + adds → all-dir barrage; slow drift + rotating arms.
- **Hive Tyrant** — mobile carrier: telegraphed charges across the arena, swarm spawns, hazard trails.
- **Void Reaver** — 4-phase teleporter: blinks, void singularities that pull, homing void bolts.

dps currently has only generic `Boss { tier }` HP/size overlays on Titans. The named
bosses are deferred until the standard-enemy maneuvers land.
