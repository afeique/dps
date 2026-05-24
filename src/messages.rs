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

use crate::components::{EnemyKind, Faction};

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

/// `entity` reached 0 HP at `position` (captured before despawn so death FX
/// can place an explosion there). Drives death FX + drops/score.
#[derive(Message, Debug, Clone, Copy)]
pub struct Death {
    pub entity: Entity,
    pub position: Vec2,
    /// The enemy kind that died, or `None` for the player. Lets `spawn_drops`
    /// compute the gold budget + point reward from the roster (spec VI.4/VI.5).
    pub kind: Option<EnemyKind>,
    /// Boss tier (0 = not a boss); bumps the gold drop-profile budget.
    pub boss_tier: u8,
    /// Was this a mid-wave mini-boss promotion (spec V.6)? → 2× points + the
    /// miniboss gold profile.
    pub mini_boss: bool,
}

/// The player actually took `amount` HP of damage this tick (post-shield,
/// non-dodged). Drives reactive effects that need the *landed* amount —
/// currently THORNS (`collision`/`damage::apply_thorns`); a clean hook for
/// RETALIATION / screen-shake later. Decouples those from the `Damage` reader.
#[derive(Message, Debug, Clone, Copy)]
pub struct PlayerHurt {
    pub amount: f32,
}

/// A player bullet landed a critical hit. Emitted by `collision::bullet_hits_enemy`
/// when `roll_crit` returns a >1× multiplier; consumed by the `precision` mission
/// (spec V.6) to count crits this wave.
#[derive(Message, Debug, Clone, Copy)]
pub struct Crit;

/// Shove `target` by `impulse` world-units (the `_KNOCK` bullet trait, spec
/// III.2/III.6 — a flat 16 px positional shove). Applied by
/// `collision::apply_knockback` so the producer needn't hold a mutable handle.
#[derive(Message, Debug, Clone, Copy)]
pub struct Knockback {
    pub target: Entity,
    pub impulse: Vec2,
}

/// A weapon fired: spawn a bullet from `origin` along unit `dir`. `faction`
/// selects the bullet's kind / team / color (player gold vs enemy magenta).
#[derive(Message, Debug, Clone, Copy)]
pub struct Fire {
    pub origin: Vec2,
    pub dir: Vec2,
    pub damage: f32,
    pub speed: f32,
    pub faction: Faction,
    /// When a *raged* boss fires, its bullets curve toward the player
    /// (spec IV.7 `enableHomingBullets` / IV.5 homing nudge). `spawn_bullets`
    /// tags such enemy bullets with `RageHoming`. Always `false` for player
    /// shots and normal (un-raged) enemy fire.
    pub homing: bool,
}
