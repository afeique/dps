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
