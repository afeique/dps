//! Game events used to decouple systems.
//!
//! Bevy 0.18 terminology: **buffered, broadcast events are `Message`s**
//! (`MessageWriter` / `MessageReader`, `app.add_message::<T>()`). The `Event`
//! trait is now reserved for *observer*-style targeted events. These
//! domain events are buffered pub/sub, so they derive `Message`.
//!
//! Flow this enables (all within one `FixedUpdate` run, ordered by `.chain()`):
//!   player_fire → `Fire` → spawn_bullets
//!   bullet_hits_enemy → `Damage` → apply_damage → `Death` → (drops/score, Phase 3)

use bevy::prelude::*;

/// Broadphase/narrowphase output: entity `a` overlapped entity `b` this tick.
#[derive(Message, Debug, Clone, Copy)]
pub struct Collision {
    pub a: Entity,
    pub b: Entity,
}

/// Apply `amount` HP of damage to `target`. Consumed by `systems::damage`.
#[derive(Message, Debug, Clone, Copy)]
pub struct Damage {
    pub target: Entity,
    pub amount: f32,
}

/// `entity` reached 0 HP. Drives drops, score, and death FX (Phase 3+).
#[derive(Message, Debug, Clone, Copy)]
pub struct Death {
    pub entity: Entity,
}

/// A weapon fired: spawn a bullet from `origin` along unit `dir`.
#[derive(Message, Debug, Clone, Copy)]
pub struct Fire {
    pub origin: Vec2,
    pub dir: Vec2,
    pub damage: f32,
    pub speed: f32,
}
