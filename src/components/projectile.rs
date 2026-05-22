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
