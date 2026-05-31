//! Hunter enemy — the **dive-bomber**. Orbits the Core at a stand-off radius,
//! strafes (turning to face the Core to shoot), then every few cycles winds up
//! and **dives straight through the Core** in a fast lunge before looping back
//! out to a fresh orbit slot. Ported/retargeted from rainboids' hunter arc/orbit
//! + lunge movement (`movement.js` `hunterArcMovement`).
//!
//! State (`AiState.phase` = strafe-dwell timer; `AiState.wander.x` = slot angle,
//! `wander.y` = init flag; the [`Diving`] marker = the lunge):
//!   ORBIT → arrive at a point on the ring; STRAFE → hold + face Core + fire;
//!   DIVE (≈1-in-3) → thrust through the Core, overshoot; then a fresh slot.
//! Movement is thrust-based (steering → Velocity) so turns ease in.

use crate::components::{AiState, Core, Diving, Enemy, EnemyKind, FaceTarget, SpeedMul, Velocity};
use crate::systems::enemy::EnemyStats;
use crate::systems::steering::{approach, arrive, seek};
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

// ── Stats ─────────────────────────────────────────────────────────────────────

/// JS HUNTER: health 5, size 32 (radius 16), speed 2.6 px/frame @60 fps.
/// fire_cooldown mirrors the JS `shootRate: 1.5` (seconds between bursts).
pub fn stats() -> EnemyStats {
    EnemyStats {
        health: 5.0,
        radius: 16.0,
        speed: 160.0,
        fire_cooldown: Some(1.5),
    }
}

// ── Shape ─────────────────────────────────────────────────────────────────────

/// Aggressive red-orange triangle, nose pointing +Y (Bevy forward), radius ≈ 16.
pub fn shape() -> Shape {
    let r = stats().radius;
    let nose = Vec2::new(0.0, r * 1.0);
    let right = Vec2::new(r * 0.85, -r * 0.72);
    let notch = Vec2::new(0.0, -r * 0.32);
    let left = Vec2::new(-r * 0.85, -r * 0.72);

    let path = ShapePath::new()
        .move_to(nose)
        .line_to(right)
        .line_to(notch)
        .line_to(left)
        .close();

    ShapeBuilder::with(&path)
        .fill(Color::linear_rgb(0.04, 0.0, 0.0))
        .stroke((Color::linear_rgb(8.0, 1.5, 0.5), 2.0))
        .build()
}

// ── AI ────────────────────────────────────────────────────────────────────────

const ORBIT_RADIUS: f32 = 230.0;
const STRAFE_DWELL: f32 = 1.5;
const ARRIVE_RADIUS: f32 = 28.0;
const SLOW_RADIUS: f32 = 130.0;
const DIVE_SPEED_MUL: f32 = 2.0; // the lunge is much faster than the orbit
const ACCEL_FRAC: f32 = 0.18; // ease factor on the orbit
const DIVE_ACCEL_FRAC: f32 = 0.5; // commits to the dive quickly
const DIVE_TIME: f32 = 0.75; // how long the lunge lasts (overshoots through)

fn is_hunter(kind: EnemyKind) -> bool {
    matches!(
        kind,
        EnemyKind::Hunter | EnemyKind::AshenDetonator | EnemyKind::TeslaWraith
    )
}

pub fn ai(
    time: Res<Time>,
    mut commands: Commands,
    core: Query<&Transform, (With<Core>, Without<Enemy>)>,
    mut q: Query<(
        Entity,
        &Enemy,
        &mut AiState,
        &mut Velocity,
        &Transform,
        Option<&SpeedMul>,
        Option<&mut Diving>,
    )>,
) {
    let Ok(core_tf) = core.single() else {
        return;
    };
    let core_pos = core_tf.translation.truncate();
    let dt = time.delta_secs();
    let base = stats().speed;

    for (e, enemy, mut ai, mut vel, tf, sm, diving) in &mut q {
        if !is_hunter(enemy.kind) {
            continue;
        }
        let spd = base * sm.map_or(1.0, |s| s.0);
        let pos = tf.translation.truncate();

        // ── DIVE: locked into the lunge — thrust through the Core until it ends.
        if let Some(mut dv) = diving {
            dv.timer -= dt;
            let desired = seek(pos, core_pos, spd * DIVE_SPEED_MUL);
            vel.0 = approach(vel.0, desired, spd * DIVE_ACCEL_FRAC);
            commands.entity(e).insert(FaceTarget(core_pos));
            if dv.timer <= 0.0 {
                commands.entity(e).remove::<Diving>();
                let bearing = (pos - core_pos).normalize_or_zero();
                ai.wander.x = bearing.to_angle();
                ai.phase = 0.0;
            }
            continue;
        }

        // Init the orbit slot bearing from the spawn angle so hunters fan out.
        if ai.wander.y == 0.0 {
            ai.wander.x = (pos - core_pos).to_angle();
            ai.wander.y = 1.0;
            ai.phase = STRAFE_DWELL;
        }

        let slot = core_pos + Vec2::from_angle(ai.wander.x) * ORBIT_RADIUS;
        let slot_dist = pos.distance(slot);

        if slot_dist > ARRIVE_RADIUS {
            // ORBIT/REPOSITION: fly to the slot nose-first.
            let desired = arrive(pos, slot, spd, SLOW_RADIUS);
            vel.0 = approach(vel.0, desired, spd * ACCEL_FRAC);
            commands.entity(e).remove::<FaceTarget>();
            ai.phase = STRAFE_DWELL;
        } else {
            // STRAFE: settle on the slot, face the Core, hold while firing.
            let desired = arrive(pos, slot, spd * 0.4, SLOW_RADIUS);
            vel.0 = approach(vel.0, desired, spd * ACCEL_FRAC);
            commands.entity(e).insert(FaceTarget(core_pos));
            ai.phase -= dt;
            if ai.phase <= 0.0 {
                // Either DIVE (≈1-in-3, deterministic from a position hash) or
                // reposition further around the ring.
                let h = ((pos.x * 0.09 + pos.y * 0.13).sin() * 43758.5).fract();
                if h < 0.34 {
                    commands.entity(e).insert(Diving { timer: DIVE_TIME });
                } else {
                    let wobble = (pos.x * 0.017 + pos.y * 0.011).sin() * 0.8;
                    let dir = if (pos.x as i32).rem_euclid(2) == 0 { 1.0 } else { -1.0 };
                    ai.wander.x =
                        (ai.wander.x + dir * (2.1 + wobble)).rem_euclid(std::f32::consts::TAU);
                    ai.phase = STRAFE_DWELL;
                }
            }
        }

        // Speed cap.
        let cap = spd * 1.6;
        let v = vel.0.length();
        if v > cap {
            vel.0 = vel.0 / v * cap;
        }
    }
}
