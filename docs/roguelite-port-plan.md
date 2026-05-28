# dps — Roguelite Port Plan (target: rainboids v6.161)

**Status:** NEW SCOPE, supersedes the v6.55-era `plans.md`/`docs/exact-port-spec.md`.
**Written:** 2026-05-24. **Target:** rainboids **v6.161.0** (the live JS at
`/Users/silvr/projects/rainboids/`).

---

## Progress snapshot — 2026-05-28 (~45 commits in, 200 tests green)

The roguelite spine is **substantially complete and functional end-to-end.** Per-phase:

| Phase | Status | Notes |
|-------|--------|-------|
| **E** — elements/resist/status/reactions | ✅ **DONE** | 7 elements, resist math, 9 statuses (incl. player burn/chill/corrode), shatter + oil-flare reactions. |
| **W** — weapons/attunements | ✅ **DONE** (mods deferred) | 11 primaries + 11 power weapons, per-weapon element, attunement cycle, Overdrive; per-weapon upgrade *trees* + mechanic *mods* not ported. |
| **EN** — enemy roster | ✅ **DONE** | all 20 kinds + special mechanics (Hydra split, Spore drones, Plaguebearer hazard, Lumen aura, Warden adapt, Ashen flare) + generic formations. |
| **AB** — 4-slot abilities | ✅ **DONE** | all 14 abilities wired, loadout picker, HUD ability bar; Numpad 1–4 (Digit rebinding + per-ability attunements deferred to a future input/UI pass). |
| **ME** — meta/persistence/economy | ✅ **DONE** | XP→account level→SP (12 stats, ALL wired into gameplay); armory gold-unlocks (weapons/abilities/attunements) + cycle/loadout gating; RON persistence of gold/level/sp/sp_alloc/unlocks/ability-loadout/weapon/attunement; run banking; 3 native-UI menu overlays (armory / skills / loadout). |
| **POL** — vfx/audio | ✅ mostly **DONE** | status auras (all 9, enemy + player), reaction shockwaves, hitstop, gold flash, telegraph pulse, loot feed, damage numbers; player VFX (engine trail, muzzle flash, hit-flash, charge-glow, impact sparks, dash afterimage, shield bubble, low-HP pulse, level-up aura), enemy-element halos; **comprehensive synth SFX** (shoot, power-fire, explosion, hit, pickup, reactions, crit, level-up, dash, bomb, shield, ability-cast). Balance pass not done. |
| **PA** — 44 passives + card draft | ✅ **DONE** (align-to-built) | unified `UpgradeId` catalog (now **41**) is BOTH the shop AND the wave-clear `survivor::POOL` card draft (22 cards) — covers PA per "align to what's built"; near rainboids' ~44 target. NOT a separate keystone registry. |
| **IT** — item v2 | 🟡 **PARTIAL** | 8-tier rarity + 15 affixes (HP/toughness/vamp/thorns/crit/dodge/speed/regen + **6 per-element resists**) + equip/HP/resist reconciliation done. **Typed player damage is now COMPLETE** (contact + ranged carry elements; resist affixes + Warding feed player `Resistances`) — the old resist-affix blocker is RESOLVED. cores + stash still not built. |
| **PU** — powerup catalog | 🟡 **PARTIAL** | `powerups.rs` drops 3 kinds; the ~25-entry permanent catalog is largely subsumed by `UpgradeId` per the acquisition-model decision. |
| **UI** — BUILD tree / overlays / skins | 🟡 **PARTIAL** | egui **IS** in Cargo.toml; the unified **egui BUILD tree is built** (`build_screen.rs`, B on title — Skills/Armory/Loadout tabs all interactive) alongside the HUD ability bar + 4 native overlays. Remaining: inventory/hangar/stats overlays, radial menu, 12 skins, and egui **aesthetic/layout polish** — wants USER direction. |
| **BO** — boss chassis | ⬜ **NOT STARTED** | dps live bosses = TITAN + tier-rage; rainboids' phase-script/weak-point chassis is shipped-but-UNUSED in the JS too, so low priority. |
| **X** — run configurator / adaptive difficulty | ⬜ **NOT STARTED** | forward-looking; not shipped in rainboids either. |

