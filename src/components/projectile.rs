//! Projectile components (bullets now; mines/missiles in Phase 3).

use bevy::prelude::*;

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
