# dps — Working Plan

**dps** (*Dark Prism Solid*) — native **Rust + Bevy 0.18** port of the solo
**Rainboids** game. Living plan: where the port stands, what's left, how the
source repo diverged. Updated **2026-05-23** (branch `master`, tree clean).

> 🚩 **SCOPE RE-TARGET (2026-05-24):** the user re-opened the port to match the
> **current** rainboids (now **v6.161** — a full roguelite restructure that was
> previously "out of scope"). The comprehensive plan for that lives in
> **[`docs/roguelite-port-plan.md`](docs/roguelite-port-plan.md)**. THIS file
> describes the completed **v6.55-era** port (still accurate for the engine/render/
> combat primitives that carry over); the new doc supersedes it on scope.

## Source of truth

- **Port target = `docs/exact-port-spec.md`** — the authoritative 1:1 *behavior*
  spec, reverse-engineered from the JS at `/Users/silvr/projects/rainboids/js/
  modules/*` at **rainboids ~v6.55**. Supersedes `port-plan.md` on any number.
- Other docs: `port-plan.md` (staged delivery/phase gates), `comparison.md` (why
  Rust+Bevy), `lyon-vs-vello.md` (**stay on lyon** — vello breaks HDR bloom).
- The JS source is a **separate repo**; dps doesn't contain it. See the
  `js-source-of-truth`, `phase-status`, `no-cargo-fmt` memories.

