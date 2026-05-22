# Exact Port Specification — Rainboids (solo) → `dps` (Rust + Bevy)

**Status:** reference spec for a *faithful 1:1 port*. Reverse-engineered directly
from the JavaScript source at `/Users/silvr/projects/rainboids/js/modules/*`
(the canonical solo build, version tag 6.x). Every number, color, formula, and
vertex below is transcribed from source; where the source contradicts itself
(stale comments vs. live constants) the **live value** is used and the
discrepancy is flagged.

**How to use this doc.** This is the *what* (exact behavior + data). The
*how/when* (staged delivery, Bevy idioms, the lyon/hanabi decision) lives in
[`port-plan.md`](port-plan.md); this document supersedes it on any specific
number. When porting a system, find its section here, reproduce the constants
verbatim, and only then decide the Bevy expression. Treat **bold "PARITY"**
notes as must-match invariants that headless tests should lock down.

> **Scope.** Solo campaign only — 30 waves / 10 stages, 10 enemy types, the
> live weapon/skill/powerup economy. Mobile/touch input, the multiplayer `sim/`
> crate, and retired/`hidden` systems are out of scope (noted where they shape
> live behavior). Where the JS keeps dead code paths (legacy upgrades, the
> disabled lens-flare nebula, unused movement functions), this doc ports the
> **live** path and names the dead one so you don't chase it.

---

## Table of contents

