//! Enemy components. Phase 1 implements only `Drifter`; the other nine kinds
//! (movement + firing patterns) are ported in Phase 3 from
//! `js/modules/enemy/*`.

use bevy::prelude::*;

/// The enemy archetypes from the JS game (`enemy-data.js` `ENEMY_TYPES`). The
/// original ten plus the new elemental roster (Phase EN). New kinds reuse an
/// existing kind's movement + fire pattern (their AI filter / firing arm just
/// add the variant) until they get unique behavior/silhouettes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnemyKind {
    Drifter,
    Guardian,
    Wasp,
    Stalker,
    Prowler,
    Weaver,
    Sentinel,
    Tangerine,
    Titan,
    Hunter,
    // ── EN batch E8b — Pyro/Cryo elemental enemies ───────────────────────────
    /// Pyro swarmer (reuses Wasp's zigzag + machinegun).
    Cinder,
    /// Cryo tank (reuses Guardian's square + spread).
    Glacier,
    /// Cryo sniper (reuses Stalker's arc + charged laser).
    FrostLance,
    /// Pyro bomber (reuses Hunter's arc + burst); EN-followup adds its death-flare.
    AshenDetonator,
    // ── EN batch E8c — Volt/Toxic elemental enemies ──────────────────────────
    /// Volt skirmisher (reuses Hunter's arc + Drifter's arc-lightning fire).
    TeslaWraith,
    /// Toxic area-denier (reuses Tangerine's chase + mine); EN-followup adds its acid trail.
    Plaguebearer,
    /// Toxic drone spawner (reuses Prowler's keep-distance + Hunter's fire); EN-followup adds its spawner.
    SporeCarrier,
    /// Splitting bruiser (reuses Tangerine's chase + Hunter's fire); EN-followup adds its death-split.
    Hydra,
    // ── EN batch E8d/e — anti-meta + support ─────────────────────────────────
    /// Adaptive-resist wall (reuses Prowler's keep-distance + missile); EN-followup
    /// wires the [`crate::combat::element::Resistances`] adapt/decay (the helpers exist).
    Warden,
    /// Radiant support drone (reuses Prowler's keep-distance + Hunter's fire);
    /// EN-followup adds its ally-shield aura.
    LumenDrone,
}

#[derive(Component, Debug)]
pub struct Enemy {
    pub kind: EnemyKind,
}

/// Marks a boss-promoted enemy and carries its tier (1–4). Boss TITANs get
/// HP/size overlays per `enemy::boss_tier_mul` and (later) the rage mechanics
/// from `boss-rage.js`. Tier stats: T1 4.0×hp/1.35×sz, T2 5.0/1.45,
/// T3 6.0/1.55, T4 8.0/1.75 (port spec IV.7).
#[derive(Component, Debug, Clone, Copy)]
pub struct Boss {
    pub tier: u8,
}

/// Marks a boss that has crossed its HP-threshold rage (spec IV.7) — a one-shot
/// gate so `boss_rage` activates rage exactly once.
#[derive(Component, Debug)]
pub struct Raged;

/// A boss that has crossed its HP-threshold but is in the **rage telegraph**
/// window (spec IV.7, `TELEGRAPH_FRAMES = 24` ≈ 0.4 s): a red warning ring shows
/// before the rage burst fires, giving the player a counterplay beat.
/// `tick_rage_telegraph` counts `timer` down → `activate_rage`. The HP-threshold
/// path telegraphs; the tier-2 pair-link rages immediately (no telegraph).
#[derive(Component, Debug)]
pub struct RageTelegraph {
    pub timer: f32,
}

/// The red HDR warning-ring entity spawned for a rage telegraph (spec IV.7). A
/// top-level lyon ring with a `Lifetime == TELEGRAPH_SECS`; `render::telegraph_fx`
/// pulses its Transform scale over that window so it throbs as the rage charges.
#[derive(Component, Debug)]
pub struct TelegraphRing;

/// Marks an enemy currently bound into a generic formation (spec IV.6). Set by
/// `formations::create_formation`, cleared on release; lets other systems tell a
/// formation member apart (and gates re-binding).
#[derive(Component, Debug)]
pub struct FormationMember;

/// Marks a mid-wave **mini-boss** promotion (spec V.6) — a regular enemy buffed
/// to HP×1.7 / radius×1.25, awarding 2× points on death.
#[derive(Component, Debug)]
pub struct MiniBoss;