> ⚠️ **`../rainboids` moved past the spec** (now v6.83: element overhaul, 4-slot
> skills, item rarity, +10 enemies). All **out of scope** — see [§4](#4-rainboids-divergence).

---

## 1. Status

Phases 0–4 closed; Phase 5 (UI) largely done; Phase 6 (packaging) untouched.
**75 headless tests pass**; binary boots the full Title→Play→Shop→Survivor→Death/
Complete loop crash-free. Release build verified (`target/release/dps`, ~47 MB).

**Commit/push policy:** push to `master` is **blocked** (pre-push hook; user pushes
manually). Commit each green increment to local `master`; standing authorization to
commit autonomously during a port loop. **Never run `cargo fmt`** (see `no-cargo-fmt`
memory — manual-alignment style, churns ~36 files).

**Ported (by spec part):**
- **I Engine:** 7 states; `FixedUpdate` sim isolated from render; spawn-last order.
  *Caveats:* dt-scaled not fixed-px-tick; direct collision (not the 8×6 grid);
  `GameRng` seeded for test stability.
- **II Player:** HP 40, **%DR shield** (15%, cap 75), tanks + overheal overflow,
  dash i-frames, **no post-hit i-frames**, passive regen, Last Stand, dodge.
  *Movement is twin-stick screen-space, not the spec physics — see [§5](#5-known-divergences-decisions-needed).*
- **III Weapons/combat:** 5 primaries + **8-trait** upgrades; 6 energy-gated power
  weapons; 6 defense skills; crit, kill-streak, knockback, stun, burn; `×13/×1.6/
  round-500` cost model; contact + damage pipeline w/ positional separation.
- **IV Enemies:** all 10; per-kind movement (detailed) + fire patterns; lyon
  silhouettes; boss tiers T1–T4; HP-threshold rage (telegraph + warning ring) +
  tier-2 pair-link + rage homing bullets.
- **V Waves:** full **30-wave/10-stage** table; pulse pacing; kill-gated advance;
  4-edge spawn; full **V.4 difficulty curve** + boss speed; mini-boss promotion;
  survivor cards (stage-gated) + rewards; stage/wave title overlays.
- **VI Drops:** gold value tiers, point orbs, cooldown-gated health orbs
  (desperation curve), magnetic pickup, enemy drop profiles.
- **VII Render:** lyon silhouettes, instanced bullets, hanabi particles, parallax
  starfield, region-cloud nebula, bloom (threshold 1.0); **3D-tumbling asteroid
  wireframes** (VI.1); camera **screen shake** + **screen flash** (`render::shake`
  / `render::flash`, spec I.2).
- **VIII HUD/audio/input:** Bevy-UI HUD (+ triforce, energy sphere) + damage
  numbers + minimap; shop (26 upgrades); per-wave missions; twin-stick
  kb/mouse/gamepad; local SFX + music.

---

## 2. Remaining work

**Group A is gated on a user playtest** (harsh combat: HP 40, no i-frames, full V.4
ramp); B–C are buildable; D is blocked.

### A. Playtest-gated balance & feel
*Do **after** the user plays `cargo run --release` — building blind risks tuning the
wrong thing or regressing a working feel.*
- **Survivability upgrades** — *Reflexes* (free-dodge clock), *Triage* (−orb-cd).
  Premature until we know the player survives to need them.
- **Per-enemy movement fine-tuning** (IV.2) — AI is already detailed; exact px/tick
  constants are feel-based, unverifiable headlessly.
- **Timestep px/tick alignment** (I.1) — **risky.** Sim is dt-scaled; JS is fixed
  60 Hz px/tick (`MAX_V 3.5`, `BULLET_SPEED 8`, `AST_SPEED 1.75`). Retunes *every*
  speed — only with playtest feedback. *Gate:* headless movement test matches JS.

### B. Buildable systems (mostly autonomous)
- ~~**Powerup effects** (VI.3)~~ — ✅ **DONE** (`83604fd`…`faa7d9d`): Executioner,
  Phase Echo, Overcharge, Static Discharge, Combat Medic, Momentum, Whirlwind, all
  as shop upgrades read live by combat (`systems::passives` homes the AoE ones).
  *(The world-drop-vs-shop/card acquisition model remains the §5 decision; these
  ship via the shop, which needed no decision.)*
- ~~**Item-affix system** (VI.5)~~ — ✅ **COMPLETE** end-to-end (`41536cd` rolls →
  rarity-tinted loot feed; `0d0f76b` fade; `1167965` auto-equip + 4 defensive
  affixes; `761f3cd` offensive Vampirism/Crit/Speed; `a7a4545` MAX HP; `dce0389`
  right-edge **gear panel**; `236f540` ▲ equipped marker on cards). All 9 affixes
  affect the run; loot feed (with upgrade markers) + gear panel show drops + the
  equipped build. The full VI.5 system is done.
- ~~**Missions** (V.6)~~ — ✅ done (`21cee84` + `485613d`): all 5 objectives
  (no-damage/fast-kill/asteroid/streak/precision) + HUD line + wave-scaled **gold**
  reward (SP→gold substitution, since meta/SP is retired). Added a reusable `Crit`
  message for the precision count.
- ~~**Shop-suggest chain** (V.6)~~ — ✅ done (`7343afa`): a stage-clear survivor
  pick now chains into the shop (a "spend your gold" break) → next wave. *(Shows
  the full shop; the curated 3-cheapest-affordable suggest is a refinement.)*
- **Boss formations** (IV.7 t3+) — `_formationCenter` orbit + spring; t4 phase
  toggle. Only with 3+ bosses; complex, rare, feel-based. (Generic-formation
  machinery now exists to build on.)
- ~~**Generic formations** (IV.6)~~ — ✅ done (`4a20a4d`): ≥3 fresh non-boss members
  bind to orbit/weave/flank/cross/figure8 slots, overriding AI ~6–9 s
  (`systems::formations`, hooked into the wave pulse). *Feel needs playtest tuning.*
- **Save/load** (I.8) — serde+ron to OS config dir. **Premature** (run resets by
  design; no meta progression yet).

### C. Cosmetic / visual polish
- **HUD cosmetics** — triforce tank glyphs ✅ (`cd9a34b`), energy sphere ✅
  (`8e38f2c`), pulse-phase toast ✅ (`0b8c4e8`). Remaining: loot feed (needs the
  item system), per-wave `WAVE_SUBTITLES` text (strings live in v6.83 JS — scope).
- **Weapon visual effects** (III.7) — Lance/Arc zig-zag beam ✅ (`419c1ea`).
  burn/stun enemy auras ✅ (`ca98e9d`). Remaining: cluster charge telegraph.
- **Asteroid wireframe** (VI.1) — ✅ done (`6c7ee20` + `867b0c6`): 3D-tumbling
  icosahedron, **pulsing moving rainbow-gradient** vertex colors (custom `Mesh2d`),
  depth fade, HDR/bloom. *Remaining (cosmetic):* the black contrast underlayer.
- **Nebula/starfield fine-tuning** — user-earmarked (live fbm JWST vs. baked perf).
- **Juice polish** — done: rage telegraph + ring (`bc5b7bd`), camera shake
  (`52a004d`), rage flash (`a66f4c7`), telegraph ring-pulse (`c9e6479`), flash gold
  channel (`df0895a`), **hitstop** on boss/mini kills (`ae1b9f2`). Remaining: ember
  particles (hanabi, transient — marginal).

### D. Blocked
- **Music streaming** (port-plan §4) — needs a CDN URL + track list we don't have;
  music already plays locally from `music/`. Leave as-is.

---

## 3. Known divergences (decisions needed — flag, don't silently "fix")

1. **Player movement** — twin-stick screen-space `move_dir`, now **velocity-tracking
   for tight, low-momentum control** (`6c7ee20`, user-requested) — *intentionally*
   not the spec II.1 thrust/friction/edge-bounce physics. Tune via `RESPONSE_K` in
   `movement.rs`. (The spec-physics port remains an option, tied to timestep, A.)
2. **Powerup acquisition** — ours drop as world gems; spec VI.3 says powerups are
   shop/card rewards, not kill drops. *Decide before the full catalog (B).*
3. **Static Field / shield-as-pool effects** — several spec powerups assume an
   absorbing-pool shield; we use **%DR**. Need re-expression or omission.
4. **Seeded RNG** — `GameRng` seeded for test determinism vs. spec's unseeded
   `Math.random()`. Deliberate (tests assert ranges); no action unless it affects feel.

---

## 4. `../rainboids` divergence

Port started **2026-05-20**; spec froze at **~v6.55** (2026-05-21). Everything in
rainboids **6.56→6.83** (all 2026-05-22) is a **post-spec divergence the port does
not chase** (user: "high-level changes that likely don't affect us now"):

| ver | change | stance |
|---|---|---|
| 6.71–6.83 | +10 enemies (elemental + Hydra/Spore/Lumen) → roster 20; hazard fields, support auras, mid-fight spawning | out of scope |
| 6.75–6.76 | player elemental statuses (chill/corrode/burn) | out of scope |
| 6.57–6.70 | **Element & Resistance System** (E1–E10) + enemy archetypes — combat overhaul | out of scope |
| 6.59–6.61 | 4-slot skill loadout + 8-tier item rarity | out of scope |
| 6.56 | +6 primaries / +5 power weapons / Sentry Drone | out of scope |

**Stay anchored to `exact-port-spec.md` (~v6.55).** Re-target only on explicit user
request (a major re-scope, not a reconciliation). Low-level notes still in scope:
- 6.54.2 "enemies always explode on death" — ✅ verified: DoT/burn kills route
  through the uniform `Death` finalize (loot+explosion+credit).
- 6.55.0 Lance arc-sweep + Cluster hold-to-charge are post-spec; our pre-6.55
  forward-ray Lance / instant Cluster are spec-faithful but stale (cosmetic).

---

## 5. Bevy 0.18 gotchas (carry-forward)

- FixedUpdate `.chain()` tuples hit Bevy's **20-element limit** — nest sub-tuples.
- **B0001** = intra-system query conflict (same-component mut+immut needs a disjoint
  filter, e.g. `Without<Enemy>`). Names are stripped — read backtrace `(P0..Pn)`
  arity. Catch via a schedule-run test *or* boot (`RUST_BACKTRACE=1 ./target/debug/
  dps`); green tests alone don't render.
- Boot-into-Playing for B0001/visual checks: temp-flip the `GameState` `#[default]`
  to `Playing`, `DPS_SCREENSHOT=<path> DPS_DEMO=1 DPS_NO_NEBULA=1 ./target/debug/dps`,
  then revert. (`DPS_DEMO` keeps the player alive + auto-fires.)
- `Entity::index()` opaque → `entity.to_bits() as u32`. `ColorMaterial`/`AlphaMode2d`
  in `bevy::sprite_render`; assets resolve exe-relative. `Sprite` has no additive
  blend. Bevy needs the `mp3`+`wav` features.

---

## 6. Loop status — PAUSED (2026-05-24; 32 commits this session)

Shipped, all tested (94 pass) + screenshot/boot-verified where visual: rage
homing+telegraph, screen shake+flash, **3D rainbow-gradient asteroids**, tight
velocity-track controls, **Lance/Arc lightning beams**, burn/stun auras, full **HUD
cluster** (triforce + energy sphere) + pulse toast, **per-wave missions** (5
objectives), the **complete powerup-effects seam** (7 effects), the
**shop-suggest chain**, the **full item-affix VI.5 system** (rolls → loot feed →
auto-equip → 9 affix→stat → gear panel), **generic formations IV.6**, and
**hitstop I.1** (boss/mini-kill impact freeze).

**The entire SAFE + VALUABLE buildable spec is now done.** Every remaining item is
risky-blind, rare, or marginal — so the loop paused (a PLAYTEST is the real unlock):
- **Playtest** `cargo run --release` (THE next step) — unlocks the group-A balance pass.
- **Timestep** (I.1) — high value but retunes EVERY speed → would likely regress the
  tuned feel. *Don't do blind* — needs playtest feedback.
- **Boss formations** (IV.7 t3+) — additive but rare (needs 3+ co-occurring tier-3+
  bosses; may never trigger) → low value. **Marginal cosmetics**: telegraph embers,
  WAVE_SUBTITLES (v6.83 scope), nebula (user's call).

Resume by: PLAYTESTING, or naming an item (boss-formations / a cosmetic / "attempt timestep").
