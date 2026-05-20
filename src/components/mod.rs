//! ECS components. Domain-specific ones live in submodules; the truly shared
//! primitives (used across player / enemy / projectile) live here.

pub mod enemy;
pub mod player;
pub mod projectile;

pub use enemy::*;
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

/// Which side an entity is on — gates which collision pairs matter.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Faction {
    Player,
    Enemy,
}

/// Auto-despawn after `seconds` elapse (bullets, transient FX).
#[derive(Component, Debug, Clone, Copy)]
pub struct Lifetime {
    pub seconds: f32,
}