/// Per-entity movement-speed multiplier applied on top of an enemy's base
/// `stats().speed` (spec V.4 campaign curve × IV.7 boss-tier speed). Each AI
/// reads it; 1.0 = unscaled.
#[derive(Component, Debug, Clone, Copy)]
pub struct SpeedMul(pub f32);

/// A damage-over-time burn (Lance Beam, Nova Inferno — spec III.3/III.7).
/// `tick_burning` applies `dps × dt` each tick and removes it at `secs ≤ 0`.
#[derive(Component, Debug, Clone, Copy)]
pub struct Burning {
    pub dps: f32,
    pub secs: f32,
}

/// A stun: while present the enemy can't fire (Arc Lightning, Nova Lightning,
/// EMP, the `_STUN` bullet trait — spec III.3/III.6). `tick_stun` counts it down.
#[derive(Component, Debug, Clone, Copy)]
pub struct Stunned {
    pub secs: f32,
}

// ── Elemental statuses (Phase E3, from `combat-manager.js` STATUS_DEF :2018+) ──
// The signature on-hit statuses for the six non-Kinetic elements (Burning +
// Stunned above already cover PYRO + the stun verb). Each ticks down in
// `FixedUpdate` and removes itself on expiry (`status::tick_status_timers`);
// DoT statuses (Bleed) emit `Damage` like `Burning`. Their gameplay *effects*
// (chill slow, conduct/corrode amplify, void pull) are read by the damage/
// movement paths as those land — application on hit arrives with elemental
// damage sources (W attunements / E5 enemy elements).

/// CRYO **chill** (`CHILL`): the enemy moves at `CHILL_SLOW`× while present.
/// 2000 ms. Escalates to `Frozen` on a heavy/again CRYO hit.
#[derive(Component, Debug, Clone, Copy)]
pub struct Chill {
    pub secs: f32,
}

/// CRYO **freeze** (`FREEZE`): the enemy is halted + can't fire; a heavy hit
/// while frozen triggers SHATTER (E4). 1500 ms.
#[derive(Component, Debug, Clone, Copy)]
pub struct Frozen {
    pub secs: f32,
}

/// VOLT **conduct** ("wet", `CONDUCT`): incoming VOLT damage is amplified
/// ×`CONDUCT_VOLT_MULT` while present (applied in the damage path, E4). 3000 ms.
#[derive(Component, Debug, Clone, Copy)]
pub struct Conduct {
    pub secs: f32,
}

/// TOXIC **corrode** (`CORRODE`): a vulnerability — incoming damage from ALL
/// sources ×(1 + `CORRODE_PER_STACK`×stacks), `stacks` capped at
/// `CORRODE_MAX_STACKS`. 4000 ms (refreshed on re-application).
#[derive(Component, Debug, Clone, Copy)]
pub struct Corrode {
    pub stacks: u32,
    pub secs: f32,
}

/// VOID **mark** (`MARK`): tags the enemy for void pull / bonus effects. 6000 ms.
#[derive(Component, Debug, Clone, Copy)]
pub struct Mark {
    pub secs: f32,
}

/// **Oil** coat (`OIL`): flammable — a PYRO hit ignites it into a burn AoE
/// (oil-flare, E4). 5000 ms.
#[derive(Component, Debug, Clone, Copy)]
pub struct Oil {
    pub secs: f32,
}

/// TOXIC **bleed** (`BLEED`): a poison DoT (`dps × dt`), no refresh. 4000 ms.
/// Distinct from `Burning` (different source + stacking rules); shares the DoT
/// tick shape (`status::tick_bleed`).
#[derive(Component, Debug, Clone, Copy)]
pub struct Bleed {
    pub dps: f32,
    pub secs: f32,
}

/// Status durations in seconds (`STATUS_DEF`, combat-manager.js:2018+).
pub const CHILL_SECS: f32 = 2.0;
pub const FREEZE_SECS: f32 = 1.5;
pub const CONDUCT_SECS: f32 = 3.0;
pub const CORRODE_SECS: f32 = 4.0;
pub const MARK_SECS: f32 = 6.0;
pub const OIL_SECS: f32 = 5.0;
pub const BLEED_SECS: f32 = 4.0;

