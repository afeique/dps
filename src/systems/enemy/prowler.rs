//! Prowler AI — an **orbit-strafer**: it holds a long preferred range like a
//! standoff artillery unit, but instead of sitting still it *circles* the Core
//! on that ring (thrusting along the tangent) while its missiles fire, so it
//! reads as a prowling predator rather than a parked turret. A radius spring
//! keeps it on the ring; the tangential thrust walks it around. Port of
//! rainboids' `keep_distance` + circler behaviour, retargeted onto the Core.
//!
//! `AiState.wander.x` stores the orbit direction (±1), seeded per-enemy so the
//! pack splits clockwise/counter-clockwise instead of all circling one way.

use crate::components::*;
use crate::systems::steering::approach;
use bevy::prelude::*;

const PREFERRED: f32 = 330.0;
const MAX_SPEED: f32 = 130.0;
const ACCEL: f32 = 6.0;
/// How hard the radius spring corrects drift back onto the ring (u/s per u error).
const RADIUS_SPRING: f32 = 1.4;

pub fn ai(
    time: Res<Time>,
    core: Query<&Transform, (With<Core>, Without<Enemy>)>,
    mut enemies: Query<(&Transform, &mut Velocity, &mut AiState, &Enemy), With<Enemy>>,
) {
    let Ok(core_tf) = core.single() else {
        return;
    };
    let core_pos = core_tf.translation.truncate();
    let _ = time;

    for (tf, mut vel, mut state, enemy) in &mut enemies {
        if enemy.kind != EnemyKind::Prowler
            && enemy.kind != EnemyKind::SporeCarrier
            && enemy.kind != EnemyKind::Warden
        {
            continue;
        }
        let pos = tf.translation.truncate();
        let to_core = core_pos - pos;
        let dist = to_core.length();
        let radial = if dist > 1.0 { to_core / dist } else { Vec2::X };
        let tangent = Vec2::new(-radial.y, radial.x);

        // Seed a stable orbit direction once (±1) from a position hash.
        if state.wander.x == 0.0 {
            let h = ((pos.x * 0.13 + pos.y * 0.17).sin() * 43758.5).fract();
            state.wander.x = if h < 0.5 { 1.0 } else { -1.0 };
        }

        // Radius spring (pull toward the ring) + tangential orbit thrust.
        let radius_err = (dist - PREFERRED).clamp(-MAX_SPEED, MAX_SPEED);
        let desired = (radial * radius_err * RADIUS_SPRING
            + tangent * state.wander.x * MAX_SPEED)
            .clamp_length_max(MAX_SPEED);
        vel.0 = approach(vel.0, desired, ACCEL);
    }
}