- [Part I — Core engine & foundations](#part-i--core-engine--foundations)
- [Part II — Player](#part-ii--player)
- [Part III — Weapons, skills & combat](#part-iii--weapons-skills--combat)
- [Part IV — Enemies](#part-iv--enemies)
- [Part V — Waves](#part-v--waves)
- [Part VI — World objects & drops](#part-vi--world-objects--drops)
- [Part VII — Rendering pipeline](#part-vii--rendering-pipeline)
- [Part VIII — HUD, UI, shop, audio, input](#part-viii--hud-ui-shop-audio-input)
- [Part IX — Bevy ECS mapping & phasing](#part-ix--bevy-ecs-mapping--phasing)
- [Appendix A — global constants quick reference](#appendix-a--global-constants-quick-reference)
- [Appendix B — parity invariants & gotchas](#appendix-b--parity-invariants--gotchas)

---

# Part I — Core engine & foundations

## I.1 The game loop & timestep (the single most important section)

**Gameplay runs at a fixed 60 Hz logic tick driven by an accumulator. Entities
do NOT scale movement by `dt`; they advance a constant amount per tick.** A
correct port uses Bevy `FixedUpdate` (or a manual accumulator) at exactly
**16.6667 ms**, never `Time::delta_seconds()` for simulation.

Driver is `requestAnimationFrame` (variable display rate) but logic is decoupled
(`game-engine.js` `gameLoop()`):

```text
dt = min(now - lastFrameTime, 100)        // CLAMP large gaps to 100 ms
logicAccumulator += dt
steps = 0
while logicAccumulator >= 16.6667 and steps < 4:
    if steps > 0: frameClock.now += 16.6667; frameClock.tick++   // advance clock on catch-up
    update()                               // ONE fixed logic tick
    logicAccumulator -= 16.6667
    steps++
if steps >= 4: logicAccumulator = 0        // spiral-of-death guard: drop backlog
```

| Constant | Value | Source |
|---|---|---|
| `LOGIC_HZ` | 60 | `constants.js:127` |
| `LOGIC_TICK_MS` | `1000/60 ≈ 16.6667` ms | `constants.js:128` |
| `TICK_SCALE` | `30/60 = 0.5` | `constants.js:129` |
| `maxLogicStepsPerFrame` | 4 | `game-engine.js:887` |
| frame-dt clamp | 100 ms | `gameLoop` |

`TICK_SCALE = 0.5` exists because the original speeds were tuned at 30 Hz; the
live constants pre-bake it: `MAX_V = 7*0.5 = 3.5`, `BULLET_SPEED = 16*0.5 = 8`,
`AST_SPEED = 3.5*0.5 = 1.75` (all **px/tick @60 Hz**). Timers that take a `dt`
are passed the *constant* `LOGIC_TICK_MS`, never real elapsed time — this is why
pausing freezes everything (the `update()` branch simply stops being reached).

**`frameClock`** (`frame-clock.js`): caches `Date.now()` into `frameClock.now`
once per advance and bumps `frameClock.tick`; all gameplay reads `frameClock.now`
rather than calling `Date.now()`. On catch-up steps it is manually advanced one
tick so cooldown timestamps and AI frame-parity don't stall.

**Hitstop**: when `_hitstopFrames > 0`, the loop decrements it and runs ONLY VFX
pools + player movement + camera + damage-number animation, then renders and
returns early — gameplay entities freeze but juice keeps moving. Global budget:
max 10 hitstop frames/second; light hits (<4 frames) rate-limited to once/200 ms;
kill hits (≥4 frames) always punch through; coalesced via `max`.

### I.1.1 Canonical update order (inside `update()`, PLAYING/WAVE_TRANSITION)

Reproduce this order exactly; **spawning runs LAST, after collisions & cleanup.**

1. Tick `_gameTimers` backwards by `LOGIC_TICK_MS`, splice finished.
2. `survivalTime += LOGIC_TICK_MS` (tick-accumulated, not wall clock).
3. Gather input → `Intent`.
4. `player.update(...)`.
5. Passive AoE powerups: `tickStaticDischarge()`, `tickWhirlwind()`.
6. `updateCamera()`.
7. UI: money-pickup, damage-numbers, hover detection, kill-streak.
8. Player bullets update → cleanup.
9. VFX pools: particles, line-debris, asteroid-shards.
10. Powerups update.
11. Asteroids update → cleanup.
12. `formationManager.update()` (BEFORE enemy AI so slot targets are fresh).
13. Enemies update → cleanup → `_separateEnemies()` soft push-apart (`push = overlap*0.55`, 50/50 split, no damage).
14. Enemy bullets update → cleanup.
15. Color-stars, gold coins/shapes (tractor pull), background stars (parallax drift).
16. `handleCollisions()`.
17. Powerup-display fade.
18. Periodic pool cleanup (every `PARTICLE_CLEANUP_INTERVAL = 30` s).
19. `updateWaveSystem()`  ← spawning.
20. `updateScore(money)`.

Other states: GAME_OVER/PAUSED/SHOP tick only VFX + background stars. TITLE only
animates background drift + nebula + title animation.

## I.2 Coordinate system, world & camera

- **World/field** = fixed logical resolution **1920 × 1080** (`FIELD_WIDTH/HEIGHT`).
  `+X` right, `+Y` **down**. Angles via `atan2` (radians). All boundary checks use
  the field, not the screen.
- **Screen/canvas** = `window.innerWidth/innerHeight`. **No devicePixelRatio
  scaling** — all layers share one CSS-pixel grid. (Bevy: render at window res,
  one camera; world is a logical 1920×1080 space the camera maps into.)
- **Camera** (`camera-manager.js`): `{x, y, smoothing: 0.1, zoom: 1}`; `(x,y)` is
  the top-left of the view in world coords. Follows player centered:
  `targetX = player.x - width/2`; lerp `camera.x += (targetX - camera.x)*0.1`/tick.
  Clamped so the view stays inside the field (field centered if view larger than
  field on an axis). Desktop zoom = 1.0 (mobile 0.78/0.88 — ignore).
  `screenToWorld` at zoom 1: `world = screen + camera`.
- **Screen shake** `triggerScreenShake(duration, magnitude, asteroidSize)`:
  `sizeMult = max(1.5, asteroidSize/20)`, +random duration (0–8) & magnitude (0–5),
  only applied if stronger than current. Applied as a translate of
  `sin/cos(time)*intensity*0.25 + random*intensity*0.75` (Vlambeer-style),
  decremented one unit/rendered-frame. **Camera kick**: directional offset that
  decays `*0.7`/frame, snapped to 0 below 0.3. **Flashes**: separate white & gold
  alpha channels with their own timers.

## I.3 RNG & math (`utils.js`)

- **Gameplay RNG is unseeded `Math.random()`.** `random(a,b) = rand*(b-a)+a`.
  **PARITY: the port is non-deterministic by design; do not seed gameplay RNG.**
  Headless tests must assert on ranges/invariants, not exact sequences.
- A **seeded** LCG exists ONLY for noise/starfield placement:
  `state = (state*9301 + 49297) % 233280; return state/233280`. Not used by gameplay.
- `wrap(obj, w, h)`: single-step toroidal wrap (only used as a dead fallback —
  live entities bounce off field edges instead).
- `collision(a,b)`: circle test `hypot(dx,dy) < a.r + b.r`.
- `starCollision(player, star)`: swept test for fast collectibles — circle test
  with `+15` bonus radius, plus next-frame position, plus segment projection of
  player onto the star's velocity clamped `t∈[0,2]`.
- `isCirclePolygonColliding`: point-in-polygon (ray cast) OR any edge within
  radius. Used for ship-vs-asteroid.
- No generic `lerp`/`distance`/`clamp` exports — code inlines `hypot`, `atan2`,
  `a + t*(b-a)`.

## I.4 Spatial grid (`spatial-grid.js`)

Uniform grid **8 cols × 6 rows** over 1920×1080 → cells **240 × 180**.
Pre-allocated 48 cell arrays, cleared by `length=0` each frame (no GC).
`insert` spans all cells the entity's circle overlaps; `retrieve` dedupes via a
Set and excludes self. Each tick `handleCollisions` clears then bulk-inserts
`asteroidPool`, `enemyPool`, `enemyBulletPool`. Player-vs-asteroid is brute force;
bullet-vs-* and player-vs-enemy-bullet use the grid. Stars/gold/powerups are not
gridded (direct distance / tractor logic).

## I.5 Object pools (`pool-manager.js`)

Generic pool: `get()` pops free or allocates (grows unbounded), calls `reset(...)`,
tracks `_poolIndex` for O(1) swap-and-pop `release()`. `cleanupInactive()` frees
`active===false`. Initial sizes:

| Pool | Class | Initial |
|---|---|---|
| bulletPool | Bullet | 10 |
| particlePool | Particle | 2500 *(soft — grows on demand)* |
| lineDebrisPool | LineDebris | 100 |
| asteroidShardPool | AsteroidShard | 200 |
| asteroidPool | Asteroid | 5 |
| enemyPool | Enemy | 5 |
| enemyBulletPool | EnemyBullet | 20 |
| colorStarPool | ColorStar | 35 |
| goldCoinPool | GoldCoin | 60 |
| goldShapePool | GoldShape | 20 |
| backgroundStarPool | BackgroundStar | 60 |
| powerupPool | Powerup | 5 |

Caps: `MAX_ASTEROIDS = MAX_WAVE_ASTEROIDS = 16`; particle pool soft cap 2500
(grows, no eviction since 6.16.1, peak ~3000); bullet soft cap 300 (evicts oldest
non-piercing). In Bevy, "pool" = archetype storage; spawn/despawn directly. Keep
the **300 active player-bullet eviction** and **16 asteroid** caps as parity.

## I.6 Timers (`game-timer.js`)

`GameTimer(durationMs, cb)` is frame-counted: `tick(dt)` subtracts; fires once at
≤0. Only advanced inside `update()` under PLAYING/WAVE_TRANSITION with the
constant `LOGIC_TICK_MS`. Pause = no tick = frozen. (Bevy: store as `Timer`s
ticked only in `FixedUpdate` gated on the playing state.)

## I.7 Game states & flow (`game-state.js`, `GAME_STATES`)

State machine validated against a transition table; an `epoch` counter bumps on
each change to invalidate stale callbacks.

| State | Meaning | Valid targets |
|---|---|---|
| `TITLE_SCREEN` | animated title | PLAYING, WAVE_TRANSITION |
| `PLAYING` | active combat | PAUSED, SHOP, WAVE_TRANSITION, GAME_OVER, GAME_COMPLETE |
| `WAVE_TRANSITION` | between waves/pulses | PLAYING, SHOP, PAUSED, GAME_OVER, GAME_COMPLETE |
| `PAUSED` | pause/overlay | PLAYING, WAVE_TRANSITION, SHOP |
| `SHOP` | upgrade shop | PLAYING, WAVE_TRANSITION, PAUSED, GAME_COMPLETE |
| `GAME_OVER` | death screen | TITLE, PLAYING, WAVE_TRANSITION |
| `GAME_COMPLETE` | victory stats | TITLE, PLAYING, WAVE_TRANSITION |
| `ORIENTATION_LOCK` | mobile only | (ignore) |

Flow: Title→Playing on start. Wave clear → WAVE_TRANSITION (see Part V). Shop is
**not** auto-opened between waves (changed 5.74.2) — only via button/pause. Pause
on Esc or window blur. Death → GAME_OVER. Clear wave ≥30 → GAME_COMPLETE.

The current `dps` uses `GameState { Playing, Paused, GameOver }`; extend to
`Title, Playing, Paused, Shop, WaveTransition, GameOver, GameComplete`.

## I.8 Persistence (`storage.js`, `sp-stats.js`)

localStorage keys (all try/catch wrapped): `rainboidsSettings`, `rainboidsSave`
(run checkpoint, `SAVE_SCHEMA=1`, written at each wave start), `rainboidsMeta`
(cross-run meta, merge-written, NOT cleared by New Game),
`rainboidsSurvivalRecord`, `rainboidsAssists`, `rainboids:mobile-stick-side`.

Run-save schema (`serializeRunState`): `{schema, savedAt, wave, money, engineTanks,
stats{...}, player{level, experience, …, health, maxHealth, shield, healthTanks,
activePrimary, activePower, activeSkill, ownedPrimaries[], ownedPowers[],
ownedSkills[], powerups{type:{stacks, isPermanent}}}}`.

Meta schema (`rainboidsMeta`): `{level, xp, sp, spStats, money, powerups{type:stacks},
equippedItems{slot:item}}`. SP system: `MAX_LEVEL=100`, `SP_STAT_MAX_POINTS=20`,
XP curve `xpForLevel(L) = 500 + (L-1)*250`, +1 SP/level. (Bevy: `serde` + `ron`
to the OS config dir.)

---

# Part II — Player

Ship-local convention: after `translate(x,y)` then `rotate(angle + π/2)`, ship
**−Y is the nose/forward**. All geometry scales by `r = radius`.

## II.1 Movement physics

Velocity-based with exponential friction, once per fixed tick. **No turn-rate
physics** — `angle` is set instantly to face the aim point each tick
(`angle = atan2(aimY−y, aimX−x)`). Movement is 8-direction strafe (WASD),
decoupled from facing; no banking.

Per-tick integration (`player.js:685–748`):
1. Build move vector from keys (up = −Y): `moveX -=/+= 1`, `moveY -=/+= 1`.
2. `moveAngle = atan2(moveY, moveX)`.
3. `thrustForce = thrustPower * speedMult`; `vel += (cos,sin)(moveAngle)*thrustForce`.
4. Friction `vel *= pow(0.50, TICK_SCALE) ≈ 0.7071`.
5. Snap-to-zero if `|vel.axis| < 0.05`.
6. Clamp: `effMaxV = MAX_V*(1 + (speedMult−1)*0.7)`; rescale if `hypot(vel) > effMaxV`.
7. `x += vel.x; y += vel.y`.
8. Edge handling.

| Stat | Value |
|---|---|
| `thrustPower` (base) | `2.0 * TICK_SCALE = 1.0` |
| friction/tick | `pow(0.50, 0.5) ≈ 0.7071` |
| snap threshold | `|vel| < 0.05` |
| `MAX_V` base cap | 3.5 |
| effective cap | `MAX_V*(1 + (speedMult−1)*0.7)` |
| `speedMult` | `1 + SPEED_BOOST_stacks*0.65 + (item speed% + SP SPEED%)/100` |
| live radius (collision) | **15** (`SHIP_SIZE*scale/2 = 30/2`; constructor's `12` is overwritten) |
| mass | `π·r²·0.5` |
| initial facing | `−π/2` (up) |
| initial pos | screen center, `vel=(0,0)` |

> Note `constants.js` lists `SHIP_THRUST 0.12` / `SHIP_FRICTION 0.993`, but the
> live player uses `thrustPower = 2.0*TICK_SCALE = 1.0` and friction
> `pow(0.5, TICK_SCALE)`. **Use the player.js values.**

**Edge = damped bounce, not wrap.** On hitting a field edge: clamp position to
`r`/`dim−r`, reflect velocity at `±|vel|*0.8`. (Torus `wrap` only runs as a dead
fallback when no field is passed; live calls always pass the field.)

### II.1.1 Dash (SHIFT) — fixed-velocity burst

| Stat | Value |
|---|---|
| duration | 250 ms |
| cooldown | 1500 ms |
| distance | 135 px |
| dash speed | `135*1000/250 = 540` px/s |
| direction | aim angle; if `hypot(vel) > 0.5` use velocity direction (mid-strafe) |
| i-frames | active burst (`isDashing && dashTimer>0`) + tail `makeInvincible(250 + 1000 + 2000*PHASE_ECHO_stacks)` |

Guards: no-op if already dashing or `dashCooldown>0`. Dash velocity integrated in
`skills.updateActiveSkills` (dt = `LOGIC_TICK_MS`). Total invuln window:
1250 ms base / 3250 ms (1 PHASE_ECHO) / 5250 ms (2). Trail color `#aa88ff`.

### II.1.2 Thrust juice (FX only, not physics)

`thrustLevel` ramps `+0.08`/tick toward 1 while moving, decays `−0.03`/tick when
idle; idle→thrust after >1200 ms idle sets `engineStartup = 1.0` (decays
`−0.06`/tick). Drives engine plume length and a startup shudder
`(sin(t*65)*shudder, cos(t*85)*shudder*0.7)`, `shudder = engineStartup*2.2`.

## II.2 Health, shield, lives/tanks, damage pipeline

| Stat | Value |
|---|---|
| starting / base max HP | 40 / 40 |
| max HP cap | 600 |
| base shield (damage reduction) | 15% |
| shield cap | 75% |
| starting spare tanks (`healthTanks`) | 1 (engine init); `MAX_HEALTH_TANKS = 3` |
| total effective tanks | `healthTanks + 1` (active bar is the "+1"), cap 4 |
| base crit chance / cap | 8% / 60% |
| base crit damage | 200% (2×); roll `randUniform(200, 300 + bonuses)`, cap 550% |

Effective max HP = `min(600, 40 + HEALTH_BOOST*35 + item hp + SP HEALTH)`.
Effective shield = `min(75, 15 + SHIELD_BOOST*8 + item toughness + SP TOUGHNESS)`.

**`takeDamage` pipeline (in order):**
1. `invincible` → 0. 2. dash i-frame → 0.
3. **DODGE** roll `min(0.5, DODGE*0.05 + (item+SP)/100)` → ignore.
4. **REFLEXES**: one free dodge / 30000 ms → `makeInvincible(700)` + ignore.
5. Shield: `reduced = dmg*(1 − shield/100)`.
6. (mobile early-wave mult — desktop 1.0).
7. **BULWARK** active: `reduced *= 1 − (IRON_WILL ? 0.65 : 0.5)`.
8. **STATIC_FIELD**: absorb up to `2*stacks` from a regenerating buffer.
9. `final = round(reduced)`; `health = max(0, health − final)`.
10. On loss: break kill streak; **THORNS** reflect `25%*stacks` to source;
    **RETALIATION** AoE pulse (`R=180`, dmg 8) if Bulwark active.
11. On lethal: **GUARDIAN** (clamp 1 HP + invuln, once/wave) → **LAST_STAND**
    (survive at 1 HP, 2500 ms invuln, once/run) → consume tank → death.

**There is NO post-hit invulnerability grace** (removed 5.88.0). The only invuln
windows are deliberate skills (REFLEXES 700, LAST_STAND 2500, dash tail). Tank
consumption refills HP in place (no respawn/delay/invuln) + coin SFX + gold flash.

**Health overflow → tanks**: `TANK_OVERFLOW_HP = 40`; overheal credit accumulates
`_tankProgress += credit/40` (×2 with BLOOD_BANK); each full unit grants +1 tank
to max 3. **Passive regen** = `min(3.0, REGEN*0.5 + item regen)` HP/s, gated to
after `4000 ms` since last damage; at max HP feeds tank overflow.

**Death sequence**: state→GAME_OVER, player explosion, screen shake (25,15,50),
hitstop 15, camera kick 25 opposite facing, 12-vertex hull shatter, staged
explosion rings/shrapnel/embers at 100/160/220/400/650 ms, palette
`#ffffff #78ebff #ff5ad2 #be96ff #ffff96 #00ccff`, overlay timer 90 frames.

## II.3 Progression / SP / energy

**In-run leveling is RETIRED** (6.0.0): `level=1`, `experience=0`,
`experienceToNextLevel=Infinity`, `skillPoints=0` are inert; `gainExperience`/
`levelUp` are no-ops. In-run growth = gold-bought powerups + item affixes +
survivor-card picks only.

**Meta progression** (persistent, see I.8): `MAX_LEVEL=100`,
`xpForLevel(L)=500+(L−1)*250`, +1 SP/level, `SP_STAT_MAX_POINTS=20`, per-point
value `stat.max/20`. The 8 SP stats and caps:

| id | max @20 | feeds |
|---|---|---|
| HEALTH | 400 | max HP (cap 600) |
| TOUGHNESS | 50 | shield % (cap 75) |
| VAMPIRISM | 50 | lifesteal % |
| THORNS | 100 | reflect % |
| CRIT_CHANCE | 50 | crit % (cap 60) |
| CRIT_DAMAGE | 200 | crit dmg % (cap 550) |
| DODGE | 50 | dodge (cap 0.5) |
| SPEED | 100 | thrust & top speed |

**Energy meter** (6.29.0): `energy=0`, `maxEnergy=100`, reset each run. Built `+4`
per landed hit; spent to fire power weapons. **Gold Find** = `1 + max(0,wave−1)*0.10`.

## II.4 Ship silhouette geometry (`player/renderer.js`)

Hero asset. Coords scale by `r=15`. Composite `'lighter'` for fills (after a black
outline pass). Cached Path2D set:

```text
rightWing:  (0.32r,−0.18r)→(1.12r,0.28r)→(0.82r,0.68r)→(0.28r,0.58r) Z
leftWing:   mirror in X
rightTip:   (1.12r,0.28r)→(1.42r,0.08r)→(1.18r,0.56r)→(0.82r,0.68r) Z
leftTip:    mirror in X
centralHull:(0,−r)→(0.32r,−0.18r)→(0.28r,0.58r)→(0,0.38r)→(−0.28r,0.58r)→(−0.32r,−0.18r) Z
```

Full outline (16 pts, for glow/hit-flash):
`(0,−r)(0.32r,−0.18r)(1.12r,0.28r)(1.42r,0.08r)(1.18r,0.56r)(0.82r,0.68r)(0.42r,0.78r)(0.28r,0.58r)(0,0.38r)(−0.28r,0.58r)(−0.42r,0.78r)(−0.82r,0.68r)(−1.18r,0.56r)(−1.42r,0.08r)(−1.12r,0.28r)(−0.32r,−0.18r)` Z.

Sub-shapes: **engine pods** ×2 at `(±0.42r,0.78r)`, ellipse `(0.13r,0.09r)`;
**cockpit** at `(0,−0.42r)`, ellipse `(0.17r,0.21r)`; **nose** arc `0.075r` at `(0,−r)`;
**hull panel lines** (center `(0,−0.75r)→(0,0.3r)` + two angled).

| Piece | Fill | Stroke | lw |
|---|---|---|---|
| black outline | — | `#000` | 4 wings/tips, 5 hull, 2.6 pods, 2.4 cockpit |
| wings | `rgba(0,90,180,0.45)` | `#0088ff` | 1.6 |
| wing tips | `rgba(0,160,255,0.25)` | `#44aaff` | 1.1 |
| central hull | `rgba(0,25,55,0.92)` | `#00ccff` | 2 |
| panel lines | — | `rgba(0,200,255,0.35)` | 0.8 |
| engine pods | `#001530` | `#0066ff` | 1.2 (+glow `#0088ff` r=0.13r) |
| cockpit | radial `rgba(160,235,255,0.95)`→`rgba(0,110,200,0.75)@0.55`→`rgba(0,50,110,0.25)` | `rgba(140,220,255,0.6)` | 0.9 (+glow `#aaeeff` r=0.21r) |
| nose | `rgba(200,245,255,0.9)` | — | (+glow `#ffffff` r=0.075r) |
| hull outline glow | — | `rgba(0,180,255, 0.3 + thrustLevel*0.3)` | 2.5 |

Engine exhaust ellipses at `(±0.42r,0.78r)`: length `r*(0.45 + thrustLevel*1.1)*(0.8 + engPulse*0.4)`,
width `0.12r`, gradient `rgba(255,220,120,0.9p)`→`rgba(255,80,10,0.6p)@0.35`→0, glow `#ff8800`.
Invincibility flashes globalAlpha 0.35/0.85 via `sin(now*0.02)>0`. Cooldown ring at
ship tip `(0,−r−14)` r=8 shows the defense-skill recharge.

## II.5 Player bullet (`player/bullet.js`)

| Property | Value |
|---|---|
| spawn offset | `+ (cos,sin)(angle) * SHIP_SIZE/1.5 = +20 px` |
| base radius | `4 * scale = 4` |
| speed | `(cos,sin)(angle) * BULLET_SPEED`, `BULLET_SPEED = 8` |
| `maxLife` | `round(240 / TICK_SCALE) = 480` ticks (~8 s, full field) |
| mass | 1 |
| trail | ring buffer length 16 |

Update: record trail (pre-move) → `life++` → expire at `life≥effMaxLife`
(spawn 5–8 sparkle puff) → fade → homing (if active) → move → off-field despawn
(50 px margin). **Fade/shrink**: over final 35% of life, `fadeFactor =
remaining<0.35 ? remaining/0.35 : 1`; `radius = base*(0.3 + 0.7*fadeFactor)`.

Default visual `#FFFF00` (glow `#FFDD00`, intensity 8), drawn as a **comet**:
gradient tail (length `size*2` opposite velocity), solid head, white center
`arc(radius*0.5)`. Trail in `'screen'` composite, per-segment alpha `(i+1)/count`.
Powerup overrides (later wins): RAPID_FIRE `#ff6600`/triangle, MULTI_SHOT
`#66aaff`/hexagon, SPEED_BOOST `#ffff33`, BIG_BULLETS `#66ff66` size×1.2,
piercing `#ffcc66`/diamond, homing `#ff66cc`/diamond, EXPLOSIVE `#ff9933`/star,
overcharged `#ffeb44`/star.

**Homing**: target within 400 px (nearest-to-cursor → nearest-to-bullet;
enemies > enemy-mines > asteroids), lead 8 frames, max turn 0.15 rad/frame,
distance-scaled strength, maintains `BULLET_SPEED*1.1`. **Explosive**: radius 30,
AoE `dmg = max(1, ceil(2*(1−d/r)))`, 15 `#ff6600` particles. **Piercing**: hits
`piercing+1` targets, tracks a `hitTargets` set.

---

# Part III — Weapons, skills & combat

`damage` values are small multipliers (enemies have low HP; asteroids 1–5 HP at
L1). All primaries are FREE and fire while LMB held, gated only by fire rate
(no ammo/clips). **Mobile early-wave damage ramp** (W1×3.0…W6+×1.0) is desktop=1.0.

## III.1 Primary weapons (`PRIMARY_WEAPONS`)

| id | name | fireRate ms | damage | bulletSpeed× | bulletSize× | count | spread rad | pierce | range× | unlockWave | color |
|---|---|---|---|---|---|---|---|---|---|---|---|
| PULSE_CANNON | Pulse Cannon | 400 | 1.2 | 1.0 | 1.0 | 1 | 0 | 0 | 1.0 | 0 | `#00ccff` |
| STORM_NEEDLES | Storm Needles | 130 | 0.4 | 1.1 | 0.5 | 1 | 0.20 | 0 | 1.0 | 3 | `#b3ff44` |
| SCATTER_GUN | Scatter Shot | 700 | 0.42 | 0.9 | 0.6 | 5 | 0.4 | 0 | 1.2 | 5 | `#ff8844` |
| RAIL_DRIVER | Rail Driver | 1200 | 3 | 1.4 | 1.2 | 1 | 0 | 99 | 0.85 | 8 | `#ff44ff` |
| CLUSTER_LAUNCHER | Cluster Launcher | 800 | 50 | 1.0 | 1.4 | 1 | 0 | 0 | 9999 | 10 | `#ff5544` |

- **Fire rate** = inter-shot interval ms; effective `= round(fireRate*(1−0.12)^RAPID_stacks)`. Fires when `now − lastShot ≥ effRate` and held.
- **Effective damage** = `config.damage × (PULSE: 1+0.15*OVERCHARGE) × mobileRamp`, then per-weapon + global modifiers.
- bulletSpeed/Size/range multiply pool defaults (8 px/tick, r=4, maxLife 480).

Per-weapon firing:
- **Pulse**: 1 bullet via `createChargedBullets`; multishot fan inside.
- **Storm Needles**: `count = 1 + NEEDLE_MULTI(+1 HAILSTORM)`; fan `min(0.5, 0.10*(count−1))`; each needle ±0.10 rad random jitter ("cone of fire"); radius×0.5.
- **Scatter**: `pellets = 5 + BUCKSHOT + SCATTER_MULTI(+2 CONE_OF_FIRE)`; fan across ±0.2 rad with ±0.025 jitter.
- **Rail**: double-helix pair (2 bullets/pair, `pairs = 1 + RAIL_MULTI`); helix `AMP=9px`, `FREQ=0.42 rad/tick`, phases 0/π; built-in pierce 99.
- **Cluster**: bomb flies straight at cursor at v=12; on first contact blast `r=90, dmg=50`, scatters 5 sub-bomblets (`speed 4, friction 0.94, life 20f, r=50, dmg 25`). Exempt from global homing/pierce/explosive.

## III.2 Weapon upgrades — the live 8-trait system (`PRIMARY_UPGRADES`, 6.28.0)

Kinetic primaries (Pulse/Storm/Scatter/Rail) get all 8; Cluster gets Multi/Stun/Knock.
Same mechanic, weapon-flavored names. **Cost = base × `UPGRADE_COST_MULT(13)` ×
`1.6^(stack−1)`, rounded to nearest 500, floor 500.**

| Trait | maxStacks | base cost | effect/stack |
|---|---|---|---|
| `_MULTI` | 3 (Cluster 2) | 1800 (2000) | +1 projectile, fanned `min(0.8, 0.12*(count−1))` |
| `_RAPID` | 4 | 1200 | fireRate `×(1−0.12)^stacks` |
| `_PIERCING` | 3 | 1500 | `piercing += 1` |
| `_BIG` | 3 | 1200 | `radius += 2.2 px` |
| `_EXPLODE` | 3 | 1800 | explosive, `explosionRadius = 30 + 10*stacks` |
| `_HOMING` | 3 | 1600 | homing, `strength = min(0.4, 0.09*stacks)` rad/frame |
| `_STUN` | 3 | 1500 | `stunChance = 0.12*stacks` (non-lethal hits) |
| `_KNOCK` | 3 | 1300 | `knockbackChance = 0.15*stacks`; proc = 16 px shove |

The `PRIMARY_UPGRADES_LEGACY` block (DEAD_EYE, OVERCHARGE, SLUG_ROUND, …) is
commented out; firing code still *reads* those IDs but they're inert. **Port the
8-trait set; skip legacy branches** (or keep dormant for save-compat).

## III.3 Power weapons (`POWER_WEAPONS`) — energy-gated (6.29.0)

Right-click / Space. Energy cost spent per fire; per-weapon `cooldown` is now a
short anti-spam floor. `isPowerReady = powerCooldown≤0 && energy≥cost`. Energy
costs: CHARGE_SHOT 20, MINE_LAYER 25, LIGHTNING_ARC 30, NOVA_BLAST 45,
MISSILE_SALVO 55, LANCE_BEAM 60.

| id | name | cooldown | cost | unlockWave | color | key stats |
|---|---|---|---|---|---|---|
| CHARGE_SHOT | Charge Shot | 0 (charge) | 0 | 0 | `#00e6aa` | hold 3000–5000 ms |
| MINE_LAYER | Seeker Mines | 4000 | 1500 | 2 | `#ff3300` | maxMines 3, trigger r 60, blast r 80, dmg 3 |
| NOVA_BLAST | Nova Blast | 8000 | 2000 | 3 | `#ffaa00` | ring r 320, dmg 4, dur 600 |
| MISSILE_SALVO | Missile Salvo | 10000 | 3000 | 7 | `#ff4444` | count 3, dmg 1.5, speed 4, homing 0.18 |
| LANCE_BEAM | Lance Beam | 8000 | 0 | 12 | `#44ff44` | 0.05/tick, range 0.9, dur 3000, width 6 |
| LIGHTNING_ARC | Arc Lightning | 8000 | 0 | 5 | `#a855ff` | 0.05/tick, chainRange 360, dur 3000 |

- **Charge Shot**: hold ≥3000 ms; `baseDmg = 1 + 0.5*CHARGE_POWER`; `sizeMul = 1 + (t/1000)*0.4`; `speedMul = 1 + (t/1000)*0.2`; `+(t/1000)*0.6` dmg; `+(t/1000)*0.04` crit chance; homing `min(0.15, (t/1000)*0.03)`. Visual cyan `#00FFFF` if size>1.5 else white >1.2.
- **Mine Layer**: `maxMines = 3 + EXTRA_PAYLOAD`; arm 1000 ms, life 12000 ms; seeks (max speed 1.4, accel 0.06, turn 0.08) + magnetic pull. Default turret pulse every 800±200 ms (dmg ×0.5). Detonation: falloff `dmg*(1 − d/r*0.5)`, knockback 12 (ast 6). DAISY_CHAIN 220 px.
- **Nova Blast**: `maxRadius = 320 + 40*SHOCKWAVE`; ring over 600 ms, width 30, hit once per band crossing, dmg 4, knockback 16/9. DOUBLE_PULSE, AFTERSHOCK slow, NOVA_LIGHTNING stun, NOVA_INFERNO burn, NOVA_CHAIN (3 hops).
- **Missile Salvo**: `count = 3 + EXTRA_ORDNANCE`, one shared locked target (6.34.0); dmg 1.5, speed 4, homing 0.18, life 3000 ms, r=5; CLUSTER_WARHEAD splits ×3.
- **Lance Beam**: continuous `dur = 3000 + 100*LINGER`; width `6*(1+0.3*BEAM_WIDTH)`; dmg/tick `0.05*(TRIPLE_BEAM?2.5:1)*(1+0.15*LANCE_VELOCITY)`; range 360; stops at first hit; 15%/tick burn.
- **Arc Lightning**: continuous single-target tether 3000 ms; dmg/tick `0.05*(1+0.2*AMPLIFIER)*(ARC_OVERCHARGE?1.6:1)`; range 360; target nearest cursor; 25%/tick stun.

Full `POWER_UPGRADES` cost table (base, pre-scale; same ×13/×1.6 scaling): see
[Part VIII shop](#viii4-shop) — duplicated there with the rest of the tree.

## III.4 Defense skills (`DEFENSE_SKILLS`) — active, one equipped

SHIFT-tap cycles, Tab/Q/Space activates (per input map below); cooldown
auto-recharges. (Dash is a separate SHIFT core primitive, not a skill.)

| id | name | cooldown | duration | effect | unlockWave | color |
|---|---|---|---|---|---|---|
| BULWARK | Bulwark | 20000 | 4000 (+1000*FORTIFY) | 50% (65% IRON_WILL) damage resist | 2 | `#ffcc00` |
| REPAIR_NANITES | Repair Nanites | 25000 | 5000 (+2000*EXTENDED_CARE) | regen `3 + POTENCY` HP/s | 2 | `#44ff88` |
| DEFLECTOR_ORBS | Deflector Orbs | 15000 | 5000 | `3 + EXTRA_ORB` orbs at r=45, each blocks `3 + 2*HARDENED` bullets | 4 | `#44ddff` |
| EMP_PULSE | EMP Pulse | 22000 | 2000 | stun enemies within `200 + 60*WIDE_BAND` for 2000 ms | 5 | `#8888ff` |
| TRACTOR_SHIELD | Tractor Shield | 18000 | 4000 | forward arc `π/2 + (π/6)*WIDE_ANGLE`, absorbs bullets <55 px → `5 + 5*PROFIT` coins | 6 | `#ff88ff` |

`SKILL_UPGRADES` cost is **SP** (not gold-scaled). FORTIFY/IRON_WILL/RETALIATION;
POTENCY/EXTENDED_CARE/EMERGENCY_PROTOCOL (auto <20% HP); EXTRA_ORB/HARDENED_ORBS/
REFLECT; WIDE_BAND/EMP_OVERLOAD (+20% dmg to stunned)/CASCADE; WIDE_ANGLE/PROFIT/
REDIRECTION. (Full magnitudes in the source `SKILL_UPGRADES` table.)

## III.5 Defense items / passives

`DEFENSE_CONFIGS` (DEFENSE shop tab suspended but supplies pickup metadata):

| id | name | effect/stack | maxStacks | cost |
|---|---|---|---|---|
| HEALTH_BOOST | Health Boost | +35 max HP, full heal | 10 | 1200 |
| SHIELD_BOOST | Shielding | +8% DR (cap 75) | 8 | 1500 |
| SPEED_BOOST | Afterburner | +65% thrust | 4 | 2200 |
| HEALTH_DROP_FREQUENCY | Triage | shorten health-orb cooldown | 6 | 1800 |
| REFLEXES | Reflexes | free dodge/30 s (700 ms invuln) | 1 | 5500 |
| LAST_STAND | Last Stand | survive lethal at 1 HP (once/run, 2500 ms invuln) | 1 | 8000 |
| STATIC_FIELD | Static Field | +2 regenerating shield HP/stack after 8 s no-dmg | 3 | 3200 |

`PASSIVE_UPGRADES` = the **wave-clear survivor-card pool** (`PASSIVE_REWARD_IDS`):
CRIT_CHANCE (+7%, ×6), CRIT_DAMAGE (+15%, ×6), HEALTH_BOOST (+35, ×10),
SHIELD_BOOST (+8%, ×8), VAMPIRISM (heal 5% dealt, ×5), THORNS (reflect 25%, ×4),
DODGE (+5%, ×10), SPEED_BOOST (+65%, ×4).

## III.6 Collision & damage model (`collision-system.js`)

Broadphase via spatial grid (I.4). Collision pairs handled each tick:
player↔asteroid; player-bullet↔asteroid; asteroid↔asteroid (elastic along normal,
only when closing); player↔orbs/coins/shapes; player↔powerup; player↔enemy;
player-bullet↔enemy; cluster↔enemy; player-bullet↔enemy-mine; enemy-bullet↔player;
enemy-bullet↔asteroid (poof, rock unharmed); enemy-bullet↔enemy (friendly fire,
skips shooter); enemy↔asteroid (momentum only); weapon-effects (beam/mine/nova/
arc/missile/orbs/tractor).

**Damage to enemy** (`applyDamageToEnemy`, single entry point): blocked if
warping / `_deathFlash>0` / boss-rage invuln. `EMP_OVERLOAD` → ×1.2 on stunned.
`health -= dmg` clamped; destroyed at ≤0.001. **Vampirism** on applied damage.
`+4 energy` per landed hit. **Contact damage**: enemy→player uniform
`getLevelScaledDamage(25) = round(25*(1+(level−1)*0.30))`; player→enemy contact 5;
player→asteroid contact 2 (ramming non-viable by design).

**Crits**: chance `min(60, 8 + 7*CRIT_CHANCE + items + SP)`%; damage
`min(550, randUniform(200, 300 + 15*CRIT_DAMAGE + items + SP))`% — base crit is a
random 2.0×–3.0×. Roll `rand*100 < chance` → `×= critDmg/100`, recolor gold.

**Kill streak** (`STREAK_TIERS`, every 10 kills, `STREAK_BUFF_DURATION = 4000 ms`
refreshed per kill). **PARITY: streak resets only when the player takes damage,
never on a timer.** Multiplier applied before crit (compounds). Tiers (kills →
mult / goldMult): 10→1.25/1.05, 20→1.40/1.10, 30→1.55/1.15, 50→1.85/1.25,
70→2.12/1.32 (LEGENDARY: auto-splash unlocks `explosionRadius += 22` on every
bullet), 100→2.42/1.38, 150→2.78/1.45, 200→3.00/1.50 (cap "RAINBOIDS GOD").

**Knockback**: bullet trait proc = flat 16 px shove. Power-weapon knockback uses
`min(3.5, 1 + 0.4*KNOCKBACK)`. Bounce constants: `BOUNCE_RESTITUTION 0.9`,
`ASTEROID_KNOCKBACK_MULTIPLIER 22.0`, `OVERLAP_SEPARATION_RATIO 0.6`. **Stun**:
bullet `0.12*stacks`; arc 25%/tick; EMP within radius 2000 ms. **Piercing**: skips
already-hit enemies, dies after `onHit` when destroyed; Rail uses 99. `HIT_FLASH_FRAMES = 10`.

## III.7 Weapon visual effects (`weapon-effects-renderer.js`)

Universal recipe (no `shadowBlur`): wide black under-stroke → faint wide colored
"fake-glow" stroke → sharp colored stroke → thin white inner core.

- **Lance Beam**: lightning zig-zag to first hit, grow-in 150 ms cubic; segments `max(6, range/28)`, jitter `beamW*0.7`; strokes black (`w+3`), glow `#44ff44` (`w*2.5`, α0.45), main (`w`), white core (`max(1, w*0.3)`); glitter embers `#88ddff`/`#ffffff`.
- **Mines**: r=12 casing, 4 diagonal spikes, pulsing core, 3 blink phases (pre-arm/armed/urgent red), 6-LED ring, dashed magnetic ring at `triggerR*1.8`.
- **Nova ring**: expanding circle, α `1−progress`, black/`#ffaa00`/sharp strokes, wavefront sparkles.
- **Arc Lightning**: 3-strand renderer crossfading frayed-fan ↔ focused-tether (`lockBlend` 0.12/frame), heavy jitter + shimmer, 4 strokes/strand (`#a855ff`).
- **Missiles**: vector rocket, thruster flame, red body `#cc2222`/fins, pulsing nose, blink-out final 800 ms; impact flash 36 + 16 shrapnel.
- **Deflector Orbs**: filled circle r=6 `#44ddff`. **Bulwark**: circle r=35 `#ffcc00` α~0.3. **Tractor**: pie wedge r=50 `#ff88ff`. **EMP**: expanding ring over 500 ms `#8888ff`. **Dash trail**: ghost circle r=15 `#aa88ff`.
- Bullet recolors: crit `#FFFF00`, charged `#00FFFF`/`#FFFFFF`. Muzzle flares `#ffdd88`(pulse)/`#88ccff`(needles)/`#ffaa44`(scatter)/`#44ffaa`(rail).

---

# Part IV — Enemies

10 base types. **Bosses are not separate types** — they are TITANs (and others)
promoted via `bossTier` overlays. `TANGERINE` displays as **"Bomber"**.

## IV.1 Roster (level-1 base stats)

Contact damage is uniform (`getLevelScaledDamage(25)`).

| Key | Name | HP | radius | Points | Speed | Color | Glow | Move | Shoot |
|---|---|---|---|---|---|---|---|---|---|
| HUNTER | Hunter | 5 | 32 | 120 | 2.6 | `#ff4444` | `#ff6666` | hunter_arc | hunter_single (3-burst) |
| GUARDIAN | Guardian | 12 | 48 | 200 | 1.25 | `#44ff44` | `#66ff66` | square | guardian_spread (5-fan) |
| WASP | Wasp | 5 | 36 | 100 | 3.5 | `#ffff44` | `#ffff66` | wasp_zigzag | wasp_machinegun |
| STALKER | Stalker | 7 | 38 | 130 | 3.1 | `#44ffff` | `#66ffff` | arc | charged_laser |
| DRIFTER | Drifter | 9 | 38 | 180 | 3.1 | `#00ffff` | `#44ffff` | drifter_wave | arc_lightning |
| PROWLER | Prowler | 14 | 45 | 240 | 0.75 | `#ff00ff` | `#ff44ff` | keep_distance | missile |
| WEAVER | Weaver | 5 | 32 | 160 | 2.75 | `#ffff00` | `#ffff44` | weaver_spinup | spiral_laser |
| SENTINEL | Sentinel | 10 | 41 | 220 | 2.5 | `#00ff00` | `#44ff44` | weaver_spinup | sentinel_sweep (8-burst) |
| TANGERINE | Bomber | 10 | 45 | 160 | 2.0 | `#ff8844` | `#ffaa66` | chase | lay_mine |
| TITAN | Titan | 20 | 64 | 320 | 1.5 | `#ff44ff` | `#ff66ff` | boulder | sweep_laser |

**Per-spawn init scaling** (`initializeEnemy`): `maxHealth = round(baseHP*(1 + (level−1)*0.147))`;
size **does not** scale (`sizeMultiplier=1`); `scaledSpeed = speed*(1 + (level−1)*0.134)`;
`mass = π·r²·0.8`. Campaign-wide multipliers (Part V) then apply on top.

**Per-enemy AI config** (evasion / preferredRange / dodgeBullets):
HUNTER 0.65/250/yes, GUARDIAN 0.3/300/yes, WASP 0.7/200/yes, STALKER 0.6/200/yes,
DRIFTER 0.45/280/yes, PROWLER 0.3/400/no, WEAVER 0.6/180/yes, SENTINEL 0.3/280/no,
TANGERINE 0.15/150/no, TITAN 0.15/300/no. `turnSpeed` live default 0.08 (TITAN 0.06).

**Shared per-frame AI** (all enemies, always target player): evasive maneuvers,
avoid asteroids (BUFFER 70, force 0.14), maintain distance from player
(`keep = 120 + radius`, force 0.5) & from other enemies (force 0.3), dodge enemy
bullets (radius 40, lookahead 30, ×1.5) & player bullets (radius `25*(speed/3)`,
lookahead 25, ×0.8), micro-movements, fish-like undulation (freq 3, amp 0.15). In
waves ≥15 heavy scans run on alternating frames per `_aiOffset` parity.

## IV.2 Per-enemy movement (exact)

- **HUNTER** `hunterArc`: sticky one-way orbital strafe, `_arcRadius = 230+rand*80`,
  `omega = (0.020+rand*0.012)*vortex*(sling?1.4:1)`, `vortex = 1+0.5*sin(now*0.0012+angle*0.9)`;
  radius breathe `sin(now*0.0006+angle*1.7)*30`; perp weave `sin(now*0.0085+phase)*18`;
  35% lunge dice (target r 90, steer 0.20, cap speed×2.6), 60% slingshot dice
  (omega×1.4, cap speed×2.0). Default cap speed×1.7, friction ×0.92.
- **GUARDIAN** `square`: burst-dart cardinal axes. wait 2000 ms / burst 1200 ms,
  distance `screen/7…screen/5`, burst speed ×3.0, waiting friction ×0.85.
- **WASP** `zigzag`: 3–5 perpendicular segments (`perpAngle = toPlayer + (π/2)*dir`,
  speed ×2.6 + drift 0.4), flip each segment, then cooldown 2000–2800 ms (friction ×0.88).
- **STALKER** `arc` (3-phase, gates fire): swooping 3000 ms (arc center lerps 0.02,
  speed ×1.2) → charging 1200 ms (friction ×0.8, aim 0.08) → firing 800 ms (only-shoot window).
- **DRIFTER** `drifter_wave`: sinusoidal orbit at 220 px; `phase += 0.042`;
  radial `sign(d−220)*min(|d−220|*0.06, speed*0.8)`; tangential `sin(phase)*speed*1.6`;
  while charging, slow strafe (speed×0.40, smoothing 0.10).
- **PROWLER** `keep_distance`: patrol/dive/retreat. idealDist `280 + sin(now*0.0009)*40`;
  push/approach 0.05–0.06, strafe 0.04; dive rush 0.18 (cap speed×3.0); retreat 0.14.
- **WEAVER & SENTINEL** `weaver_spinup` (3-phase): spinning_up 2400 ms (`spinRate = (progress²)*0.26`,
  Sentinel must fire 3 bursts first) → arcing 3600 ms (`orbitRate = 0.028*dir`, Weaver
  fires spiral every 130 ms; Sentinel adds radial wave `sin(phase)*70`, speed ×2.8) →
  cooldown 2600 ms.
- **TANGERINE** `chase`: player-seeking roam, spread 0.35/0.85 by distance, re-rolled
  700–1600 ms; wall repulsion margin 180; accel 0.042, cap = speed; pause after mine (×0.82).
- **TITAN** `boulder`: idle (slow orbit, drift 0.45, smoothing 0.08) → approaching
  (accel 0.06 along locked angle, cap speed×3.0) → braking (friction ×0.93).

> ~20 other movement functions exist but none are assigned to a live type — skip them.

## IV.3 Per-enemy fire patterns (exact)

Fire gate: line-of-sight (asteroids/large enemies block) + range `min(territory*1.5,
screen*1.0)` + facing within ±π/6 (for non-charging non-continuous). STUN
suppresses. Cooldown `getEnemyFiringCooldown(type, level) = MAX − (MAX−MIN)*min(1,(level−1)/9)`:

| Type | MIN | MAX |
|---|---|---|
| HUNTER | 600 | 2200 |
| GUARDIAN | 3000 | 8000 |
| WASP | 600 | 2000 |
| TITAN | 1200 | 4000 |
| STALKER | 2000 | 6000 |
| TANGERINE | 2500 | 7000 |
| DRIFTER | 2000 | 5500 |
| PROWLER | 1000 | 3500 |
| WEAVER | 400 | 1600 |
| SENTINEL | 1800 | 5000 |

- **HUNTER** `shootBurst3`: 3 shots `shotDelay 75 ms`, speed 4, `#ff4444`, triangle r=16, long range (life 10000 ms).
- **GUARDIAN** `shootGuardianSpread`: 5-fan offsets `[−0.5,−0.25,0,0.25,0.5]`, speed 4.5, alternates square/triangle bullets, sine_wave_nospin (amp 2.5, freq 0.10), r=11, dmg `LS(2)`.
- **WASP** `machinegun`: phases fire (4000±1000 ms) ↔ reload (2000±1000 ms); fires every 520 ms; speed 6, `#ffff44`, sine (amp 2.2, freq 0.12), r=8, dmg `LS(1)`.
- **STALKER** `chargedLaser` → `createLaserBeam`: length 150, width 30, 8 segments, speed 4, `#44ffff`, deathBurst, dmg `LS(3)`; only in firing phase.
- **DRIFTER** `arcLightning`: charge 1200 ms → fractal bolt (5 iter / 32 seg main, 2–4 branches), bolt 460 ms; damage bullets every 8th main point, `#44ffff`, dmg `LS(2)`, life 1000–1500 ms.
- **PROWLER** `shootMissile`: 1 missile speed 12, `#cc44ff`, missile_fast_slow (homing 0.18, maxSpd 14<0.95s then 5), r=12, dmg `LS(3)`.
- **WEAVER** `shootSpiralLaser`: 1 bullet in spinning `faceAngle` every 130 ms, speed 6, `#ffff44`, r=7, dmg `LS(2)`.
- **SENTINEL** `sentinel_sweep` → `shootCircleBurst(8)`: 8 hexagon bullets at `i/8*2π` every 1400 ms (during spin-up/cooldown), speed 4, `#00ff88`, r=12, dmg `LS(2)`.
- **TANGERINE** `layMine`: homing proximity mine at own pos, `#ff8844`, life 60000 ms, r=12, dmg `LS(4)`, HP `floor(5*(1+(level−1)*0.25))`.
- **TITAN** `sweep_laser`: rotating beam, warning 1800 ms (arc `toPlayer ± π/9`) → sweeping 1600 ms (ease-in-out), length `min(screen)*0.65`, half-width 28; damage tick every 180 ms when in beam, `#aa44ff`, dmg `LS(3)`. First sweep 4000 ms, then every 8000 ms.

**Generic bullet factory**: aim jitter `±0.35*(1−(level−1)/4)` (0 at L5+); speed
`base*1.05*(1+min(0.6,(level−1)*0.10))*campaignMul` clamped by SPEED_LIMITS;
`maxRange = 600*(1+(level−1)*0.15)`. Speed/lifetime limits per pattern in
`ENEMY_BULLET_CONFIG` (e.g. AIMED 2–6 / 1.0–1.5 s, LASER 8–15 / 0.6–1.0 s,
HOMING 1–3 / 1.2–2.0 s).

## IV.4 Enemy silhouettes (`render/shapes.js`)

Drawn pre-translated/rotated; `size = radius*K`, `t = now*0.001`. Vertices in
`(±size*k)` units. (Full geometry — port directly.)

- **HUNTER** (K=0.9): body hex `(1.15,0)(0.18,−0.3)(−0.52,−0.2)(−0.72,0)(−0.52,0.2)(0.18,0.3)` `#1a0000`/`#ff4444`; swept wings `rgba(255,40,40,0.15)`/`#ff6666`; engine radial at `(−0.72,0)` r=0.38; cockpit ellipse `(0.32,0)`.
- **GUARDIAN** (K=0.8): dashed shield ring r=1.28 `[6,4]`; 2 swept wings `(0.25,0)(1.5,0.3)(1.25,0.9)(−0.5,1.1)(−0.7,0.35)` `rgba(0,180,60,0.45)`/`#00bb44`; central hex alt r 0.68/0.58 `#001a08`/`#00ff66`; forward cannon rect `(0.55,−0.1, 0.7×0.2)`.
- **WASP** (K=0.8): twin exhaust ellipses `(−0.95,±0.18)`; razor wings `(0.15,0)(0.8,±0.22)(−0.05,±1.05)(−0.65,±0.9)(−0.75,±0.28)` `#cccc00`; abdomen `(−0.45,0)` + 3 stripes; thorax radial; head `(0.38,0)` + 2 eyes + stinger.
- **STALKER** (K=0.92, shimmer `sin(t*11.3)*0.15`): hull `(1.3,0)(0.4,−0.22)(−0.5,−0.18)(−0.75,0)(−0.5,0.18)(0.4,0.22)` `#000d10`; mantis blades `(0.55,−0.2)(1.05,−0.85)(0.05,−1.1)(−0.45,−0.55)(−0.35,−0.18)`; 7-line cloak grid `#00ffff`; plasma edge in `'lighter'`.
- **DRIFTER** (K=0.85, `chargeBoost = laserCharging?1.5:1`): 18-pt jagged ring r `1.5+jitter` `rgba(0,220,255,0.4)`; 6 radial lightning bolts; body 10-pt jagged star r 0.88/0.48 `#000a10`/`rgba(0,255,255,~1)`; core radial.
- **PROWLER** (K=0.8): hull `(1.1,0)(0.6,0.7)(−0.5,0.9)(−1.1,0.4)(−1.1,−0.4)(−0.5,−0.9)(0.6,−0.7)` `#1a0028`/`#cc44ff`; missile pods per side (rect + 3 tubes); spinning nose dish (`t*2.2`); rear twin engines.
- **WEAVER** (K=0.8): outer glow ring (charge), body ring `arc(size)` `color+'40'`, 3 spokes + tip nozzles, core `0.28+charge*0.12`.
- **SENTINEL** (K=0.8, `spin = t*0.8`): outer rotating hex r=1.2, inner counter-rotating hex r=0.88, 6 emitter arms `#00cc55`, solid inner hex r=0.4 `#001a0a`/`#00ff55`, core.
- **TANGERINE** (inner 0.5r, outer 0.8r): inner circle + 8 spikes (forward spike ×1.5), uses caller color.
- **TITAN** (K=0.9): outer armor hex (vert 0 ×1.35 forward) `#1a0020`/`#ff44ff` lw4; corner spikes; inner hex r=0.72 radial; core; rear pods; **independent turret** (rotated `tankTurretAngle − faceAngle`): base ring + barrel rect `(0.28,−w/2, len=1.5, w=0.13)` `#aa00cc`.

**Death flash**: solid white silhouette, scale `1.5→0.3`, alpha `1→0`, additive
halo r `radius*scale*1.5`. Mini-boss: pulsing halo + dashed ring r `radius*1.45`.

## IV.5 Enemy bullets (`enemy-bullet.js`)

Defaults: non-explosive r=9 (glow 18, trail 6, dmg 2); explosive r=14 (glow 26,
trail 12, dmg 3). Distance lifetime `maxRange 600` (level-scaled); persistent
bullets use `maxLifetimeOverride` (default 15000). Off-bounds margin 50. GL shapes:
triangle/square/needle/hexagon; Canvas2D: mine/missile/crescent/explosive.

Movement patterns (key constants): `aimed` straight; `mine` static; `homing_mine`
(homing 0.09, maxSpeed 1.8); `spread` (sine freq 3 amp 0.5); `laser` (2× speed);
`laser_beam` (3× speed); `missile` (homing 0.05); `titan_tomahawk` (accel 0.12,
max 9.0); `missile_fast_slow` (homing 0.18, maxSpd 14 then 5); `pulse` (accel);
`sine_wave[_nospin]` (uses sineFreq/Amp/Phase). Boss-rage adds homing nudge
`vel += dir*0.04`.

## IV.6 Formations (`formations.js`)

A freshly-warped group of ≥3 non-boss/non-miniboss members can bind into a
coordinated plan (`_formation`, `_formationSlot`); per-frame each member lerps
(0.08) toward its slot, overriding individual AI. Ends after `duration` ms or when
survivors <50%. Default `duration 9000`, `radius 220`, `angularSpeed 0.6 rad/s`.

Slot formulas: **orbit** `(px,py)+(cos,sin)(seed+i/n*2π+t*ω)*r`; **weave**
Lissajous-ish offsets; **flank** sided sweep; **cross** pulsing pair angles;
**figure8** `(sin(phase)*r, sin(2*phase)*r*0.5)`. `pickFormation(n, wave)`: needs
≥3; pool `[orbit, weave, flank]` (+cross ≥4, +figure8 ≥5); `radius = 180 +
min(120, wave*6)`; `duration = 6000 + min(6000, wave*250)`.

## IV.7 Boss rage & tiers (`boss-rage.js`)

**Boss tier overlay** (on top of level scaling, `isBoss` TITANs):

| Tier | hpMul | sizeMul | speedMul | points |
|---|---|---|---|---|
| 1 | 4.0 | 1.35 | 1.00 | 500 |
| 2 | 5.0 | 1.45 | 1.05 | 1000 |
| 3 | 6.0 | 1.55 | 1.10 | 1750 |
| 4 | 8.0 | 1.75 | 1.15 | 3000 |

**HP-threshold rage** (all tiers, one-shot): at `health ≤ maxHealth*0.33` →
telegraph (`TELEGRAPH_FRAMES = 24`, red pulsing ring + embers) → `activateRage`:
`RAGE_INVULN_MS = 1500`; `firingCooldown *= 0.66`; `enableHomingBullets`;
16-bullet circular tantrum (speed 4, `#ff3344`); screen flash (0.42,12) + shake
(40,22). Aura = faint red ring `radius*1.35`.

Per-tier: **Tier 2** links bosses into a `_bossPair` — a partner death immediately
rages the survivor. **Tier 3+** share a `_formationCenter` (orbit `radius =
min(380, fieldW*0.22)`, ω ±0.012, critically-damped spring k=0.08 damp=0.20).
**Tier 4** `_phaseTimer` 720 frames (12 s) toggles formation-orbit ↔ free-raged AI.

---

# Part V — Waves

## V.1 Constants & structure

`MAX_WAVES = 30`, `WAVES_PER_STAGE = 3`, `MAX_STAGES = 10`,
`BOSS_WAVES = [3,6,9,12,15,18,21,24,27,30]`. Label `"stage-subIndex"` via
`getStageLabel` (e.g. wave 12 = "4-3"). `isStageClear(w) = (w % 3 == 0)`.
`MAX_ASTEROIDS = 16`, `AST_SPEED = 1.75`.

Each wave config = `{asteroids: N, subWaves: [pulse, …]}`. Pulse 0 spawns
immediately; later pulses spawn when **≤2 enemies remain OR 12000 ms** elapsed.
Boss waves hold the TITAN(s) in the final pulse.

## V.2 Complete wave table (desktop counts)

Pulse = an in-wave `subWaves` entry. Each cell lists groups `TYPE ×n`.

**Stage 1 (HUNTER+WASP):**
- W1 (1-1, ast 5): P0 HUNTER×3 · P1 HUNTER×2,WASP×2 · P2 HUNTER×3,WASP×2
- W2 (1-2, ast 5): P0 HUNTER×3,WASP×2 · P1 WASP×4 · P2 HUNTER×3,WASP×3
- **W3 (1-3, ast 3, BOSS T1):** P0 HUNTER×3,WASP×2 · P1 **TITAN×1(T1)**,HUNTER×2,WASP×2

**Stage 2 (+GUARDIAN):**
- W4 (2-1, ast 5): P0 GUARDIAN×2 · P1 GUARDIAN×2,HUNTER×3 · P2 GUARDIAN×2,WASP×3
- W5 (2-2, ast 5): P0 GUARDIAN×3,HUNTER×2 · P1 WASP×4,GUARDIAN×1 · P2 GUARDIAN×2,HUNTER×3,WASP×2
- **W6 (2-3, ast 3, BOSS T1):** P0 GUARDIAN×3,WASP×2 · P1 **TITAN×1(T1)**,GUARDIAN×3,HUNTER×2

**Stage 3 (+STALKER):**
- W7 (3-1, ast 5): P0 STALKER×2 · P1 STALKER×2,HUNTER×3 · P2 STALKER×2,GUARDIAN×2,WASP×2
- W8 (3-2, ast 5): P0 STALKER×3,HUNTER×2 · P1 GUARDIAN×3,STALKER×1 · P2 STALKER×2,GUARDIAN×2,HUNTER×3
- **W9 (3-3, ast 3, BOSS T2):** P0 STALKER×2,GUARDIAN×2 · P1 **TITAN×1(T2)**,STALKER×2,HUNTER×2

**Stage 4 (+DRIFTER+TANGERINE):**
- W10 (4-1, ast 4): P0 DRIFTER×2,HUNTER×2 · P1 TANGERINE×2,WASP×2 · P2 DRIFTER×2,TANGERINE×2,HUNTER×2
- W11 (4-2, ast 4): P0 STALKER×2,DRIFTER×2 · P1 TANGERINE×2,GUARDIAN×2 · P2 STALKER×2,DRIFTER×2,TANGERINE×1
- **W12 (4-3, ast 3, BOSS T2):** P0 GUARDIAN×3,STALKER×2,WASP×2 · P1 **TITAN×2(T2)**,STALKER×2,TANGERINE×1

**Stage 5 (+WEAVER+SENTINEL):**
- W13 (5-1, ast 4): P0 WEAVER×2,WASP×3 · P1 WEAVER×2,HUNTER×3 · P2 WEAVER×2,GUARDIAN×2,STALKER×1
- W14 (5-2, ast 4): P0 SENTINEL×2,WASP×2 · P1 SENTINEL×2,GUARDIAN×2,WEAVER×1 · P2 SENTINEL×2,STALKER×2,WEAVER×2
- **W15 (5-3, ast 2, BOSS T3):** P0 GUARDIAN×3,SENTINEL×2,WEAVER×1 · P1 **TITAN×3(T3)**,SENTINEL×2,STALKER×1

**Stage 6 (+PROWLER — full roster):**
- W16 (6-1, ast 4): P0 PROWLER×2,HUNTER×3 · P1 PROWLER×2,STALKER×2,WASP×2 · P2 PROWLER×2,GUARDIAN×2,WEAVER×2
- W17 (6-2, ast 4): P0 TANGERINE×2,DRIFTER×2,HUNTER×2 · P1 SENTINEL×2,WEAVER×2,STALKER×2 · P2 PROWLER×2,GUARDIAN×2,WASP×3
- **W18 (6-3, ast 2, BOSS T3):** P0 PROWLER×3,SENTINEL×2,WASP×2 · P1 **TITAN×3(T3)**,PROWLER×2,GUARDIAN×2

**Stage 7:**
- W19 (7-1, ast 4): P0 HUNTER×4,GUARDIAN×2,WASP×2 · P1 STALKER×2,WEAVER×2,DRIFTER×2 · P2 PROWLER×2,SENTINEL×2,TANGERINE×2
- W20 (7-2, ast 4): P0 STALKER×3,PROWLER×2,WASP×2 · P1 SENTINEL×3,GUARDIAN×2,HUNTER×2 · P2 WEAVER×2,TANGERINE×2,DRIFTER×2
- **W21 (7-3, ast 2, BOSS T4):** P0 STALKER×3,GUARDIAN×3,WEAVER×1 · P1 **TITAN×4(T4)**,STALKER×2,SENTINEL×2

**Stage 8:**
- W22 (8-1, ast 4): P0 TANGERINE×2,GUARDIAN×2,HUNTER×2 · P1 WEAVER×2,DRIFTER×2,STALKER×2 · P2 PROWLER×2,SENTINEL×2,WASP×3
- W23 (8-2, ast 4): P0 HUNTER×5,STALKER×2 · P1 SENTINEL×3,PROWLER×2,WEAVER×2 · P2 GUARDIAN×3,TANGERINE×2,DRIFTER×1
- **W24 (8-3, ast 2, BOSS T4):** P0 TANGERINE×3,GUARDIAN×3,STALKER×2 · P1 **TITAN×4(T4)**,TANGERINE×2,PROWLER×2

**Stage 9:**
- W25 (9-1, ast 4): P0 STALKER×3,GUARDIAN×3,WASP×2 · P1 SENTINEL×3,PROWLER×2,WEAVER×2 · P2 TANGERINE×3,DRIFTER×2,HUNTER×3
- W26 (9-2, ast 4): P0 PROWLER×3,SENTINEL×2,TANGERINE×2 · P1 WEAVER×3,STALKER×2,GUARDIAN×2 · P2 HUNTER×4,WASP×3,DRIFTER×2
- **W27 (9-3, ast 2, BOSS T4):** P0 WEAVER×3,GUARDIAN×2,SENTINEL×2 · P1 **TITAN×5(T4)**,WEAVER×2,STALKER×2

**Stage 10:**
- W28 (10-1, ast 4): P0 STALKER×3,GUARDIAN×3,WASP×3 · P1 TANGERINE×3,PROWLER×2,HUNTER×3 · P2 SENTINEL×3,WEAVER×3,DRIFTER×2
- W29 (10-2, ast 4): P0 HUNTER×4,GUARDIAN×3,WASP×3 · P1 STALKER×3,WEAVER×3,PROWLER×2 · P2 **TITAN×1 (NORMAL, not boss)**,SENTINEL×2,TANGERINE×2,DRIFTER×2
- **W30 (10-3, ast 2, FINAL BOSS T4):** P0 GUARDIAN×3,SENTINEL×2,STALKER×2 · P1 PROWLER×2,WEAVER×2,TANGERINE×2 · P2 **TITAN×5(T4)**,GUARDIAN×2,SENTINEL×2,STALKER×2,PROWLER×1

> **PARITY:** W29 P2 TITAN has no `isBoss`/`bossTier` — a normal leveled TITAN.
> Boss detection (`isBossWave`) is purely the `[3,6,…,30]` list, independent of
> per-group flags.

## V.3 Wave manager logic

Port the **live** path only (the `completeWave`/`startNewWave`/`spawnContinuous*`
functions are dead). Per tick (`updateWaveSystem`, PLAYING/WAVE_TRANSITION only):
1. Clean pools. 2. Stuck recovery. 3. `tryAdvanceSubWave`. 4. Wave-complete gate.

**Wave advance is kill-gated only:** `enemyCount==0 && !waveComplete &&
allPulsesSpawned`. Asteroids never block. On clear: `waveComplete=true`,
state→WAVE_TRANSITION, award bonuses; if `wave≥30` → `completeRun()` → GAME_COMPLETE.

**Pulse pacing**: advance when ≤2 enemies OR 12000 ms since last pulse. Pulse 0
immediate. Each pulse spawns every group via `spawnLeveledEnemies(type, count, {onScreen, bossTier?})`.

**Wave start** (`startNextWave` → at +700 ms `spawnWaveEntities`, player
`makeInvincible(3000)`; at +2800 ms state→PLAYING): set enemy level = wave,
asteroid level = `ceil(wave/2)`, assign mission, spawn ALL asteroids once, spawn
pulse 0. **No intra-pulse stagger** — every enemy in a pulse warps in same tick.

After wave 30 → GAME_COMPLETE. No loop, no endless mode.

## V.4 Difficulty curves (keyed off wave `w`, `t = (w−1)/29`)

- Enemy level = `w`; asteroid level = `ceil(w/2)`.
- Enemy speed mult = `0.55 + t^1.5*1.2` (W1 0.55× → W30 1.75×).
- Enemy bullet speed mult = `1.15 + t^1.4*1.9` (W1 1.15× → W30 3.05×).
- Enemy HP mult (`tL=(L−1)/29`) = `1 + tL*8.0 + tL^2.5*6.5`; points mult `1 + tL^1.4*5.5`.
- Asteroid HP mult (`tA=(L−1)/9`) = `1 + tA*6.5`; asteroid spawn speed `min(5.0, 1.75 + (w−1)*0.15)`.
- Firing cooldown normalizes over 10 levels (saturates at wave 10).

## V.5 Spawn positioning

All wave spawns use `{onScreen:true}`: pick a target inside the viewport, then a
source just outside the nearest edge, warp source→target.

**Target** (`getOnScreenSpawnPosition`): viewport rect inset by `edgePad`; up to 24
attempts of `random(safeRect)`, reject if in minimap area, within
`minDistFromPlayer`, or overlapping an entity. Enemies: `minDist 260, edgePad 90,
selfR 18`. Asteroids: `minDist r+220, edgePad r+12, selfR r`.

**Source/edge**: `sourceMargin = 220 + rand*160` outside the closest edge;
e.g. top → `(targetX + rand(−120,120), camY − margin)`. Then `startWarpIn(target)`.

Minimap exclusion (top-left): size `minDim<500 ? max(80, floor(minDim*0.22)) : 150`,
margin `size<120 ? 10 : 20`.

## V.6 Wave events: missions, rewards, survivor cards

**Announcements**: dark intro overlay `STAGE {label}` + `WAVE_SUBTITLES[wave]`
(2800 ms); mission toast (+1 SP, 3500 ms); pulse phase toast for P>0 (1600 ms);
stage-clear banner (mid-stage clears suppressed). Per-wave subtitles are listed in
the source `WAVE_SUBTITLES` (e.g. W3 "BOSS — Iron Scout. Aim for the bolts.").

**Rewards on clear** (every wave): `bonusCoins = stageClear ? (50+wave*25)*2 :
round((50+wave*25)*0.6)`; survivor `picks = stageClear ? 1 : 0`.

**Survivor-card / shop flow** (the real inter-wave gate; no fixed timer): 2700 ms
after clear, on **stage clears** (waves 3,6,…) → pause + draw 3 random non-maxed
PASSIVE cards (pick grants +1 stack free; boss waves auto-grant one extra), then
chain into shop-suggest (3 cheapest affordable upgrades for the equipped weapons),
then `startNextWave`. **Mid-stage waves auto-advance** with no pick.

**Missions** (one/wave, +1 SP desktop): boss waves always `no_damage`; non-boss
roll one of: `no_damage`, `fast_kill` (5 kills/8 s), `asteroid` (clear all rocks),
`streak` (12-streak), `precision` (25 crits).

**Mid-wave mini-boss promotion** (non-boss non-TITAN groups, wave ≥4): chance
`min(0.45, 0.06 + (wave−4)*0.025)`, one per group → HP×1.7, radius×1.25, points×2.

---

# Part VI — World objects & drops

`TICK_SCALE = 0.5` applies to asteroid/powerup position+timers and ALL particle
decay (`×TS`), but NOT to gold/color-star integration (those step `x += vel` with
friction baked in).

## VI.1 Asteroids (`asteroid.js`)

Spawn radius `random(30, 60)`. Geometry = **3D tumbling wireframe** of a 12-vertex
icosahedron point set (`t = φ`), 30 edges, each vertex jittered `d = 1 +
random(−0.25, 0.25)` (±25%). Perspective `fov = 300`, `scale = fov/(fov+z)`.
Effective `radius = (minVtxDist + maxVtxDist)/2`; `mass = (4/3)π·r³`.

Vertices `[±1,±t,0]`/`[0,±1,±t]`/`[±t,0,±1]` permutations; edges:
```
[0,1][0,5][0,7][0,10][0,11][1,5][1,7][1,8][1,9][2,3][2,4][2,6][2,10][2,11]
[3,4][3,6][3,8][3,9][4,5][4,9][4,11][5,9][5,11][6,7][6,8][6,10][7,8][7,10][8,9][10,11]
```

**Color** (per-asteroid): `baseHue` = 20% gold `40+rand*20` else `150+rand*130`;
`hueSpread 30–100`, `hueCycleSpeed 10–30`, `saturation 80–95`, `lightness 65–80`.
Wireframe: black underlayer (alpha 0.85, lw 4.5) then 5 depth-bucketed colored
strokes (lw 2), per-edge `hue = (baseHue + now/cycleSpeed + (i/edges)*hueSpread)%360`,
`alpha = max(0.2, ((fov−avg)/(fov+radius))²)`.

**Motion**: `vel = random(±1.75) || 0.2`; `ASTEROID_MAX_SPEED = 2.0` cap; `x +=
vel*TICK_SCALE`; edge bounce `*0.9`; 3D rotation `rotVel = random(±0.04)`/axis.

**HP** (`sizeRef = baseRadius`): large (≥40) `floor(3 + (sizeRef−40)/20*2)` → 3–5;
medium (20–40) `floor(1 + (sizeRef−20)/20*2)` → 1–3; small (<20) → 1. Level scale
`*(1 + (level−1)*0.35)`. Collision dmg to player = `LS(2)`.

**Split** (two paths, both gate on `baseRadius > 20`): bullet inline → `count =
(rand<0.5?2:3)+1` = **3–4** fragments; `destroyAsteroid` (AoE) → **2**. `newR =
baseRadius/√count`; `fragHP = max(5, round(parentMaxHP*random(0.7,0.9)))`; evenly
spread, `speed random(4.5,7.5)`, `vel = parentVel*0.3 + dir*speed`. Small rocks
don't split. Death flash 6 frames (white scale 1.4→0.3). No numeric score
(routes through `onEnemyKill` + kill streak; +12 XP comment).

## VI.2 Asteroid shards (`asteroid-shard.js`)

3D-spinning wireframe triangles. Base `(−1,−0.577,0)(1,−0.577,0)(0,1.155,0)`,
`FOCAL=100`; spin `random(±0.10/0.14/0.18)`/axis; `life=1`, `lifeStep=1/120` (~2 s);
drag `*0.985`; `lineWidth 1.6`. Emission (asteroid death): `count = floor(10 +
12*sizeScale)`, `sizeScale = min(1.5, baseRadius/25)`, `speed random(3.5,9.0)*scale`,
every 5th white else cycle bright/base/dim.

## VI.3 Powerups (`powerup.js`)

**Powerups do NOT drop from kills** (5.70.0) — acquired via shop/survivor cards.
World entity `radius 18` (glow 45), `life = 25*60` ticks, `pulse = 0.85 +
sin*0.15`. Body shape: EXPLOSIVE 8-point star, CRIT_DAMAGE 12-point star, others
hexagon. Magnet (k=0.55): base homing always; `<100` `+15*prox`; `<40` `+25*prox`;
friction `ORB_FRIC 0.92`.

Full `POWERUP_TYPES` (many `hidden` = retired to per-weapon upgrades, won't roll):

| id | display | color | effect | maxStacks | description |
|---|---|---|---|---|---|
| RAPID_FIRE* | Rapid Fire | `#ff3300` | rapidFire | 5 | +22% fire rate |
| MULTI_SHOT* | Multi Shot | `#3366ff` | multiShot | 4 | +1 bullet |
| BIG_BULLETS* | Big Bullets | `#33cc33` | bigBullets | 3 | +2.2px radius |
| EXPLOSIVE* | Explosive | `#ff6600` | explosive | 3 | AoE blast (+10px) |
| CRIT_CHANCE | Critical Chance | `#ffcc00` | critChance | 6 | +7% crit |
| CRIT_DAMAGE | Critical Damage | `#ff0066` | critDamage | 6 | +15% crit dmg |
| KNOCKBACK | Knockback | `#ffaa44` | knockback | 3 | +40% power-weapon knockback |
| HEALTH_BOOST | Health Boost | `#ff5555` | healthBoost | 10 | +35 max HP, full heal |
| SHIELD_BOOST | Toughness | `#33ff99` | shieldBoost | 8 | +8% DR (cap 75) |
| REGEN | Health Regen | `#66ffaa` | regen | 5 | +0.5 HP/s out of combat |
| PHASE_ECHO | Phase Echo | `#88ddff` | phaseEcho | 2 | +2s post-dash invuln |
| VAMPIRISM | Vampirism | `#cc0033` | vampirism | 5 | heal 5% dealt |
| THORNS | Thorns | `#ff7733` | thorns | 4 | reflect 25% taken |
| EXECUTIONER | Executioner | `#cc0044` | executioner | 5 | +20% dmg vs <25% HP |
| MOMENTUM | Momentum | `#ffaa33` | momentum | 4 | +5%/s sustained (cap +15%) |
| OVERCHARGE_ROUNDS | Overcharge | `#ffcc00` | overchargeRounds | 4 | every Nth bullet ×3 |
| GUARDIAN | Guardian | `#ffeb44` | guardian | 3 | survive lethal at 1 HP, 1/wave |
| STATIC_DISCHARGE | Static Discharge | `#88aaff` | staticDischarge | 5 | periodic AoE pulse |
| WHIRLWIND | Whirlwind | `#88ffcc` | whirlwind | 4 | orbiting damage zone |
| HEALTH_DROP_FREQUENCY | Triage | `#66ffaa` | triage | 6 | −2.5s health-drop cd |
| LUCKY_DROPS | Lucky Drops | `#88ccff` | luckyDrops | 3 | +12% health-drop chance |
| FIELD_RATIONS | Field Rations | `#ffcc66` | fieldRations | 3 | +30% heal/orb |
| TRIAGE_SURGE | Triage Surge | `#ff66aa` | triageSurge | 3 | steeper desperation curve |
| COMBAT_MEDIC | Combat Medic | `#ff7777` | combatMedic | 1 | first kill after hit heals (8s cd) |
| SALVAGE_PLATING | Salvage Plating | `#ffaa44` | salvagePlating | 1 | tank pop → health orb |
| TRIAGE_NET | Triage Net | `#66ffcc` | triageNet | 1 | 2× health-orb magnet |
| ADRENAL_RESERVE | Adrenal Reserve | `#ff4477` | adrenalReserve | 1 | ≤25% HP → orb refills tank (15s cd) |
| FIELD_SURGEON | Field Surgeon | `#aaffaa` | fieldSurgeon | 1 | heal over 1.5s +50% |
| BLOOD_BANK | Blood Bank | `#cc3344` | bloodBank | 1 | overflow → tanks 2× faster |

`*` = `hidden: true` (retired; mechanic now lives in per-weapon upgrades).

## VI.4 Gold, coins, drop tiers

- **GoldCoin** (pixel, many/drop): `value`, `#ffd700`, shape square45/circle40/dot15,
  `radius 1.5–3.0`, `life 7200` ticks, scatter `speed 0.8–2.2`, friction 0.92.
- **GoldShape** (chunky, 1–2/drop): 2D shapes (star4/5/6/8, hexagon, diamond,
  triangle); jewel roll 15% (`value×3`, jewel colors); `radius = (8 + ratio*8)*tier.sizeScale`,
  `life 7200`, scatter `2.4–4.5`, rotation `±0.012–0.024`.

**Drop tiers** (by final value): bronze (≥1, `#cd7f32`, scale 0.80), silver (≥35,
`#dcdcdc`, 0.95), gold (≥100, `#ffd700`, 1.00), platinum (≥200, `#88ddff`, 1.20).

**Magnet** (both gold types, `MAGNET_Z=2.5`): mid `<180` `+6*((180−d)/180)*2.5`;
snap `<60` `+14*((60−d)/60)*2.5`; tractor `<240`. Magnet hierarchy: gold 180 >
health 110 > inventory 90.

## VI.5 Drop tables (`combat-manager.dropOrbsFromEntity`)

**Health orb** (cooldown-gated): `rate = min(1.0, (0.70 + (wave−1)*0.015 +
(level−1)*0.05 + enemy 0.15 + LUCKY*0.12) * desperationMult)`,
`desperationMult = 1 + (1.5 + TRIAGE_SURGE*1.0)*(1−hpPct)²`. Cooldown `max(12000,
25000 − Triage*2500)`, halved at ≤25% HP. One orb. `heal = floor(rand*(maxHeal−minHeal+1))+minHeal`,
`minHeal = 4 + floor((wave−1)*0.6)`, `maxHeal = 8 + …`. FIELD_RATIONS `×min(2.0, 1+0.30*stacks)`.

**Money orb** (no cooldown): `rate = min(0.95, (0.65 + (wave−1)*0.015 +
(level−1)*0.05 + enemy 0.15)*goldFind*streakGold*profileRate)`. Budget `= max(1,
round(legacyCount*avgMoney*goldFind*streakGold*profileBudget))`, `avgMoney =
(minMoney + maxMoney)/2`, `minMoney = 10 + (wave−1)*3`, `maxMoney = 20 + (wave−1)*5`.
`_splitMoneyDrop`: `dropMax = boss?600:250`, `shapeCap = 80`, ≤2 shapes, pixels
`min(30, max(6, 8 + shapeN*2 + profile.pixelBonus))`.

**Enemy drop profiles** (`ENEMY_DROP_PROFILES`): grunt (0.75/0.85/+2/minShape0),
standard (1.00/1.00/0/1), tanky (1.40/1.00/0/1), miniboss (1.80/1.00/+4/1), boss
(2.40/1.00/+6/1). Type→profile: HUNTER/WASP/STALKER grunt; DRIFTER/WEAVER/TANGERINE
standard; GUARDIAN/PROWLER/SENTINEL tanky; TITAN miniboss; `isBoss` → boss.

**Item drops** (`item-system.js`, left-edge loot feed, enemy kills only, never a
world pickup): 3 rolls — HP slot (cockpit/hull) `0.025` (boss 0.085), Toughness
(shielding/chassis) `0.020` (0.075), Trinket (nanites) `0.015` (0.060). Item level
= wave. Rarity common 0.65 / rare 0.27 / epic 0.08; affix counts 1/2/3. Affix pool
`value = (base + (wave−1)*perWave)*rarityMult`: hp 8/2, toughness 3/0.3%,
vampirism 2/0.1%, thorns 6/0.4%, critChance 3/0.2%, critDamage 8/0.5%, dodge 2/0.1%,
speed 6/0.3%, regen 0.3/0.05.

## VI.6 Particles (`particle.js`)

Pool object; integrate with `TS = 0.5`; `life ≤ 0` dies. Bright types
(`explosion, starSparkle, explosionFlash, explosionEmber, explosionShrapnel,
explosionRingColored, enemyPlasmaCore`) render on the WebGL additive layer; rest on
Canvas2D. Selected types (init r / velocity / life / decay×TS / color):

| type | r | velocity | life | decay | color |
|---|---|---|---|---|---|
| explosion | 1–3 | ±5,±5 | 1 | 0.04 | `hsl(rand,100,70)` |
| playerExplosion | 0→150 | — | 1 | 0.02 | `#0ff` ring |
| thrust | 1–2.5 | angle±0.26, 2.5–4.5 | 1 | 0.04 | `#ff4500/#ff8c00/#ffa500` |
| explosionFlash | `(r‖40)*0.3`→max | — | 1.2 | 0.06 | `#ffffff` |
| explosionShrapnel | 1.5–4 | angle±0.3, speed`(r‖5)*0.7–1.3` | 0.6–1.0 | 0.03 | `#ff8800`, streak 6–16 |
| explosionEmber | 1.2–3.5 | rand, 0.3–1.8 | 0.6–1.0 | 0.020 | `#ffaa44` |
| explosionRingColored | `(r‖50)*0.15`→max | — | 0.9 | 0.035 | `#ff8800`, lw 3–8 |
| enemyPlasmaCore | `(r‖60)*0.55`→max | — | 1.8 | 0.033 | `#ffcc44` |
| burnFlame | 1.6–3.2 | x±0.35, y −1.4..−0.7 | 0.5 | 0.033 | `#ff7722`/`#ffcc44` |
| stunArc | len 10–20 | static | 0.3 | 0.055 | `#88ddff` |

**Asteroid death `createDebris`** (`sizeScale = min(1.5, baseRadius/25)`): 1 flash
(r `baseRadius*1.5*scale`); 3 staggered rings (t0/+70/+150 ms); `floor(14+10*scale)`
shrapnel; 4 center embers + `floor(10+7*scale)` embers; 20 explosion particles;
line debris per edge + shards. Enemy death: elaborate plasma-core/shockwave/
lightning sequence staged at frames 0/6/24.

## VI.7 Starfield (`background-star.js`, `color-star.js`)

- **BackgroundStar**: `radius = (z*1.0 + 0.5)*densityFactor`, circle, twinkle
  `speed 0.004–0.018`, opacity floor ~85%. Color rolls: 40% blue-white, 20% white,
  12% cyan-white, 13% gold, 15% saturated nebula tint. Parallax `z^1.8 * 0.12`.
- **ColorStar (decorative)**: 15% big star; shapes from `STAR_SHAPES` (`point`×5,
  circle, diamond×2, triangle, hexagon, star4/5/6/8, cube, octahedron, tetrahedron,
  prism) or `BIG_STAR_SHAPES`=[point,circle]; `z≥2.0` → forced point. Colors from
  `NORMAL_STAR_COLORS` (18-entry nebula gamut, listed in `constants.js:296`).
  `life = -1` (immortal). Parallax same as background.
- **ColorStar (collectible orbs)**: 3D solids (tetra/cube/octa/dodeca), health
  `#00aaff`, money `#ffd700`; z `1.5–3.0`; health permanent, money fades over last
  120 ticks; health-orb magnet far `<110` near `<45` (TRIAGE_NET ×2).

WebGL counts: `BACKGROUND_STAR_COUNT 30 ×6`, `COLOR_STAR_COUNT 25 ×3`, buffer cap
`WEBGL_STAR_BUFFER_SIZE 4000`.

## VI.8 Line debris (`line-debris.js`)

One rotating line segment per asteroid wireframe edge on death (≤30/rock). `life=1`,
`life -= 0.02` (~50 ticks); velocity from edge-midpoint angle, `speed 2–5`,
`rotVel ±0.1`; `lineWidth 2`; color = asteroid base color or rainbow `hsl`.

---

# Part VII — Rendering pipeline

## VII.1 Layer architecture & glow

Three full-screen canvases + DOM HUD (web). Bottom→top:

| z | element | context | contents | blend |
|---|---|---|---|---|
| 0 | `#glCanvas` | WebGL2 | starfield (first) + nebula + particles | additive |
| 1 | `#gameCanvas` | Canvas2D | ship, enemies, asteroids, powerups, **bullet trails**, weapon effects | normal alpha |
| 2 | `#bulletCanvas` | WebGL2 | **bullet bodies** (instanced SDF) | **normal src-over** |
| 10+ | DOM | HTML/CSS | HUD/overlays | — |

**No real HDR/blur in the hot path.** Glow is faked three ways: (1) **additive
blend** on the particle/starfield layer (`SRC_ALPHA, ONE`) with shader
`BRIGHTNESS_GAIN = 1.3`; (2) Canvas2D `'lighter'` for ship body / plasma / muzzle
flashes; (3) **fake-glow rings** — wide faint stroke (lw 8, α 0.18) + sharp stroke
(lw 2, α 0.40) + inner white (lw 4, α 0.25) at `r+8`/`r+5` (no `shadowBlur`);
plus `glowSpriteCache` (bake a blurred circle once, then `drawImage` per frame).

**Bevy mapping (the payoff):** render stars+particles+nebula and the bullet layer
as **HDR emissive sprites/meshes into an HDR `Camera2d` + `Bloom`**. Every
`#ffffff`-cored radial gradient, glow-sprite halo, fake-glow ring, `'lighter'`
composite, and the 1.3× gain → genuine emissive intensity >1 feeding bloom.
**PARITY caveats:** (a) bullet bodies use *normal alpha*, not additive — only
particles/starfield are additive; (b) the lens-flare nebula (`nebula-renderer.js`)
is **disabled** (`draw()` early-returns) — the live nebula is the WebGL atlas slots
below.

## VII.2 Bullet rendering (`webgl-bullet-renderer.js`)

One instanced draw, own context. Per-instance 10 floats `[x, y, w, h, r,g,b,a,
angle, shapeId]`; unit quad TRIANGLE_STRIP; `maxInstances 1024`. Quad scaled
`size*1.25` (SDF body radius 0.40 in unit space). Shape SDFs (port to a Bevy WGSL
`Material2d` or pre-baked meshes):

```glsl
circle:   length(p) - 0.40
triangle: p.y=-p.y; p.x=abs(p.x); max(p.x*0.866 + p.y*0.5 - 0.40, -p.y - 0.34)
hexagon:  p=abs(p); max(p.x-0.40, p.x*0.5 + p.y*0.866 - 0.40)
diamond:  abs(p.x)+abs(p.y) - 0.42
star(5):  ang=atan(p.y,p.x)+π/2; r=length(p); k=ang*5/2π; s=fract(k);
          w=s<0.5?s*2:(1-s)*2;  r - mix(0.18,0.42,w)
square:   d=abs(p)-0.32; max(d.x,d.y)
needle:   p=abs(p); length(vec2(p.x, max(0,p.y-0.32))) - 0.06
charge:   length(p) - 0.45
```
shapeId map: circle 0, triangle 1, hexagon 2, diamond 3, star 4, square 5,
needle 6, charge 7. Fragment = flat body + 1px AA; trails are separate Canvas2D.

## VII.3 Particle rendering (`webgl-particle-renderer.js` + atlas)

One instanced draw, shared `#glCanvas`, **additive** `(SRC_ALPHA, ONE)`,
per-instance 13 floats. `MAX_PARTICLES 2500` initial (grows by doubling, peak
~3000). Fragment `rgb = clamp(tex.rgb*color.rgb*1.3, 0,1)`. Atlas 1280×256, five
256×256 slots (procedurally painted): **dot** (ember/explosion), **flash** (flash/
plasma-core, +4-point cross), **ring** (annulus), **streak** (shrapnel), **spark**
(8-point star). Per-type size/alpha-over-life curves are transcribed in the source
`_packParticle` (e.g. ember `draw=(r+4)*1.8`, `alpha=pow(life,0.45)`; flash
`eased=flashLife²*√flashLife`).

## VII.4 Starfield rendering (`webgl-starfield-renderer.js` + atlas)

One instanced draw, shared context (TEXTURE1, before particles), additive.
`maxStars 4000`; populated once, GPU does parallax/twinkle/rotation. Per-instance
16 floats. Vertex: parallax `mod(basePos − drift*parallax, field)`; twinkle
`0.5+0.5*sin(time*speed+phase)`; **size pulse** `0.94 + 0.18*wave`; **5 Hz blink**
`pow(0.5+0.5*sin(time*5+phase*7), 8)` dip 0.45. Fragment: atlas × palette,
**radial glow halo** where atlas transparent, **CRT scanlines** (period 2 CSS px,
contrast 0.78→1.0, no scroll; bypassed for orbs `noScan`). Atlas 1920×128, 15 slots
(dot, diamond, triangle, hexagon, star4/5/6/8, cloud, cube, octahedron, tetrahedron,
prism, nebula_wispy, nebula_core).

## VII.5 Nebula

**Live nebula = WebGL atlas slots 8/13/14**, drawn as huge low-alpha additive star
instances. Generated via multi-octave value noise (xorshift32 → bilinear smoothstep
→ 3-octave fbm): slot 13 **wispy** (anisotropic `2.5/0.8`, `pow(noise,1.7)`, oval
mask), slot 14 **core** (`exp(−r²*4) + halo*0.55*noise`), slot 8 **cloud** (wide
gaussian × 8×8 noise). JWST multi-hue look = layering several cloud quads with
distinct palette tints; motion = parallax only (static alpha). The current
`dps` already has a procedural JWST fbm shader (`render/nebula.wgsl`) — keep it,
match these recipes.

The standalone lens-flare nebula (`nebula-renderer.js`, 4 parallax layers, cross
diffraction spikes, 12 palettes) is **disabled** — port only if desired.

## VII.6 Depth-batch & color cache

`depth-batch-renderer.js` is the Canvas2D fallback batcher: 11 opacity buckets,
group by color/shape, single path per bucket — irrelevant once on GPU instancing.
`color-cache.js`: precomputed `rgba`/`hsl` string tables (the asteroid hue-cycle
uses the `hsl` cache). For Bevy these become plain `Color` values.

## VII.7 shapes.js inventory (master art reference)

`render/shapes.js` exports the shared silhouette library. Functions: `getShipPaths`
(shared winged hull + `SHIP_PALETTE_MAGENTA` for remote ships), `drawShipShape`,
`drawAsteroidShape` (+ `generateAsteroidVertices`, `projectAsteroidVertices`),
`drawEnemyShapeByType` (router → the 10 per-type drawers in Part IV.4), plus pickup
glyphs and effect helpers. The **hero player ship** is the richer variant in
`player/renderer.js::_getShipPaths` (Part II.4) — cyan/blue with wing tips the
shared one lacks. All vertex data is `radius`-relative; port directly to lyon paths.

---

# Part VIII — HUD, UI, shop, audio, input

## VIII.1 HUD layout

Canvas-rendered, `'Press Start 2P'` font. Top-left cluster: `[triforce][healthbar]
[LV-shield][level][energy-sphere]`.

- **Health bar**: `barX 70, barY 20, barW 220, barH 30`, beveled; gradient by tier
  (>0.6 blue, >0.3 yellow, ≤0.3 red+glow); smoothed `_displayedHealth` (drain
  16%/frame, gain 30%/frame); HP text `N/max`.
- **Triforce (spare tanks)**: 3 gold triangles `#FFD700`/`#B8860B`, each = 1 spare;
  active tank = the bar; gold vaporize burst on loss, green sparkle on gain.
- **Level/shield badge**, **power-weapon energy sphere** (fills tinted by primary
  color, gold ready-pulse when `energy ≥ cost`).
- **Gold readout** (bottom-right, slot-roll counter, white flash on gain, "+N"
  popups), **survival timer** (`H:M:SS:mmm`, `#FFA500`), **XP bar** (full-width
  bottom, 6px).
- **Loadout squares** (PRM/PWR, bottom-left, 50px, weapon icon tinted by color).
- **Powerup indicators** (right column, 40px, stack badges).
- **Wave overlays** (`STAGE 1-1` wavy title + subtitle; WAVE COMPLETE banner).
- **Kill-streak** (bottom-center, tier-colored `N KILLS` + progress + idle bar +
  `+N% GOLD`; resets on damage).
- **Damage numbers** (gold hit / red −N player / green +N heal / orange CRIT 22px).
- **Target info** (top-center `LV.N NAME` + health bar).
- **Minimap** (top-left 150px, cyan view-rect + player dot, red enemy dots),
  **off-screen indicators** (red edge glow), **item loot feed** (left edge cards).
- **Bottom-center buttons**: UPGRADES, STATS, PAUSE.

**Crosshair / aim** (`cursor.js`): cyan `#00ffff` crosshair; red targeting cursor
over enemies; jitter circle scaled by shooting intensity. **Aim laser/cone**
(desktop, `assists.laserSight`): single-line (4 stacked additive red strokes + range
tick + per-pierce reticles) or spread cone (red wedge + edge lasers + reticles per
entity).

## VIII.2 Input mapping (`input-handler.js`, `event-setup.js`)

| Control | Action |
|---|---|
| W/A/S/D | move up/left/down/right |
| Arrow ←/→ | rotate aim |
| Arrow ↑ / ↓ | mirror primary / power fire |
| Mouse move | set aim (world); ship faces cursor |
| Left click | primary fire |
| Right click / Space | power weapon (hold-charge / release) |
| Tab | activate equipped defense skill |
| Shift | dash (one-shot `dashPulse`) |
| Q | activate skill |
| F (hold) | PRIMARY weapon radial menu |
| E (hold) | POWER weapon radial menu |
| Esc | close stats → close inventory → pause |
| I | inventory; ` (backquote) | stats |
| Mouse wheel | shop scroll |
| `[` / `]` | cheats +1000 gold / +5 SP |

**No gamepad** in the JS source — but Bevy's built-in `gilrs` is a planned upside
(map sticks → move/aim, triggers → fire/power). Current `dps` input is WASD/arrows
+ Space + Tab/Q + 1–5; extend to the full scheme above.

## VIII.3 Audio (`audio-manager.js`, `music-player.js`, `sound-defs.js`)

**SFX**: jsfxr-generated WAVs in `sfx/`, `playSound(name)` throttled per
`SOUND_THROTTLE_MS` (default 30), master `sfxMasterVol 0.8`. Specific→generic
fallback (`enemyDestroy_HUNTER` else `enemyDestroy`). Event catalog (names, not
synthesis): `shoot, tractorBeam, arcLightningLoop, arcStrike1..4, arcHit1..3,
laserBeamLoop, laserBeamHit1..3, coin, powerup, healthRegen, playerHitAsteroid,
playerHitEnemy, playerExplosion, shield, hit, enemyHit, explosion, asteroidDestroy,
enemyDestroy[_<TYPE>], playerHit_<WEAPON>, enemyHit_<pattern>, bulwark, repairNanites,
phaseDash, deflectorOrbs, empPulse, tractorShield, menuClick`. Loop API
`startLoop/stopLoop` for beams (skips attack transient on wrap).

**Music**: HTML5 `<audio>` streaming, Fisher-Yates shuffle, single-track preload,
auto-advance on `ended`, auto-skip-on-error, default volume 0.5. **Port plan**:
pre-render SFX offline to `.ogg`/`.wav` and play via `bevy_kira_audio`/`kira`;
stream + cache music from the CDN (389 MB, never bundle). See `port-plan.md` §4.

## VIII.4 Shop (`shop-manager.js`)

Unified skill-tree shop (`shopCategory='TREE'`). Opens on demand (button/pause),
**not** auto-opened between waves. Single live currency = **gold** (`game.money`).
**Cost = base × `UPGRADE_COST_MULT(13)` × `1.6^(stack−1)`, rounded to 500, floor
500.** Selling refunds the exact at-cost price of the last stack.

PRIMARY weapon upgrades = the 8-trait set (Part III.2). POWER upgrades
(base costs, pre-scale):

| id | weapon | maxStacks | base | effect |
|---|---|---|---|---|
| CHARGE_POWER | Charge | 6 | 1600 | +0.5 charge dmg |
| CHARGE_SPEED | Charge | 3 | 3200/6400/10500 | −1s charge |
| CHARGE_OVERCHARGE | Charge | 1 | 4300 | full charge explodes |
| CHARGE_HOMING / CHARGE_PIERCING | Charge | 3 | 1800 | seek / +1 pierce |
| EXTRA_PAYLOAD | Mine | 2 | 1500 | +1 max mine |
| BLAST_RADIUS | Mine | 3 | 1700 | +30px blast |
| DAISY_CHAIN | Mine | 1 | 4300 | chained detonation |
| RAPID_DEPLOY | Mine | 2 | 2400 | −25% cd |
| MINE_SHIELD_RADIUS | Mine | 3 | 1700 | +50px shield |
| MINE_MISSILES | Mine | 1 | 2300 | mines fire homing missile |
| SHOCKWAVE | Nova | 3 | 1700 | +40px ring |
| AFTERSHOCK | Nova | 1 | 2600 | slow on hit |
| DOUBLE_PULSE | Nova | 1 | 4300 | 2nd ring |
| RESONANCE | Nova | 2 | 3200 | −1.5s cd |
| NOVA_LIGHTNING | Nova | 2 | 1900 | stun on hit |
| NOVA_CHAIN | Nova | 1 | 4300 | kills spawn novas |
| NOVA_INFERNO | Nova | 1 | 2300 | burn on hit |
| EXTRA_ORDNANCE | Missile | 2 | 2200 | +1 missile |
| CLUSTER_WARHEAD | Missile | 1 | 3900 | split ×3 |
| QUICK_RELOAD | Missile | 2 | 3200 | −2s cd |
| MISSILE_PIERCING | Missile | 2 | 1800 | +1 pierce |
| BEAM_WIDTH | Lance | 3 | 1100 | +30% width |
| LINGER | Lance | 3 | 1500 | +0.1s |
| REFRACTION | Lance | 1 | 2700 | splits on hit |
| OVERLOAD_BEAM | Lance | 1 | 2300 | final 0.1s ×3 |
| LANCE_VELOCITY | Lance | 3 | 1700 | +15% dmg |
| TRIPLE_BEAM | Lance | 1 | 9000 | MASTERY +150% dmg (req BEAM_WIDTH×3) |
| AMPLIFIER | Arc | 3 | 1500 | +20% dmg |
| ARC_OVERCHARGE | Arc | 1 | 7500 | MASTERY +60% dmg (req AMPLIFIER×3) |

SKILL upgrades: SP cost → gold via `SKILL_SP_TO_GOLD 800` × `UPGRADE_COST_MULT`.
PASSIVE tab = the survivor-card pool (Part III.5). Mastery unlock toast on first
prereq satisfaction (2800 ms). Suspended DEFENSE list (costs documented): HEALTH
1200, SHIELD 1500, SPEED 2200, Triage 1800, REFLEXES 5500, LAST_STAND 8000,
STATIC_FIELD 3200, SPARE_SHIP 12000.

## VIII.5 Icons (`ui/icons.js`)

SVG registry (`ICON_PATHS`, 24×24, `currentColor`). Slugs: `shield, bolt,
multi-shot, wind, circle-fill, bow-arrow, bomb, target, heart, dagger, battery,
stopwatch, hourglass, fast-forward, pause, undo, skull, fist, sparkle, star,
vortex, wave, rain, tornado, medal, snail, ghost, pill, gem, anger, explosion,
dizzy, money-bag, chart, ruler, satellite, shuffle, loop, mute, volume, chain,
fire, flashlight, wrench, pistol, crystal-ball, rocket, bullet-train, siren, cart,
dna, magnet, coin`. Port as a small SVG/mesh atlas.

---

# Part IX — Bevy ECS mapping & phasing

## IX.1 Architecture invariants

- **Fixed 60 Hz `FixedUpdate`** for all simulation; max 4 catch-up steps; clamp
  frame dt to 100 ms; advance a logical clock per catch-up step. Render/interpolate
  in `Update`. **Never** use `delta_seconds()` for sim movement — advance the
  per-tick constants (Part I.1).
- **Simulation never touches rendering; rendering never mutates sim** (keeps
  headless tests viable — the existing `gate_tests.rs` pattern).
- Spawning runs **last** in the tick (after collisions/cleanup).

## IX.2 Component / resource / system map

| JS concept | Bevy |
|---|---|
| `class Enemy {x,y,vx,vy,hp, kind}` | entity + `Transform, Velocity, Health, Enemy{kind}, FireCooldown, AiState` |
| `enemy.update()` | per-kind `FixedUpdate` systems matching on `EnemyKind` (current `systems/enemy/<kind>.rs`) |
| `gameEngine`, pools, `money`, `wave` | resources: `Score`, `Wallet`, `Wave`, `EnergyMeter`, `Rng`(thread RNG), `PlayBounds` |
| `gameState` string | `States` enum (Part I.7) |
| cross-entity calls | `Message`s: `Collision, Damage, Death, Fire` (already present) — add `Pickup, SpawnDrop, ApplyKnockback, Stun` |
| spatial grid | a `Resource` holding the 8×6 grid, rebuilt each tick, OR `bevy`'s spatial query if perf allows |
| `requestAnimationFrame` + accumulator | `FixedUpdate` schedule |
| Canvas2D silhouettes | `bevy_prototype_lyon` paths (Part II.4, IV.4) |
| WebGL particles | `bevy_hanabi` GPU effects (Part VI.6) |
| WebGL bullets | instanced `Mesh2d` or a `Material2d` SDF (Part VII.2) |
| HDR/bloom fake-glow | HDR `Camera2d` + `Bloom`, emissive >1 (Part VII.1) |
| localStorage | `serde` + `ron` to OS config dir |

## IX.3 Faithful-port phasing (supersedes `port-plan.md` Phase 3+ where stricter)

The current repo has Phase 1 (core loop) and Phase 2 (renderer) done and Phase 3
(combat) underway. The remaining faithful-port work, in dependency order:

1. **Timestep correctness** — confirm `FixedUpdate` at 16.6667 ms with the
   accumulator/clamp; port the per-tick constants exactly (`MAX_V 3.5`,
   `BULLET_SPEED 8`, `AST_SPEED 1.75`, friction `0.7071`). *Gate:* a headless
   movement test reproduces JS positions within float epsilon for a fixed input.
2. **Player parity** — movement bounce-not-wrap, dash (250/1500/135), shield/tanks,
   the `takeDamage` pipeline order, no post-hit i-frames. *Gate:* damage-pipeline
   scenario tests (dodge → shield → bulwark → tank → death).
3. **Weapons & combat economy** — 5 primaries + 8-trait upgrades, 6 power weapons
   (energy-gated), 5 defense skills; crit/streak/knockback/stun; the
   `×13 / ×1.6 / round-500` cost model. *Gate:* fire-pattern + crit/streak tests.
4. **Enemy fidelity** — per-kind movement formulas (Part IV.2), fire patterns
   (IV.3), level + campaign scaling (IV.1, V.4), bullet patterns (IV.5). *Gate:*
   per-enemy scenario tests (spawn, approach, fire cadence).
5. **Waves & progression** — the 30-wave table (V.2), kill-gated advance, pulse
   pacing (≤2 or 12 s), spawn positioning, boss tiers + rage, mini-boss promotion,
   missions, survivor-card/shop flow. *Gate:* full-campaign headless run reaches
   wave 30 → GAME_COMPLETE with correct counts.
6. **Drops & world** — asteroid split paths (3–4 vs 2), drop tables + tiers, gold
   magnet hierarchy, powerup effects, item-affix loot feed. *Gate:* drop-budget +
   tier scenario tests.
7. **HUD / shop / states** — full state machine, HUD elements, the tree shop, save
   schema. *Gate:* title → play → shop → death → restart with zero web deps.
8. **Audio + input** — baked SFX (event catalog VIII.3), music stream/cache,
   full control scheme + gamepad. *Gate:* fully playable with sound + controller.

## IX.4 Testing strategy

Mirror `gate_tests.rs`: spin a minimal `World` with only the systems under test,
drive N fixed ticks, assert on the `World`. Because gameplay RNG is unseeded,
assert on **ranges and invariants** (HP after a known hit, bullet count for a fan,
wave advance conditions), not exact sequences. Re-express the JS E2E scenarios
(per-enemy kills, wave progression, survival) as headless Bevy runs (port-plan §9).
Visual parity stays a manual screenshot-diff judgment (the `DPS_SCREENSHOT` hook).

---

# Appendix A — global constants quick reference

```text
Timestep:    LOGIC_HZ 60   LOGIC_TICK_MS 16.6667   TICK_SCALE 0.5   maxSteps 4   dtClamp 100ms
Field:       1920 × 1080   (+X right, +Y down)
Ship:        SHIP_SIZE 30   radius 15   thrust 1.0/tick   friction 0.7071/tick   MAX_V 3.5
             baseFireRate 400ms   maxHP 40 (cap 600)   shield 15% (cap 75%)   tanks 1 (max 3, eff 4)
             crit 8%/200% (cap 60%/550%)   dash 250ms/1500ms/135px   energy 0..100 (+4/hit)
Bullet:      BULLET_SPEED 8   radius 4   maxLife 480 ticks
Asteroids:   AST_SPEED 1.75   MAX_ASTEROIDS 16   spawn r 30–60   split>20px (3–4 bullet / 2 AoE)
Particles:   MAX_PARTICLES 2500 (soft, grows)   star buffer 4000
Campaign:    MAX_WAVES 30   WAVES_PER_STAGE 3   BOSS_WAVES [3,6,9,…,30]
Waves:       pulse advance ≤2 enemies OR 12000ms   wave start +700ms spawn / +2800ms play / 3000ms invuln
             survivor-card delay 2700ms   bonusCoins stageClear?×2:×0.6 of (50+wave*25)
Economy:     UPGRADE_COST_MULT 13   UPGRADE_STACK_RAMP 1.6   round-to-500 floor-500
             STREAK_BUFF_DURATION 4000ms (resets on DAMAGE, not timer)   streak cap 200 kills (×3.0)
Boss tiers:  T1 hp4.0/sz1.35/sp1.00/500 · T2 5.0/1.45/1.05/1000 · T3 6.0/1.55/1.10/1750 · T4 8.0/1.75/1.15/3000
Rage:        threshold HP≤33%   telegraph 24f   invuln 1500ms   fireRate×0.66   tantrum 16 bullets
Spatial:     8×6 grid (240×180 cells)
Camera:      follow centered, lerp 0.1, zoom 1.0 (desktop)
```

# Appendix B — parity invariants & gotchas

1. **Fixed-tick, not dt-scaled.** All speeds are px/tick @60 Hz; movement advances
   constants, not `dt * speed`.
2. **Gameplay RNG is unseeded** `Math.random()`. Non-deterministic by design.
3. **Ship bounces off field edges** (`*0.8`), it does not wrap. Wrap is dead code.
4. **No post-hit invulnerability.** Only REFLEXES/LAST_STAND/dash grant i-frames.
   Tank consumption refills HP with zero invuln.
5. **Kill streak resets only on taking damage**, never on a timer.
6. **Contact damage is uniform** across enemies (`LS(25)`); only *bullet* damage
   differs per enemy.
7. **Enemy visual size does not scale with level** (only HP/speed/dmg/fire-rate).
8. **Wave advance is kill-gated only**; asteroids never block. Pulse 0 is immediate.
9. **W29 P2 TITAN is a normal enemy**, not a boss. `isBossWave` = the `[3,6,…,30]`
   list, independent of per-group flags.
10. **Power weapons are energy-gated** (6.29.0), not cooldown-gated; per-weapon
    `cooldown` is a short anti-spam floor.
11. **Bullets render with normal alpha**, not additive; only particles + starfield
    are additive.
12. **The lens-flare nebula is disabled** — the live nebula is the WebGL atlas
    slots (FBM). Powerups/items never drop as world pickups; only gold + health orbs do.
13. **Two asteroid split paths**: bullet inline = 3–4 fragments, `destroyAsteroid`
    (AoE) = 2. Both gate on `baseRadius > 20`.
14. **`MONEY_ORB_SHAPE_VALUE_MAX` is 80** (a code comment wrongly says 200).
15. **In-run leveling is retired**; growth is gold-bought powerups + item affixes +
    survivor cards. Meta SP progression is cross-run only.
16. **Many upgrade IDs are read but inert** (legacy/`hidden`). Port the live tables;
    leave the dead branches out or dormant for save-compat.