/// Movement-speed factor while chilled (combat-manager: ×0.7).
pub const CHILL_SLOW: f32 = 0.7;
/// Corrode stack cap (combat-manager: 3).
pub const CORRODE_MAX_STACKS: u32 = 3;
/// Per-stack incoming-damage amplification from corrode (+15%/stack).
pub const CORRODE_PER_STACK: f32 = 0.15;
/// VOLT-damage amplifier vs a conducting target (collision-system: ×1.5).
pub const CONDUCT_VOLT_MULT: f32 = 1.5;

/// Flat **armor** (`ENEMY_ARMOR`, enemy-data.js:655): subtracted from each
/// incoming hit down to a 25% floor (`max(dmg×0.25, dmg − armor)`), unless the
/// hit includes RADIANT (purge). Makes chip damage fall off; big / corrode-
/// amplified hits punch through. Guardian is the armored archetype (1.0).
#[derive(Component, Debug, Clone, Copy)]
pub struct Armor(pub f32);

/// **Frontal shield** (`ENEMY_FRONTAL_SHIELD`, enemy-data.js:663, Sentinel):
/// hits arriving within `arc/2` of the enemy→player bearing are reduced by
/// `reduction`; flank / bounced / returning shots land full. Bypassed by RADIANT
/// (purge). Sentinel: `arc` 2.4 rad (~137° cone), `reduction` 0.8 (80% blocked).
#[derive(Component, Debug, Clone, Copy)]
pub struct FrontalShield {
    pub arc: f32,
    pub reduction: f32,
}

/// Flat-armor damage floor (`applyDamageToEnemy`: 25% of a hit always lands).
pub const ARMOR_FLOOR: f32 = 0.25;

/// A **split-on-death** enemy (Hydra, `splitOnDeath`): on death it spawns `lings`
/// half-size copies. A spawned ling gets `lings = 0` so it can't split again
/// (maxGen 1). Read by `damage::apply_damage` at the death tick.
#[derive(Component, Debug, Clone, Copy)]
pub struct Splitter {
    pub lings: u32,
}

/// A **drone spawner** enemy (Spore Carrier, `spawner`): births a [`Drone`] Wasp
/// every [`SPORE_DRONE_INTERVAL`] s while under the [`SPORE_DRONE_CAP`] global
/// drone cap. `timer` counts down to the next birth. Run by
/// `enemy::mechanics::spore_spawner`.
#[derive(Component, Debug, Clone, Copy)]
pub struct DroneSpawner {
    pub timer: f32,
}

/// Marks an enemy spawned as a Spore Carrier drone (so the spawner can cap the
/// live drone count without counting wave-spawned Wasps).
#[derive(Component, Debug)]
pub struct Drone;

/// Seconds between Spore Carrier drone births (enemy-data.js: 4000 ms).
pub const SPORE_DRONE_INTERVAL: f32 = 4.0;
/// Max live Spore Carrier drones (enemy-data.js cap: 16).
pub const SPORE_DRONE_CAP: usize = 16;

/// A **support aura** enemy (Lumen Drone, support-aura.js `shield`): every
/// `interval` s it stamps an [`AllyShield`] on allies within `radius`. Kill it to
/// drop the escort's shield (it lingers [`AURA_LINGER`] s after the last pulse).
#[derive(Component, Debug, Clone, Copy)]
pub struct SupportAura {
    pub radius: f32,
    pub amount: f32,
    pub interval: f32,
    pub timer: f32,
}

/// A lingering ally damage-reduction buff from a [`SupportAura`]: incoming damage
/// ×(1 − `amount`) while present. Refreshed each aura pulse; `secs` counts down
/// so killing the support drops it shortly after.
#[derive(Component, Debug, Clone, Copy)]
pub struct AllyShield {
    pub secs: f32,
    pub amount: f32,
}

/// Lumen Drone aura (support-aura.js / enemy-data.js): r180, 40% DR, every 300 ms,
/// lingering 400 ms.
pub const AURA_RADIUS: f32 = 180.0;
pub const AURA_AMOUNT: f32 = 0.4;
pub const AURA_INTERVAL: f32 = 0.3;
pub const AURA_LINGER: f32 = 0.4;

/// Per-enemy AI scratch state (steering targets, phase timers). Filled out
/// per-kind in Phase 3; carried now so the component shape is stable.
#[derive(Component, Debug, Default)]
pub struct AiState {
    pub wander: Vec2,
    pub phase: f32,
}

/// Firing cadence for enemies that shoot (Phase 3).
#[derive(Component, Debug)]
pub struct FireCooldown {
    pub cooldown: f32,
    pub timer: f32,
}
