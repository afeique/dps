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
}
