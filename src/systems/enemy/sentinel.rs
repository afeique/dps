//! Sentinel AI — a **kiter**: it holds a mid standoff from the Core and slowly
//! sidesteps along the ring while its firing arm sweeps, but the moment anything
//! (the Core's defenders / a closing front) pushes it inside its comfort radius
//! it *thrusts away* to re-open the gap before resuming its aim. Always turns to
//! face the Core so its sweep tracks. Port of rainboids' sniper/kite behaviour.

use crate::components::*;
use crate::systems::steering::{approach, arrive, flee};
use bevy::prelude::*;

const STANDOFF: f32 = 280.0;
const PANIC_RADIUS: f32 = 200.0; // inside this it kites away hard
const MAX_SPEED: f32 = 95.0;
const KITE_SPEED: f32 = 190.0; // faster while fleeing than while holding
const ACCEL: f32 = 6.0;
const STRAFE_SPEED: f32 = 45.0; // gentle lateral drift while holding station

pub fn ai(
    mut commands: Commands,
    time: Res<Time>,
    core: Query<&Transform, (With<Core>, Without<Enemy>)>,
    mut enemies: Query<(Entity, &Transform, &mut Velocity, &mut AiState, &Enemy), With<Enemy>>,
) {
    let Ok(core_tf) = core.single() else {
        return;
    };
    let core_pos = core_tf.translation.truncate();
    let t = time.elapsed_secs();

    for (e, tf, mut vel, mut state, enemy) in &mut enemies {
        if enemy.kind != EnemyKind::Sentinel {
            continue;
        }
        let pos = tf.translation.truncate();
        let to_core = core_pos - pos;
        let dist = to_core.length();
        let radial = if dist > 1.0 { to_core / dist } else { Vec2::X };
        let tangent = Vec2::new(-radial.y, radial.x);

        let desired = if dist < PANIC_RADIUS {
            // Too close → kite away to re-open the gap.
            flee(pos, core_pos, KITE_SPEED)
        } else if dist > STANDOFF + 40.0 {
            // Too far → ease back onto the standoff ring.
            arrive(pos, core_pos - radial * STANDOFF, MAX_SPEED, 80.0)
        } else {
            // In the pocket → drift slowly sideways (a swaying turret), the side
            // chosen by a per-enemy phase so a row of sentinels doesn't sync.
            let side = if (t * 0.5 + pos.x * 0.01).sin() > 0.0 { 1.0 } else { -1.0 };
            tangent * side * STRAFE_SPEED
        };
        vel.0 = approach(vel.0, desired, ACCEL);
        let _ = &mut state;

        // Always aim at the Core so the sweep tracks it.
        commands.entity(e).insert(FaceTarget(core_pos));
    }
}
