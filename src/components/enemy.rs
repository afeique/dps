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
