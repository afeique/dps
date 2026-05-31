//! Wasp AI — a true **boid swarm**: each wasp flocks (separation + cohesion +
//! alignment) with the other wasps while weakly seeking the Core, producing the
//! living, erratic shoal that a lone-darting wasp never quite sold. Fast, twitchy,
//! and hard to predict because the swarm shifts as a body. Port of rainboids'
//! swarmer archetype (`separationWeight` flocking) retargeted onto the Core.
//!
//! Two-pass: snapshot every swarmer's `(pos, vel)` (immutable) so the boid math
//! sees a consistent frame, then steer each one (mutable). `AiState.wander` seeds
//! per-wasp wander jitter so the shoal shimmers instead of moving as one rigid blob.

use crate::components::*;
use crate::systems::steering::{alignment, approach, cohesion, seek, separation, wander};
use bevy::prelude::*;

const MAX_SPEED: f32 = 210.0;
const ACCEL: f32 = 10.0;
const NEIGHBOUR_RADIUS: f32 = 120.0;
const SEPARATION_RADIUS: f32 = 52.0;
// Boid weights: spread strongest, then a weak pull together + heading-match, and
// a gentle Core-seek so the whole shoal still drifts in to attack.
const W_SEPARATION: f32 = 1.6;
const W_COHESION: f32 = 0.5;
const W_ALIGNMENT: f32 = 0.35;
const W_SEEK: f32 = 0.55;
const W_WANDER: f32 = 0.4;

fn is_swarmer(kind: EnemyKind) -> bool {
    matches!(kind, EnemyKind::Wasp | EnemyKind::Cinder | EnemyKind::LumenDrone)
}

pub fn ai(
    time: Res<Time>,
    core: Query<&Transform, (With<Core>, Without<Enemy>)>,
    mut enemies: Query<(&Transform, &mut Velocity, &mut AiState, &Enemy), With<Enemy>>,
) {
    let Ok(core_tf) = core.single() else {
        return;
    };
    let core_pos = core_tf.translation.truncate();
    let t = time.elapsed_secs();

    // Pass 1 — snapshot the swarm so flocking sees a consistent frame.
    let flock: Vec<(Vec2, Vec2)> = enemies
        .iter()
        .filter(|(_, _, _, e)| is_swarmer(e.kind))
        .map(|(tf, vel, _, _)| (tf.translation.truncate(), vel.0))
        .collect();

    // Pass 2 — steer each swarmer by the boid combination.
    let mut idx = 0u32;
    for (tf, mut vel, mut state, enemy) in &mut enemies {
        if !is_swarmer(enemy.kind) {
            continue;
        }
        let pos = tf.translation.truncate();
        idx += 1;

        let sep = separation(pos, flock.iter().map(|(p, _)| *p), SEPARATION_RADIUS);
        let coh = cohesion(pos, flock.iter().map(|(p, _)| *p), NEIGHBOUR_RADIUS);
        let ali = alignment(pos, flock.iter().copied(), NEIGHBOUR_RADIUS);
        let seek_dir = (core_pos - pos).normalize_or_zero();
        let wnd = wander(idx, t, 1.0);

        // Combine the weighted steering urges into a desired velocity.
        let desired = ((sep * W_SEPARATION
            + coh * W_COHESION
            + ali * W_ALIGNMENT
            + seek_dir * W_SEEK
            + wnd * W_WANDER)
            .normalize_or_zero())
            * MAX_SPEED;
        vel.0 = approach(vel.0, desired, ACCEL);
        let _ = (seek, &mut state); // seek kept in the shared vocabulary
    }
}
