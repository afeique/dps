//! Hunter AI — the **dive-bomber**: orbits the Core at a stand-off radius,
//! strafes (turning to face the Core to shoot), then every few cycles winds up
//! and **dives straight through the Core** in a fast lunge before looping back
//! out to a fresh orbit slot. A port of rainboids' hunter arc/orbit + lunge
//! movement, retargeted onto the Core.
//!
//! State machine (`AiState.phase` = per-state timer; `AiState.wander` = the orbit
//! slot target; the [`Diving`] marker = the lunge):
//!   • ORBIT      — arrive at a point on the orbit ring around the Core
//!   • STRAFE     — hold near the slot, face the Core, let firing.rs shoot
//!   • DIVE       — thrust hard through the Core (a lunge), then loop back out
//!   • REPOSITION — after a dwell, pick a new slot (or commit to a dive) and go
//!
//! Movement is thrust-based (Reynolds steering → Velocity), so turns ease in.

use crate::components::*;
use crate::systems::steering::{approach, arrive, seek};
use bevy::prelude::*;

const ORBIT_RADIUS: f32 = 230.0;
const STRAFE_DWELL: f32 = 1.5;
const ARRIVE_SLOWING: f32 = 60.0;
const MAX_SPEED: f32 = 150.0;
const DIVE_SPEED: f32 = 320.0; // the lunge is much faster than the orbit
const ACCEL: f32 = 8.0;
const DIVE_ACCEL: f32 = 22.0; // commits to the dive quickly
const DIVE_TIME: f32 = 0.75; // how long the lunge lasts (overshoots through)

fn is_hunter(kind: EnemyKind) -> bool {
    matches!(
        kind,
        EnemyKind::Hunter | EnemyKind::AshenDetonator | EnemyKind::TeslaWraith
    )
}

pub fn ai(
    mut commands: Commands,
    time: Res<Time>,
    core: Query<&Transform, (With<Core>, Without<Enemy>)>,
    mut enemies: Query<
        (Entity, &Transform, &mut Velocity, &mut AiState, &Enemy, Option<&mut Diving>),
        With<Enemy>,
    >,
) {
    let Ok(core_tf) = core.single() else {
        return;
    };
    let core_pos = core_tf.translation.truncate();
    let dt = time.delta_secs();

    for (e, tf, mut vel, mut state, enemy, diving) in &mut enemies {
        if !is_hunter(enemy.kind) {
            continue;
        }
        let pos = tf.translation.truncate();
        let to_core = core_pos - pos;
        let dist = to_core.length();

        // ── DIVE: locked into the lunge — thrust through the Core until it ends.
        if let Some(mut dv) = diving {
            dv.timer -= dt;
            let desired = seek(pos, core_pos, DIVE_SPEED);
            vel.0 = approach(vel.0, desired, DIVE_ACCEL);
            commands.entity(e).insert(FaceTarget(core_pos));
            if dv.timer <= 0.0 {
                // Pull out: reseat the orbit slot on the *far* side and resume.
                commands.entity(e).remove::<Diving>();
                let bearing = (pos - core_pos).normalize_or_zero();
                state.wander = core_pos + bearing * ORBIT_RADIUS;
                state.phase = 0.0;
            }
            continue;
        }

        state.phase -= dt;

        // The orbit slot is stored in `wander`; (re)initialize it lazily.
        if state.wander == Vec2::ZERO {
            let bearing = if dist > 1.0 { -to_core / dist } else { Vec2::X };
            state.wander = core_pos + bearing * ORBIT_RADIUS;
        }

        let slot = state.wander;
        let slot_dist = (slot - pos).length();

        if slot_dist < 28.0 && state.phase <= 0.0 {
            // Arrived fresh → begin the strafe dwell + face the Core to shoot.
            state.phase = STRAFE_DWELL;
        }

        if state.phase > 0.0 {
            // STRAFE: ease to a near-stop and let the firing system shoot.
            let desired = arrive(pos, slot, MAX_SPEED * 0.35, ARRIVE_SLOWING);
            vel.0 = approach(vel.0, desired, ACCEL);
            commands.entity(e).insert(FaceTarget(core_pos));
        } else {
            // ORBIT/REPOSITION: arrive at the slot at cruising speed.
            let desired = arrive(pos, slot, MAX_SPEED, ARRIVE_SLOWING);
            vel.0 = approach(vel.0, desired, ACCEL);
            if slot_dist < 28.0 {
                // Reached it after the dwell → either DIVE (1-in-3, deterministic
                // from a position hash) or reposition further around the ring.
                let h = ((pos.x * 0.09 + pos.y * 0.13).sin() * 43758.5).fract();
                if h < 0.34 {
                    commands.entity(e).insert(Diving { timer: DIVE_TIME });
                } else {
                    let bearing = (pos - core_pos).normalize_or_zero();
                    let rot = Vec2::new(
                        bearing.x * (-0.5) - bearing.y * 0.866,
                        bearing.x * 0.866 + bearing.y * (-0.5),
                    );
                    state.wander = core_pos + rot * ORBIT_RADIUS;
                }
            } else {
                // In transit, fly nose-first (heading) — drop any face lock.
                commands.entity(e).remove::<FaceTarget>();
            }
        }
    }
}
