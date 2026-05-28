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

use crate::combat::element::ElementSet;
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

/// The player collected an orb or powerup this frame (emitted by `drops::collect_orbs`
/// / `powerups::collect_powerups`). Drives the pickup chime (`audio::play_pickup`),
/// which collapses a same-frame cluster + throttles. Carries nothing.
#[derive(Message, Debug, Clone, Copy)]
pub struct Pickup;

/// A resolved elemental reaction (E4b), emitted by `systems::reactions` — drives
/// a one-shot expanding-ring shockwave VFX (`render::reaction_fx`). Pure
/// presentation signal; the gameplay AoE is already applied via `Damage`.
#[derive(Message, Debug, Clone, Copy)]
pub struct Reaction {
    pub center: Vec2,
    pub kind: ReactionFx,
}

/// Which reaction fired — selects the shockwave's color + radius.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReactionFx {
    /// CRYO shatter — an icy cyan burst.
    Shatter,
    /// OIL flare — a fiery orange burst.
    Flare,
}

/// Shove `target` by `impulse` world-units (the `_KNOCK` bullet trait, spec
/// III.2/III.6 — a flat 16 px positional shove). Applied by
/// `collision::apply_knockback` so the producer needn't hold a mutable handle.
#[derive(Message, Debug, Clone, Copy)]
pub struct Knockback {
    pub target: Entity,
    pub impulse: Vec2,
}

/// Spawn a mid-flight player **shard** bullet — the reusable path for Mitosis
/// (split-on-impact) and Flak (airburst shrapnel), which can't spawn bullets
/// directly (the visual needs `BulletAssets`). `gen > 0` means the shard can
/// split again (Mitosis). Consumed by `systems::weapons::spawn_shards`.
#[derive(Message, Debug, Clone, Copy)]
pub struct Shard {
    pub origin: Vec2,
    pub dir: Vec2,
    pub speed: f32,
    pub damage: f32,
    pub element: ElementSet,
    /// Remaining split generations (Mitosis); 0 = a terminal shard.
    pub generation: u32,
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
    /// The attack's element (E8). Enemy fire carries its kind's element
    /// (`element_for`) so `spawn_bullets` tags the bullet + `enemy_bullet_hits_
    /// player` applies the matching status / resist. Player Fire leaves this
    /// `Kinetic` — player bullet elements resolve from attunement/weapon instead.
    pub element: crate::combat::element::Element,
}
