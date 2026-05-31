//! Prowler enemy — an **orbit-strafer**: it holds a long preferred range like a
//! standoff artillery unit, but instead of sitting still it *circles* the Core on
//! that ring (thrusting along the tangent) while its missiles fire, so it reads as
//! a prowling predator rather than a parked turret. A radius spring keeps it on
//! the ring; the tangential thrust walks it around. Ported from rainboids'
//! `keep_distance` + circler behaviour, retargeted onto the Core.
//!
//! `AiState.wander.x` stores the orbit direction (±1), seeded per-enemy so the
//! pack splits clockwise/counter-clockwise instead of all circling one way.

use crate::components::{AiState, Core, Enemy, EnemyKind, SpeedMul, Velocity};
use crate::systems::enemy::EnemyStats;
use crate::systems::steering::approach;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

// ── Stats ─────────────────────────────────────────────────────────────────────

/// JS PROWLER: health 14, size 45 (radius 22), speed 0.75 px/frame @60 fps.
pub fn stats() -> EnemyStats {
    EnemyStats {
        health: 14.0,
        radius: 22.0,
        speed: 45.0,
        fire_cooldown: Some(2.5),
    }
}

// ── Shape ─────────────────────────────────────────────────────────────────────

/// Armored missile-fortress hull. Authored in Canvas2D (+Y down, nose +X),
/// flipped to Bevy (+Y up) by negating Y. Magenta HDR edge.
pub fn shape() -> Shape {
    let r = stats().radius;
    let s = r * 0.8;

    let hull = [
        Vec2::new(s * 1.1, 0.0),
        Vec2::new(s * 0.6, -s * 0.7),
        Vec2::new(-s * 0.5, -s * 0.9),
        Vec2::new(-s * 1.1, -s * 0.4),
        Vec2::new(-s * 1.1, s * 0.4),
        Vec2::new(-s * 0.5, s * 0.9),
        Vec2::new(s * 0.6, s * 0.7),
    ];
    let pod_x0 = s * 0.1;
    let pod_x1 = s * 0.8;
    let pod_half_h = s * 0.22;
    let pod_cy_s = -s * 0.55;
    let pod_s = [
        Vec2::new(pod_x0, pod_cy_s - pod_half_h),
        Vec2::new(pod_x1, pod_cy_s - pod_half_h),
        Vec2::new(pod_x1, pod_cy_s + pod_half_h),
        Vec2::new(pod_x0, pod_cy_s + pod_half_h),
    ];
    let pod_cy_p = s * 0.55;
    let pod_p = [
        Vec2::new(pod_x0, pod_cy_p - pod_half_h),
        Vec2::new(pod_x1, pod_cy_p - pod_half_h),
        Vec2::new(pod_x1, pod_cy_p + pod_half_h),
        Vec2::new(pod_x0, pod_cy_p + pod_half_h),
    ];

    let path = ShapePath::new()
        .move_to(hull[0]).line_to(hull[1]).line_to(hull[2]).line_to(hull[3])
        .line_to(hull[4]).line_to(hull[5]).line_to(hull[6]).close()
        .move_to(pod_s[0]).line_to(pod_s[1]).line_to(pod_s[2]).line_to(pod_s[3]).close()
        .move_to(pod_p[0]).line_to(pod_p[1]).line_to(pod_p[2]).line_to(pod_p[3]).close();

    ShapeBuilder::with(&path)
        .fill(Color::linear_rgb(0.04, 0.0, 0.06))
        .stroke((Color::linear_rgb(8.0, 0.4, 8.0), 2.0))
        .build()
}

// ── AI / orbit-strafe ─────────────────────────────────────────────────────────

const PREFERRED: f32 = 330.0;
const ORBIT_SPEED: f32 = 130.0;
const ACCEL_FRAC: f32 = 0.05;
/// How hard the radius spring corrects drift back onto the ring (u/s per u error).
const RADIUS_SPRING: f32 = 1.4;

fn is_orbiter(kind: EnemyKind) -> bool {
    matches!(
        kind,
        EnemyKind::Prowler | EnemyKind::SporeCarrier | EnemyKind::Warden
    )
}

pub fn ai(
    core: Query<&Transform, (With<Core>, Without<Enemy>)>,
    mut enemies: Query<
        (&Transform, &mut Velocity, &mut AiState, &Enemy, Option<&SpeedMul>),
        With<Enemy>,
    >,
) {
    let Ok(core_tf) = core.single() else {
        return;
    };
    let core_pos = core_tf.translation.truncate();

    for (tf, mut vel, mut state, enemy, sm) in &mut enemies {
        if !is_orbiter(enemy.kind) {
            continue;
        }
        let orbit_spd = ORBIT_SPEED * sm.map_or(1.0, |s| s.0);
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
        let radius_err = (dist - PREFERRED).clamp(-orbit_spd, orbit_spd);
        let desired = (radial * radius_err * RADIUS_SPRING + tangent * state.wander.x * orbit_spd)
            .clamp_length_max(orbit_spd);
        vel.0 = approach(vel.0, desired, orbit_spd * ACCEL_FRAC);
    }
}
