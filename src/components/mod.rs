//! ECS components. Domain-specific ones live in submodules; the truly shared
//! primitives (used across player / enemy / projectile) live here.

pub mod enemy;
pub mod loadout;
pub mod player;
pub mod projectile;

pub use enemy::*;
pub use loadout::*;
pub use player::*;
pub use projectile::*;

use bevy::prelude::*;

/// Linear velocity (world units / second). Integrated by `systems::movement`.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct Velocity(pub Vec2);

/// Circular collider radius. Broad- and narrow-phase both use this.
#[derive(Component, Debug, Clone, Copy)]
pub struct Collider {
    pub radius: f32,
}

/// Hit points. Death is emitted (see `systems::damage`) when `current <= 0`.
#[derive(Component, Debug, Clone, Copy)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

impl Health {
    pub fn new(max: f32) -> Self {
        Self { current: max, max }
    }
}

/// How much equipped-item MAX-HP is currently baked into the player's `Health.max`
/// (spec VI.5). Lives on the ship entity so it resets with each fresh run; the
/// `systems::items::apply_item_hp` system reconciles `Health.max` against the
/// equipped HP affix total by applying the delta whenever gear changes.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ItemHpBonus(pub f32);

/// Which side an entity is on — gates which collision pairs matter.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Faction {
    Player,
    Enemy,
}

/// The tower-defense **objective**: a central station the player defends. Enemy
/// AIs navigate toward it (it replaces the player as their seek target) and ram
/// it; each contact subtracts from its `Health` (see `collision::enemy_contact_core`)
/// and the run ends (`GameOver`) when it is destroyed (`tower::core_lose_check`).
/// Exactly one exists during `Playing`; the commander ship is no longer the lose
/// condition. Inert to every other system (not an `Enemy`, not a `Ship`).
#[derive(Component, Debug, Clone, Copy)]
pub struct Core;

/// Auto-despawn after `seconds` elapse (bullets, transient FX).
#[derive(Component, Debug, Clone, Copy)]
pub struct Lifetime {
    pub seconds: f32,
}

/// Brief post-hit invulnerability (i-frames). While present, the entity ignores
/// incoming `Damage`; `systems::damage::tick_invulnerability` counts it down and
/// removes it at expiry. Stops rapid contact/fire from melting the player in a
/// single tick burst — mirrors the JS ship's hit-cooldown grace window.
#[derive(Component, Debug, Clone, Copy)]
pub struct Invulnerable {
    pub seconds: f32,
}
