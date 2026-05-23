//! Enemy components. Phase 1 implements only `Drifter`; the other nine kinds
//! (movement + firing patterns) are ported in Phase 3 from
//! `js/modules/enemy/*`.

use bevy::prelude::*;

/// The ten enemy archetypes from the JS game (`enemy-data.js` `ENEMY_TYPES`).
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