**Biggest remaining piece:** egui BUILD-tree **aesthetic/layout polish** + the extra
overlays (inventory/hangar/stats), radial menu, and 12 skins (Phase UI) — these need the
user's look/layout input. The functional spine (combat, weapons, enemies, abilities, meta,
41-passive catalog + card draft, comprehensive VFX/SFX) is **built and functional**;
everything the user named as in-scope ("cards every stage, gold for unlocking weapons/
abilities/attunements", typed damage + resist affixes, the egui BUILD screen) is done.
Remaining beyond UI polish: item cores/stash, boss chassis (BO), run configurator (X) —
all lower-priority or not shipped in the JS either.

---

## 0. Why this document exists

`dps` is a faithful Rust+Bevy port of Rainboids **~v6.55** (the `exact-port-spec.md`
freeze). Since then the JS game has been **completely restructured into a roguelite**
(v6.56 → v6.161, ~100 minor versions). The earlier `rainboids-divergence` memory marked
all of that "out of scope pending an explicit re-target." **This is that re-target.**

The goal: bring `dps` "much more like the `../rainboids` version" across gameplay,
enemies, RPG systems, skills, stats, UI, and graphics. This is effectively a **v2 of the
game**, not a reconciliation. The list below is large; it is phased by dependency so each
phase ships something playable.

### Source of truth
The authoritative target is **the shipped JS code at v6.161**, NOT the old spec doc
(which froze at v6.55 and is now wrong on most numbers). When this plan says "port from
X.js", read the live file. The design docs explain *intent*; the code is *truth*. Key
JS data files (all under `js/modules/`):

| System | Authoritative file(s) |
|---|---|
| Elements / resist / reactions | `combat/elements.js`, `combat/collision-system.js` (`applyDamageToEnemy`, `_triggerStatusReactions`), `combat/combat-manager.js` (status applicators) |
| Weapons (11+11) + upgrades + attunements + mods | `combat/weapon-data.js`, `player/weapons.js`, `player/bullet.js`, `player/abilities.js` |
| Abilities (14, 4-slot) | `combat/weapon-data.js` (`ABILITIES`), `player/abilities.js`, `player/player.js` |
| Passives (44) + card draft | `combat/passive-data.js`, `player/passives.js`, `combat/card-draft.js` |
| Enemies (20) + AI + firing + auras | `enemy/enemy-data.js`, `enemy/ai.js`, `enemy/movement.js`, `enemy/firing.js`, `enemy/support-aura.js`, `enemy/formations.js`, `world/hazard-field.js` |
| Boss chassis | `enemy/boss-phases.js`, `enemy/boss-parts.js`, `enemy/boss-intro.js`, `enemy/boss-rage.js` |
| Items (8-tier) + cores | `world/item-names.js`, `world/item-system.js`, `world/inventory.js`, `world/cores.js` |
| Meta / progression / SP | `player/progression.js`, `core/sp-stats.js`, `core/storage.js`, `shop/armory.js` |
| Shop / economy | `shop/shop-manager.js`, `shop/shop-dom.js`, `world/run-shop.js`, `world/gold-*.js` |
| Powerups | `world/powerup.js` |
| UI / HUD / input | `ui/*-overlay.js`, `ui/radial-menu.js`, `hud/*.js`, `ui/input-handler.js`, `ui/gamepad-handler.js`, `player/skins/*` |
| Design rationale (read once) | `docs/Roguelite Restructure — *`, `docs/Element & Resistance System — *`, `docs/Weapon Element Identity & Meta-Progression — *`, `docs/Passive Skills & Run Difficulty — *`, `docs/Run-Meta Overhaul — *`, `docs/Enemy & Boss Revamp — *`, `docs/Balance Model — *` |

### What is shipped vs. planned in rainboids itself
Two target areas are **planned in rainboids but not yet shipped** — port them last, and
expect to track rainboids' own progress:
- **Phase D.B1–B5** — the 10 unique scripted bosses. The *chassis* (phase scripts /
  weak-points / intro-death sequences) is shipped but **unused**; live bosses are still
  TITAN + tier-rage. Port the chassis; keep TITAN-tier bosses live until rainboids ships
  concrete boss scripts.
- **Phase X** — run configurator + adaptive difficulty director + procedural wave
  composer. Replaces the static 30-wave table. Not shipped. Port the static 30-wave loop
  (already done in dps) and treat Phase X as forward-looking.

---

## 1. Current `dps` state (the baseline we extend)

Phases 0–6 of the *old* plan are done. Concretely, the codebase has:

- **States** (`states.rs`): Title · Playing · Paused · Shop · Survivor · GameOver · GameComplete.
- **Combat**: damage pipeline with crit (seeded `GameRng`), kill-streak (`resets on damage`,
  not idle-timeout), positional separation, knockback. `Burning`/`Stunned` exist; **no
  elements/resist**.
- **Weapons** (`systems/weapons.rs`): **5 primaries** (Pulse/StormNeedles/Scatter/Rail/Cluster).
  **6 power weapons** (`power_weapon.rs`: Charge/Mine/Nova/Missile/Lance/Arc), **energy-gated
  at +4/hit** (JS now passive-regen).
- **Skills** (`systems/skills.rs`): **8 keybind-bound** skills (dash/shield-burst/bomb/EMP/
  bulwark/repair/deflector/tractor) — **not** a 4-slot loadout.
- **Enemies**: **10 kinds** (`EnemyKind`), per-kind AI + firing, lyon silhouettes; boss
  tiers T1–T4 + rage (telegraph, pair-link, homing); mini-boss promotion; generic formations.
- **Waves** (`systems/wave.rs`): static **30-wave/10-stage** table, pulse pacing, 4-edge spawn,
  V.4 difficulty curve. Survivor cards (stage-gated) + missions.
- **Items** (`systems/items.rs`): **3-tier** (common/rare/epic), 9 stat affixes, 5 slots,
  **auto-equip-if-better**, **run-scoped (no persistence)**, loot feed + gear panel.
- **Shop** (`systems/shop.rs`): **one gold currency**, **26 upgrades** in a single text menu
  (stat boosts + weapon traits + passives all mixed), `×13·1.6^owned` cost.
- **Render**: lyon silhouettes, hanabi particles, parallax starfield + region nebula, bloom,
  3D rainbow asteroids, screen shake/flash, hitstop, HUD (health/energy-sphere/triforce/
  minimap), damage numbers, status auras (burn/stun), zig-zag beams.
- **NO**: elements, meta-progression, account level/SP, account-gold, cores, persistence,
  attunements, mechanic-mod nodes, scripted bosses, hazard fields, support auras, ship skins,
  BUILD tree, 4-slot loadout, inventory/hangar/stats/armory/loadout overlays, radial menu,
  run configurator.

---

## 2. Gap summary (system × have/target/delta)

| System | dps now | rainboids v6.161 | Delta |
|---|---|---|---|
| Elements | none | 7 elements + resist + 8 statuses + 5 reactions | **NEW (foundational)** |
| Primaries | 5 | 11 | +6 + per-weapon upgrade trees (81 upgrades) |
| Power weapons | 6 (energy +4/hit) | 11 (passive energy regen) | +5 + regen model + 50 upgrades |
| Abilities | 8 keybind skills | 14 abilities, **4-slot loadout** | restructure + new abilities |
| Attunements | none | 92 weapon + 33 ability + mechanic mods | **NEW** |
| Passives | ~13 via shop | 44 (keystones + modular), card-delivered | restructure + ~30 new |
| Enemies | 10 | 20 + adaptive resist + auras + hazards + special mechanics | +10 + enabling systems |
| Bosses | TITAN tiers + rage | TITAN tiers + rage + **chassis** (phases/parts/intro) | port chassis; 10 scripted = forward |
| Items | 3-tier, run-scoped | 8-tier, 15 affixes (resists), **persistent stash + cores** | restructure + persistence |
| Meta-progression | none | account level 100, SP (8 stats), account-gold, armory unlocks | **NEW** |
| Economy | 1 gold pool | run-gold + account-gold + cores | restructure |
| Card draft | survivor cards | efficacy-card draft (2 wpn + 2 ability) + stat cards + keystones | restructure |
| Powerups | gem pickups (few) | ~25-entry catalog | expand |
| Persistence | none | meta + run-snapshot + settings (localStorage) | **NEW** |
| Run shape | static 30-wave | static 30-wave (Phase X configurator planned) | parity now; X forward |
| UI | text shop, HUD | BUILD tree + 6 overlays + radial + 4-slot HUD | **NEW (large)** |
| Skins | none | 12 cosmetic ship skins | **NEW** |
| Input | twin-stick + 8 skill keys | + ability slots 1-4, I/`/F-E-R radials | extend |

---

## 3. Phasing strategy & dependency order

Build foundational systems first; each later phase assumes them. Recommended order:

```
E (elements)  ─┬─►  W (weapons + attunements + mods)
               ├─►  EN (enemies + enabling systems + auras + hazards)
               └─►  AB (4-slot abilities)
                         │
PA (passives) ◄──────────┘   IT (items v2) ──► ME (meta + persistence + economy)
                                                      │
                          PU (powerups)               ▼
                                          UI (BUILD tree + overlays + HUD + skins + input)
                                                      │
                                                      ▼
                                          BO (boss chassis; scripted = forward)
                                                      │
                                                      ▼
                                          X (run config + adaptive difficulty) [forward]
                                                      │
                                                      ▼
                                          POL (VFX/audio/balance parity)
```

Each phase below lists **Goal · Depends · Port-from · dps-changes · Steps · DoD**.
Keep the existing dps discipline: one green (compiles + tests) increment per commit; headless
unit tests + boot-into-Playing B0001 checks; **never `cargo fmt`**; commit to local `master`.

---

## Phase E — Element & Resistance & Status System  *(FOUNDATIONAL — do first)*

**Goal:** a 7-element damage model with per-enemy resistances, 8 status effects, and the 5
reaction/synergy verbs. Everything downstream (weapons, enemies, attunements) attaches here.

**Depends:** nothing. **Port-from:** `combat/elements.js`, `combat/collision-system.js`
(`applyDamageToEnemy` :2356, `_triggerStatusReactions` :2483, `applyWeaponElementStatus`
:2772), `combat/combat-manager.js` (status applicators :2018-2159), `player/player-status.js`.

**dps-changes:** new `src/combat/element.rs`; extend `components/enemy.rs`, `components/player.rs`,
`components/projectile.rs`; rework `systems/collision.rs` + `systems/damage.rs` + `systems/status.rs`.

**Steps**
1. `Element` enum: `Kinetic, Pyro, Cryo, Volt, Toxic, Void, Radiant`. Each carries color +
   signature status id. `DEFAULT = Kinetic`. (`elements.js:15-27`)
2. `Resistances` component on enemies: a `[f32; 7]` map. `elementalMultiplier(r) = clamp(1-r, 0, 2)`
   (resist >0, weakness <0, immune =1). (`elements.js:39-44`)
3. `multi_element_multiplier(resist, &[Element])` = average of per-element mults (damage splits
   across a multi-element hit → "focus vs coverage"). (`elements.js:57-63`)
4. `Bullet.elements: SmallVec<[Element;N]>` (resolved from override → attunements → weapon base,
   `resolveBulletElements` :72-80). Default `[Kinetic]`.
5. **Status components** (extend the existing `Burning`/`Stunned`): `Chill{secs,slow}`,
   `Frozen{secs}`, `Conduct{secs}`, `Corrode{stacks,secs}`, `Mark{secs}`, `Oil{secs}`,
   `Bleed{stacks,secs}`. Durations/ticks per `combat-manager.js:2018-2159` (Burn 3000ms/500ms
   tick = `srcDmg×0.1×stacks` cap 3; Stun 1500ms; Corrode 4000ms cap 3 = +15%/stack; Chill
   2000ms ×0.7 spd; Freeze 1500ms; Conduct 3000ms; Oil 5000ms; Mark 6000ms; Bleed 4000ms/300ms
   `×0.08×stacks` cap 6 no-refresh).
6. **Damage pipeline** in `systems/damage.rs` — rebuild `apply_damage_to_enemy` in the exact JS
   order (`collision-system.js:2356-2521`): block checks → EMP_OVERLOAD ×1.2 if stunned →
   passive dmg mults → **element/resist mult** → corrode/conduct amplify → ally-shield mult →
   PURGE gate → flat ARMOR `max(dmg×0.25, dmg−armor)` → SENTINEL frontal shield → subtract →
   adaptive-resist bump → status reactions.
7. **Reactions** (`_triggerStatusReactions` :2483 + `applyWeaponElementStatus` :2772):
   - **Shatter** — Frozen target hit ≥6 → AoE 8 CRYO in r=110, re-freeze neighbors, chain depth ≤2.
   - **Oil Flare** — Oiled target hit by Pyro → burn AoE `max(2, dealt×0.4)` in r=100.
   - **Conduct** — VOLT ×1.5 to conducting target; fork 0.4× to nearest in 150.
   - **Corrode** — +15%/stack from all sources.
   - **Purge** — RADIANT bypasses armor + frontal shield.
8. **Player statuses** (`player-status.js`): enemy Cryo→chill ship (×0.7 spd 1500ms), Toxic→
   corrode (+15%/stack cap 2, 3000ms), Pyro→burn DoT. Apply in the player damage path.
9. Per-enemy `element` + `resist`/`armor`/`frontalShield` data table (port from
   `enemy-data.js:610-671`); stamp at spawn. (Existing 10 enemies get their elements here.)

**DoD:** unit tests for `elementalMultiplier` (resist/weak/immune/clamp), multi-element average,
each status duration/tick, shatter chain depth, oil flare, purge bypass, armor floor. A Pyro
shot on a Pyro-resistant enemy does reduced damage; freezing + a heavy hit shatters neighbors.

---

## Phase W — Weapons, Attunements & Mechanic Mods

**Goal:** 11 primaries + 11 power weapons with their real mechanics; element attunements that
re-element and reshape a weapon; mechanic-mod behaviors; per-weapon upgrade trees.

**Depends:** E. **Port-from:** `combat/weapon-data.js` (defs + `PRIMARY_UPGRADES` 81 +
`POWER_UPGRADES` 50 + `_ATTUNE_SPEC` 92 + mechanic-mod split), `player/weapons.js`, `player/bullet.js`,
`player/abilities.js` (mine/missile behavior), `combat/weapon-effects-renderer.js` (VFX).

**dps-changes:** rework `systems/weapons.rs` + `systems/power_weapon.rs`; new
`combat/weapon_data.rs` (data table) + `combat/attunement.rs`; extend `Bullet` (homing,
bounce, split-gen, boomerang state, etc.).

**Steps**
1. **Data-drive weapons.** Replace the hand-rolled `WeaponKind`/`PowerWeaponKind` with a
   `WeaponDef` table (id, element-base, fireRate, damage, bullets, spread, pierce, range, +
   per-weapon special params) ported verbatim from `weapon-data.js:6-969`.
2. **+6 primaries:** Mitosis Rounds (split 2 @0.5×, 2 gens), Caroms (3 bounces, seek r260),
   Boomerang Discs (out-and-back, hits both legs), Spin Cannon (spool-up 1400ms, 220→60ms),
   Flak Cannon (airburst @300px → 9 shrapnel + blast), Gravity Lance (VOID, slow orb pulls
   r150). Each needs its projectile mechanic on `Bullet`/new components.
3. **+5 power weapons:** Singularity (pull r280 1600ms → collapse 190/9), Prism Beam (5-ray fan),
   Orbital Strike (telegraph 850ms → column 150/15), Cryo Burst (ring 300, freeze 2500ms),
   Overdrive (4500ms primary buff: fireRate ×0.55, dmg ×1.5). Tag each with its element.
4. **Energy model swap:** power energy now **regenerates over time** + per-power cost +
   per-power cooldown (`POWER_ENERGY_COST` weapons.js:2230). Replace `EnergyMeter` +4/hit with
   passive regen; keep cost-gating.
5. **Weapon attunements** (`_ATTUNE_SPEC`): a weapon can carry several element attunements that
   STACK (damage splits per `multi_element_multiplier`). Store `active_attunements[weapon] →
   Vec<Element/special>`; resolve into `Bullet.elements` at fire. Specials: Spectrum, Focused,
   Frostfire.
6. **Mechanic mods** (`isMechanicMod` weapon-data.js:1714): `_PIERCING/_EXPLODE/_HOMING/_STUN/_KNOCK`
   + capstones (MEIOSIS, RAZOR_EDGE, IMPLOSION, FLYWHEEL, OVERSPIN…). dps already has the 5 base
   traits; generalize into the mod system + add capstones. Mods are *upfront BUILD picks*, not cards.
7. **Per-weapon upgrade trees:** model the 81 primary + 50 power **efficacy** upgrades (damage/
   fire-rate/size/count/radius/cooldown) as the card-draft pool (→ Phase PA). Stack costs via the
   existing `×13·1.6^owned` model.

**DoD:** each of the 11+11 weapons fires with its signature behavior (split/bounce/boomerang/
spool/airburst/pull/collapse/fan/telegraph/freeze/buff); an attuned Pulse Cannon deals its
element + triggers reactions; a mechanic-mod (e.g. Explode) applies; unit tests for split-gen
count, bounce count, boomerang return, airburst shrapnel count, attunement element resolution.

---

## Phase EN — Enemy Roster Expansion + Enabling Systems

**Goal:** 20 enemies with elements, plus the special mechanics (death-flare, trail hazard,
spawner, split-on-death, adaptive resist, support aura) and 4-edge wave integration.

**Depends:** E. **Port-from:** `enemy/enemy-data.js` (roster + element/resist tables),
`enemy/ai.js`, `enemy/movement.js`, `enemy/firing.js`, `enemy/support-aura.js`,
`world/hazard-field.js`, `enemy/shapes.js` (new silhouettes).

**dps-changes:** extend `EnemyKind`; new `systems/enemy/<kind>.rs` per new type; new
`systems/hazard.rs`, `systems/enemy/aura.rs`; new `render/shapes.rs` silhouettes.

**Steps**
1. **+10 `EnemyKind`s:** Cinder, Glacier, Frost Lance, Ashen Detonator, Tesla Wraith,
   Plaguebearer, Spore Carrier, Hydra, Warden, Lumen Drone. Stats/element/resist from
   `enemy-data.js:392-599` (note: enemies have **no per-type contact-damage** — contact is
   fixed player-side).
2. **Special mechanics (enabling systems):**
   - **deathFlare** (Ashen Detonator) — Pyro AoE r130/dmg12 on death.
   - **trailHazard** (Plaguebearer) — drop a `HazardField` (Toxic, r70, 6dps, 3.5s, every 600ms).
   - **spawner** (Spore Carrier) — birth a Wasp every 4000ms, cap 16.
   - **splitOnDeath** (Hydra) — 2 lings, ×0.5 HP / ×0.7 size, 1 gen.
   - **adaptive resist** (Warden) — bump the spammed element's resist post-hit (step 0.12, cap
     0.75), decay ×0.8 when you stop (`elements.js:114-133`).
   - **support aura** (Lumen Drone) — shield aura r180, 40% DR to allies, 300ms; lingers 400ms.
3. **Hazard field system** (`hazard-field.js`): circular zones ticking `dps×0.3` per 300ms onto
   the player inside, routed through the elemental damage path (so Toxic→corrode applies). Cap 24.
4. **Reuse existing movement/firing** where the new enemies share patterns (Cinder=wasp_zigzag,
   Glacier=square, etc.); only port genuinely new behaviors.
5. **Silhouettes:** add cinder_ember, ice_crystal, icicle_lance, cracked_bomb, arc_node,
   plague_sac, prism_facet (`shapes.js:329-419`). Element-tint the existing 10.
6. **Wave integration:** the 30-wave table already exists; map the new types into the stage
   roster (each stage introduces a type per `wave-data.js`). Spore/Hydra/aura spawns honor the
   spawn cap.

**DoD:** all 20 enemies spawn with correct element/resist; Plaguebearer leaves an acid trail
that corrodes; Spore Carrier births Wasps (capped); Hydra splits; Warden's resist climbs to the
element you spam and decays; killing a Lumen Drone drops its escort's shield. Unit tests for
adaptive-resist bump/decay, hazard tick, split-gen, spawner cap.

---

## Phase AB — 4-Slot Ability Loadout

**Goal:** replace the 8 fixed keybind skills with a **4-slot loadout** of 14 selectable
abilities triggered by keys 1–4, with cooldowns shown in a HUD ability bar.

**Depends:** E (some abilities are elemental fields). **Port-from:** `combat/weapon-data.js`
(`ABILITIES` :1427, `ABILITY_UPGRADES`, `ABILITY_ATTUNEMENTS` :1165), `player/abilities.js`
(activate/tick), `player/player.js:180` (slot state).

**dps-changes:** new `components/loadout.rs` (`EquippedAbilities[4]`, `AbilityCooldowns[4]`);
rework `systems/skills.rs` → `systems/abilities.rs`; `systems/input.rs` (keys 1-4); `render/hud.rs`
(ability bar already partly exists).

**Steps**
1. `Ability` enum (14): Bulwark, FieldMedic, DeflectorOrbs, EmpPulse, SentryDrone, Blink,
   GravitySnare, Designator, SecondWind, ElementalInfusion, CryoField, StasisField, StormCell,
   PyreAura. Each: `cooldown_ms`, `duration_ms`, effect. (Map dps's existing dash/shield-burst/
   bomb/EMP/bulwark/repair/deflector/tractor onto the new roster; cut Tractor Shield.)
2. **Slot state:** `EquippedAbilities([Option<Ability>;4])`, `cooldowns([f32;4])`,
   `cooldowns_max([f32;4])`. Keys **1/2/3/4** (Digit+Numpad, no repeat) → `activate_slot(i)`.
   (`input-handler.js:203-218`, `player.js:1076-1084`)
3. **New ability effects** not yet in dps: Blink (teleport 220px + 350ms i-frame), Gravity Snare
   (pull r320), Designator (mark all in r360 for 6s), Second Wind (one-time death save),
   Elemental Infusion (cycle override element 8s), and the 4 drop-zone fields (Cryo/Stasis/Storm/
   Pyre — tick element status in a radius). Existing dash stays as a Shift primitive.
4. **Ability attunements** (radio, one element per ability) — stamp the element on affected
   enemies at the ability's verb (`abilities.js`). Defer until BUILD tree (Phase UI).
5. **HUD ability bar:** 4 slots above the weapon squares, keybind label, bottom-up cooldown fill
   (`drawAbilitySlotBar` status.js:1774). dps's HUD already has an energy sphere/triforce cluster
   to slot this next to.

**DoD:** player equips up to 4 abilities; 1–4 trigger them with independent cooldowns shown in
the HUD; drop-zone fields apply the right status; Blink i-frames work. Unit tests for slot
activation gating (empty slot, on-cooldown, GUNSLINGER passive disables all).

---

## Phase PA — Passives (44) + Card Draft Restructure

**Goal:** the 44-passive rule-modifier layer (keystones + modular) with a 5-slot equip + ≤2
keystone budget; and the efficacy-card draft that delivers weapon/ability upgrades.

**Depends:** W, AB. **Port-from:** `combat/passive-data.js` (44 entries), `player/passives.js`
(aggregation), `combat/card-draft.js`, plus the STAT boon pool (`weapon-data.js:1659`).

**dps-changes:** new `combat/passive_data.rs` + `systems/passives.rs` rework; rework
`systems/survivor.rs` → `systems/card_draft.rs`; new `PassiveLoadout` resource.

**Steps**
1. **Passive registry** (44): port `PASSIVES` with `mods`/`damageMult`/`maxHpMult` + stacking
   rule (`binary` vs `additive`) + delivery channel (slot/item/keystone). dps already has ~13
   effects (Vampirism/Thorns/Dodge/Crit/Executioner/Momentum/Whirlwind/StaticDischarge/…) —
   fold them in and add the ~30 missing, esp. the **17 keystones** (GlassCannon, Berserker,
   Gunslinger, Purist, TwinCast, PrismaticSoul, OverflowCapacitor, KillingSpree, OneWithTheVoid,
   SecondHeart, EyeOfTheStorm, Detonator, Frenzy, GravityWell, FlowState, Failsafe, HeatSink).
2. **Aggregation** (`passives.js:72-98`): `passive_damage_mult` (product), `passive_maxhp_mult`
   (product), `passive_mod(key)` (sum). Combat reads these live.
3. **Slot model:** `MAX_PASSIVE_SLOTS=5`, usable `= 3 + floor(stages/30)`, keystone budget ≤2.
4. **Card draft** (`card-draft.js`): every 2nd stage clear (stages 2,4,6,8,10), offer **1 primary
   + 1 power + 2 ability** efficacy upgrades (relevance-filtered to OWNED gear, mechanic-mods
   excluded, not-at-max). Replace dps's survivor-card pool with this. Keep a **STAT card** pool
   (8 boons) + keystone picks as separate channels.

**DoD:** keystones change the build (Glass Cannon +60%dmg/−50%HP; Purist no-crit +pierce); card
draft offers only relevant, non-maxed efficacy upgrades; passive aggregation is unit-tested.

---

## Phase IT — Item System v2 (8-tier, 15 affixes, stash, cores)

**Goal:** the 8-tier rarity ladder with 15 affixes (incl. elemental resists), no auto-equip,
a persistent stash, and the cores salvage/craft currency.

**Depends:** E (resist affixes), ME (persistence — can be built in parallel, equip needs ME).
**Port-from:** `world/item-names.js` (8 tiers, 15 affixes, slots, naming), `world/item-system.js`
(roll/score/reroll/tier-up + item-passives), `world/inventory.js`, `world/cores.js`.

**dps-changes:** rework `systems/items.rs` (already has the scaffold — extend, don't rewrite).

**Steps**
1. **8 rarities** (replace 3): common/rare/exceptional/legendary/epic/godlike/divine/transcendental
   with weights, mult bands, affix counts (1→5), colors, glow, adjectives, `prismatic`
   (`item-names.js:143-191`). `rollRarity(bossBias)` with `BOSS_BIAS_K=6`.
2. **15 affixes** (add 6 resist affixes pyro/cryo/volt/toxic/void/radiant to the existing 9);
   value = `(base + (wave−1)·perWave)·rarityMult`, rounding rules (`item-names.js:63-85`).
3. **No auto-equip** (R8.2): drops go to the loot feed + a **persistent stash** (cap 200, keep
   highest-value tail), not auto-equipped. Equipping is a deliberate ARMORY action (Phase ME/UI).
4. **Item passives** (P7): Exceptional+ items can carry a discrete rule-modifier `item.passive`
   (chance `0.15 + 0.06·(rank−exceptionalRank)`), keystone only on Transcendental.
5. **Cores** (`cores.js`): salvage value `rank·affixCount·(1+level·0.1) + traits·3`; sinks =
   reroll `max(2, rank·3)`, tier-up `(rank+1)·12`. Bulk-salvage below-best-equipped.
6. **Scoring/upgrade**: keep `score_item` (effective-HP weights) for sorting; `nextRarity` walks
   the 8-tier ladder for tier-up.

**DoD:** drops roll across all 8 tiers (boss-bias shifts upward); resist affixes appear on
Exceptional+; items land in the stash (not equipped); salvaging yields cores; reroll/tier-up
spend cores correctly. Unit tests for rarity weights, affix scaling, salvage value, tier-up
affix carry-over.

---

## Phase ME — Meta-Progression, Persistence & Economy Split

**Goal:** the roguelite spine — persistent account level/SP/gold/cores/stash/unlocks across
runs, the run-gold↔account-gold↔cores economy, armory unlocks, and pre-run loadout selection.

**Depends:** IT. **Port-from:** `player/progression.js` (level/SP/gold-find), `core/sp-stats.js`
(8 stats), `core/storage.js` (3 localStorage keys), `shop/armory.js` (unlocks + base loadout),
`world/run-shop.js` (reroll/repair/revive sinks), `world/gold-*.js`, `game-engine.js` (banking).

**dps-changes:** new `meta/` module (`save.rs`, `progression.rs`, `armory.rs`); rework economy
in `systems/drops.rs` + `Score`; new `Meta`/`RunGold`/`Cores` resources; serde + ron to OS config dir.

**Steps**
1. **Persistence** (serde+ron, OS config dir via `dirs`): three documents mirroring the JS keys —
   **Meta** (`level, xp, sp, spStats, accountGold, cores, powerups, equippedItems, stash[],
   unlocked*[], loadout`), **RunSave** (wave-start checkpoint for Continue), **Settings**
   (volumes, selectedSkin). Load on boot, save on run end + meaningful changes.
2. **Account level + SP** (`progression.js`, `sp-stats.js`): XP per kill (boss 120 / regular 12),
   `xpForLevel = 500 + (L−1)·250`, max 100, +1 SP/level. **8 SP stats** (Health/Toughness/
   Vampirism/Thorns/CritChance/CritDamage/Dodge/Speed), each cap 20 points = 1 SP each, freely
   refundable. SP feeds the effective-stat getters (stacking with item affixes + powerups).
   **In-run leveling is removed** (no-op) — meta level only grants SP.
3. **Economy split:** `RunGold` (starts 0, accrues from pickups, spends in-run, **banked** to
   account-gold at run end) vs `accountGold` (persistent wallet, spends in ARMORY) vs `cores`
   (item crafting). Gold is **pickup-only** (drop kill-streak coin bonus). Gold-find `1 +
   max(0,wave−1)·0.10` (× HOARDERS_GREED 2). Jewels 15% @ ×3; bronze/silver/gold/platinum tiers.
4. **Armory unlocks** (`armory.js`): `BASE_LOADOUT` free (Pulse / Charge / Bulwark+FieldMedic /
   Opportunist+LastBastion); 7 unlock categories with flat account-gold costs (primaries 8000,
   powers 10000, abilities 12000, attunements 7000, mods 5000, abilityAttunements 6000, passives
   9000). Per-run loadout ≤4 per category, locked at run start.
5. **Pre-run loadout selection** + **run banking** at GameOver/GameComplete (`bankRunGold`,
   commit run loot → stash, save profile).
6. **In-run gold sinks** (`run-shop.js`): paid reroll (200), extra card ([600,1200] cap 2),
   repair kit (`250·(n+1)`, +35% HP), revive token (3000, 1/run).

**DoD:** account level/SP/gold/cores/stash/unlocks survive a quit-and-relaunch; a new run starts
from the equipped loadout + meta stats; run-gold banks into account-gold at run end; SP allocation
changes effective stats; armory unlocks gate the loadout pool. Unit tests for XP curve, SP
effective-stat math, gold-find scaling, banking idempotency, save round-trip.

---

## Phase PU — Powerup Catalog

**Goal:** the ~25-entry powerup catalog (permanent, stacking) acquired as world drops / cards /
gold purchases.

**Depends:** PA (some overlap effects), ME (gold cost). **Port-from:** `world/powerup.js`
(`POWERUP_TYPES`, `powerupGoldCost`).

**dps-changes:** new `combat/powerup_data.rs`; extend `systems/powerups.rs`.

**Steps**
1. Port `POWERUP_TYPES`: ~25 active entries (CritChance, CritDamage, Knockback, Executioner,
   Momentum, OverchargeRounds, StaticDischarge, Whirlwind, HealthBoost, ShieldBoost, Regen,
   PhaseEcho, Vampirism, Thorns, Guardian, + the health-economy set: Triage/LuckyDrops/
   FieldRations/TriageSurge/CombatMedic/SalvagePlating/TriageNet/AdrenalReserve/FieldSurgeon/
   BloodBank). Each: category, maxStacks, rarity-weight, per-stack effect.
2. **Permanent + stacking** (the `duration` field is dead; `addPowerup` forces permanent).
   Many effects already exist in dps's `Upgrades`/`passives` — re-home them here.
3. **Acquisition:** world-drop (weighted by rarity, 25s life, magnet) + survivor-card pick +
   gold purchase (`powerupGoldCost`). Reconcile with the dps decision (#3 below) on acquisition model.

**DoD:** powerups drop/pick/buy and stack permanently; effects apply live; world powerups blink+
magnet+expire. Unit test the weighted world-drop pick + gold-cost formula.

---

## Phase UI — BUILD Tree, Overlays, HUD, Input, Skins  *(LARGE)*

**Goal:** the full out-of-combat meta UI (BUILD tree + overlays), the in-combat HUD updates,
the radial menu, the expanded input map, and 12 cosmetic ship skins.

**Depends:** AB, PA, IT, ME, PU. **Port-from:** `shop/shop-dom.js` (BUILD tree),
`ui/{loadout,inventory,hangar,armory,stats,sp-allocation,settings}-overlay.js`, `ui/radial-menu.js`,
`hud/{status,combat,status-icons,item-feed,navigation}.js`, `ui/{input,gamepad}-handler.js`,
`player/skins/*`.

**dps-changes:** new states (`Armory`, `Hangar`, `Inventory`, `Stats`, `Loadout`/`Build`); new
`render/build_tree.rs`, `render/overlays/*.rs`; extend `systems/input.rs`; new `render/skins.rs`.

**Steps**
1. **States:** add `Armory/Build`, `Hangar`, `Inventory`, `Stats` (pause-style overlays). Title
   menu: NEW GAME → BUILD → START; CONTINUE (saved run); HANGAR; SETTINGS.
2. **BUILD tree** (`shop-dom.js`): a Diablo-style node tree — parent node per weapon/ability with
   upgrade/attunement/mod nodes orbiting; tabs GEAR · PRIMARY · POWER · DEFENSE · PASSIVE. Pre-run
   = equip/attune/mod/pick-passive + START RUN (≥1 primary); in-run shop = buy/sell upgrades.
   This is the single biggest UI piece. In Bevy: a custom `Node` graph layout or `bevy_egui`
   (decide — see #4 below).
3. **Overlays:** Inventory (I key — 5 gear slots + loot feed), Stats+SP-allocation (` key — level/
   XP/SP rows + allocation card), Hangar (skin select), Armory (unlock + equip + cores craft),
   Loadout (legacy 4+4+4 picker, optional if BUILD covers it), Settings.
4. **Radial menu** (`radial-menu.js`): hold F/E/R → primary/power/ability switcher (mouse-angle
   slice, click commits, release cancels). Gamepad R1/L1/Triangle.
5. **HUD updates:** 4-slot ability bar (Phase AB), target-info readout (enemy LV + name + HP),
   per-enemy status badges for all 8 statuses (dps has burn/stun), powerup indicator column,
   gold rolling counter, XP/level bar, +SP pip. (Minimap/energy-sphere/triforce already exist.)
6. **Input map:** keep twin-stick; add **1-4** (ability slots), **Shift** (dash), **F/E/R** (radials),
   **I** (inventory), **`** (stats), **Esc** (close stats→inventory→pause). Gamepad mirror.
7. **12 ship skins** (`player/skins/*`): cosmetic-only paint functions over a shared collision
   radius. Port the silhouettes (aurora default + 11). Persist `selectedSkin` in Settings.

**DoD:** the player can, from the title, open BUILD, equip a loadout (weapons/abilities/attunements/
mods/passives + gear), START a run, mid-run open Inventory/Stats and the radial switcher, and
pick a skin in the Hangar. Every overlay reads/writes the right meta/run state.

> **Decision (UI tech):** Bevy UI (`Node`) is verbose for a node-graph tree; `bevy_egui` would
> speed the overlays + BUILD tree dramatically. Flag for the user (see §5).

---

## Phase BO — Boss Chassis (+ scripted bosses = forward-looking)

**Goal:** port the declarative boss chassis (phase scripts / weak-point parts / intro-death
sequences) + the boss healthbar UI. Keep TITAN-tier bosses live (as rainboids does). The 10
scripted bosses (D.B1–B5) follow rainboids' own progress.

**Depends:** E, EN. **Port-from:** `enemy/boss-phases.js`, `enemy/boss-parts.js`,
`enemy/boss-intro.js` (all shipped + unit-tested in JS), `enemy/boss-rage.js` (live).

**dps-changes:** new `systems/enemy/boss_chassis.rs` (3 pure runners) + boss healthbar in
`render/hud.rs`.

**Steps**
1. **Phase-script runner** (`boss-phases.js`): ordered `phaseScript`, gate on descending HP
   fraction / custom predicate, `onEnter` once-per-phase in order, transition invuln (1000ms).
2. **Weak-point parts** (`boss-parts.js`): ≤6 destructible parts (static/rotate/orbit), **core
   invuln while shielding parts live**; bullet→part hit routing in the collision path.
3. **Intro/death sequences** (`boss-intro.js`): time-gated beat runner; intro (warp-in → name
   card → fight-start, `introBlocksDamage`) + death (detonation → supernova → victory).
4. **Boss healthbar UI:** always-visible name + segmented HP + phase pips + element.
5. Keep dps's existing TITAN tier + rage as the live boss system (matches v6.161). Scripted
   bosses (Harbinger/Aegis/Lumen/Gemini/Maelstrom/Hivemother/Iron Throne/Warden Prime/Nullmaw/
   Prismarch) = **forward-looking**, build once rainboids ships them (D.B1+).

**DoD:** the 3 chassis runners are ported + unit-tested (phase ordering/invuln, core-invuln-while-
parts-live, beat ordering); a stub scripted boss reaches every phase and is killable; the
healthbar renders. Live play unchanged (TITAN bosses).

---

## Phase X — Run Configurator + Adaptive Difficulty  *(FORWARD-LOOKING — not shipped in rainboids)*

**Goal:** replace the static 30-wave table with a player-chosen run length + auto-tuned
difficulty + procedural wave composition. **Only attempt after rainboids ships Phase X**, or as
an explicit user-directed effort.

**Port-from (design only):** `docs/Passive Skills & Run Difficulty — Design Plan` §12,
`docs/Balance Model — *`.

**Steps (when unblocked):** `runConfig = {stages, wavesPerStage}` replacing `MAX_WAVES`; powerup-
card pool layering (efficacy → economy fallback); reward dial; an Adaptive Difficulty Director
(reads dmg-taken/time-to-clear/DPS/near-death → challenge index vs target band, rate-limited
knobs); a procedural wave composer (threat budget + themes + telegraphed modifiers); RUN SETUP UI.

**DoD:** a 10×3 and a 100×9 run both run start→finish; the Director holds a target HP-band; waves
are fresh each run.

---

## Phase POL — VFX, Audio & Balance Parity

**Goal:** close the graphical + audio + balance gaps so a played run *looks and feels* like v6.161.

**Steps**
1. **Per-element weapon VFX** (`weapon-effects-renderer.js`): element-tinted bullets/beams +
   reaction FX (shatter burst, oil flare, conduct arcs, void pull, purge flash) + the new-weapon
   visuals (boomerang trail, ricochet, flak airburst, gravity lance pull, singularity, prism fan,
   orbital telegraph, cryo burst, overdrive aura).
2. **Status visuals** (`status-icons.js`): auras/badges for all 8 statuses (dps has burn/stun) +
   particles.
3. **Boss cinematics:** intro name-card, multi-stage death detonation, camera work (dps has
   shake/flash/hitstop primitives to drive from beats).
4. **Hazard / aura rendering:** acid pools, ally shield bubbles.
5. **Audio:** map new weapons/abilities/statuses/reactions to SFX (dps's `audio.rs` does
   name-lookup with fallback — add the new event names). Music already streams locally.
6. **Balance pass:** with elements/resist/meta-power in place, co-tune enemy/boss scaling around
   permanent meta power (R-BAL1 + the Balance Model doc). **Needs playtest** — gate on it.
7. **Timestep (optional, risky):** the JS is fixed 60 Hz px/tick; dps is dt-scaled. The new
   balance math assumes the JS speeds — aligning the timestep makes balance faithful but retunes
   *every* speed. Only with playtest feedback (long-standing known divergence).

**DoD:** a recorded run visually matches v6.161 (elemental colors, reactions, boss cinematic);
the balance feels intentional on a meta-progressed account.

---

## 4. Suggested milestones (playable checkpoints)

1. **M1 — Elemental combat** (E + W partial): elements + resist + reactions live; 11 primaries
   fire with elements. *Feels like the new combat even before the meta layer.*
2. **M2 — Full arsenal & roster** (W + EN + AB): all weapons/abilities/20 enemies/attunements.
3. **M3 — Roguelite loop** (IT + ME + PA + PU): persistence, account level/SP/gold/cores, stash,
   armory, card draft, passives, powerups — the full TITLE→BUILD→run→bank loop.
4. **M4 — Full UI** (UI): BUILD tree + overlays + radial + skins.
5. **M5 — Bosses & polish** (BO + POL): chassis + VFX/audio/balance parity.
6. **M6 — Endgame** (X): run configurator + adaptive difficulty *(when rainboids ships it)*.

---

## 5. Decisions to confirm before/while building (flag, don't silently pick)

1. **Scope/ambition** — port-to-parity (all of E–UI + BO chassis) is a very large effort. Confirm
   the intent is the *full* roguelite, vs. a subset (e.g. just elements + arsenal, M1–M2).
2. **UI tech** — adopt `bevy_egui` for the BUILD tree + overlays (much faster than Bevy `Node`
   for node-graphs and data tables), or stay pure Bevy UI? Affects Phase UI heavily.
3. **Persistence location/format** — serde+ron in the OS config dir (`dirs` crate) is the natural
   Rust analog of localStorage. Confirm.
4. **Powerup acquisition model** — JS allows world-drop + card + gold-buy. dps currently world-
   drops gems. Confirm the mix (this was the long-standing open §5 decision).
5. **Timestep** — keep dt-scaled (current, "simplified-faithful") or align to fixed 60 Hz px/tick
   for balance fidelity? Risky; playtest-gated.
6. **Phase X / scripted bosses** — these aren't shipped in rainboids. Build them speculatively, or
   wait and track rainboids? (Recommendation: wait; keep TITAN-tier bosses + static waves.)
7. **Seeded vs unseeded RNG** — dps seeds `GameRng` for test determinism; JS is unseeded. Keep
   seeded (assert ranges, not sequences).

---

## 6. Notes / carry-forward

- **Engine gotchas** (from the old plan, still apply): FixedUpdate `.chain()` 20-tuple limit →
  nest; B0001 query conflicts → disjoint filters + boot-into-Playing check; `Entity::to_bits()`;
  Bevy needs `mp3`+`wav` features; assets resolve exe-relative.
- **Discipline:** one green increment per commit to local `master` (push blocked); headless tests
  + boot check; **never `cargo fmt`** (manual alignment — see `no-cargo-fmt` memory).
- **Doc drift in JS:** `item-system.js`/`powerup.js` *headers* describe retired 3-tier/SP-era
  designs — trust `item-names.js` (8 tiers) + the live code, not the comments.
- The old `plans.md` + `docs/exact-port-spec.md` remain useful for the **engine/render/Phase-1-4
  primitives** that carry over unchanged; this document supersedes them on scope and on any
  v6.161 number.
```
