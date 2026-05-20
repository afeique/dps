//! Enemy AI. Phase 1: the `Drifter` only — it drifts and bounces off the play
//! edge. The richer per-kind steering and firing patterns for all ten enemies
//! are ported in Phase 3 from `js/modules/enemy/*` (each kind becomes its own
//! system, dispatched by `EnemyKind`).

use crate::components::{Enemy, EnemyKind, Velocity};
use crate::resources::PlayBounds;
use bevy::prelude::*;

pub fn drifter_ai(bounds: Res<PlayBounds>, mut q: Query<(&Enemy, &mut Velocity, &Transform)>) {
    for (enemy, mut vel, tf) in &mut q {
        if enemy.kind != EnemyKind::Drifter {
            continue;
        }
        // Bounce inward at the play-area edges (velocity points away from edge).
        if tf.translation.x.abs() > bounds.half.x {
            vel.0.x = -vel.0.x.abs() * tf.translation.x.signum();
        }
        if tf.translation.y.abs() > bounds.half.y {
            vel.0.y = -vel.0.y.abs() * tf.translation.y.signum();
        }
    }
}
