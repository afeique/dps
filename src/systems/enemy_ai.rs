//! Enemy AI. Phase 1: the `Drifter` only — it drifts and bounces off the play
//! edge. The richer per-kind steering and firing patterns for all ten enemies
//! are ported in Phase 3 from `js/modules/enemy/*` (each kind becomes its own
//! system, dispatched by `EnemyKind`).

use crate::components::{Enemy, EnemyKind, Velocity};
use crate::resources::PlayBounds;
use bevy::prelude::*;

/// Gentle acceleration (u/s²) drawing drifters toward the Core at the origin, so
/// they menace the base instead of only bouncing around the arena. (The other
/// enemy kinds already steer toward the Core via their own AI.)
const DRIFTER_CORE_PULL: f32 = 90.0;

pub fn drifter_ai(
    time: Res<Time>,
    bounds: Res<PlayBounds>,
    mut q: Query<(&Enemy, &mut Velocity, &Transform)>,
) {
    let dt = time.delta_secs();
    for (enemy, mut vel, tf) in &mut q {
        if enemy.kind != EnemyKind::Drifter {
            continue;
        }
        // Tower-defense: steer toward the Core (origin). Without this, drifters
        // never reach the base and drifter-heavy waves stall, since pulse advance
        // waits for the field to thin out.
        vel.0 += -tf.translation.truncate().normalize_or_zero() * DRIFTER_CORE_PULL * dt;
        // Bounce inward at the play-area edges (velocity points away from edge).
        if tf.translation.x.abs() > bounds.half.x {
            vel.0.x = -vel.0.x.abs() * tf.translation.x.signum();
        }
        if tf.translation.y.abs() > bounds.half.y {
            vel.0.y = -vel.0.y.abs() * tf.translation.y.signum();
        }
    }
}
