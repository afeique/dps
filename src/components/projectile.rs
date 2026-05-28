//! Projectile components (bullets now; mines/missiles in Phase 3).

use crate::combat::element::ElementSet;
use bevy::prelude::*;

/// The element(s) a player bullet carries (resolved from the weapon's base
/// element + equipped attunements — W1; for now every player shot is the
/// `Kinetic` base). Read by `collision::bullet_hits_enemy` to apply the target's
/// `Resistances` multiplier. A bullet **without** this component is treated as
/// neutral (full damage) — kept a separate component (not a `Bullet` field) so
/// the existing `Bullet {…}` literals + tests are unchanged.
#[derive(Component, Debug, Clone, Copy)]
pub struct BulletElements(pub ElementSet);

/// A **Gravity Lance** orb: while it flies it drags enemies within `pull_radius`
/// toward it at `pull_strength` px/s (`systems::weapons::gravity_pull`).
#[derive(Component, Debug, Clone, Copy)]
pub struct GravityBullet {
    pub pull_radius: f32,
    pub pull_strength: f32,
}

/// A **Boomerang** disc: flies out for `timer` s, then `returning` flips and it
/// accelerates back toward the player (`systems::weapons::boomerang_return`).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Boomerang {
    pub timer: f32,
    pub returning: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulletKind {
    Player,
    Enemy,
}

#[derive(Component, Debug)]
pub struct Bullet {
    pub kind: BulletKind,
    pub damage: f32,
    /// Remaining extra targets this bullet can pass through. 0 = dies on the
    /// first hit; Rail Driver uses 99 (effectively infinite). Decremented per
    /// enemy hit in `collision::bullet_hits_enemy`.
    pub pierce: u32,
}

/// Marks an enemy bullet fired by a *raged* boss (spec IV.7 `enableHomingBullets`).
/// `enemy::rage_homing_steer` curves it toward the player at `turn_rate` rad/sec,
/// preserving speed — the bounded equivalent of the JS per-tick `vel += dir*0.04`
/// nudge (spec IV.5).
#[derive(Component, Debug)]
pub struct RageHoming {
    pub turn_rate: f32,
}
