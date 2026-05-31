//! Sentinel enemy — a **kiter**: it holds a mid standoff from the Core and slowly
//! sidesteps along the ring while its firing arm sweeps, but the moment anything
//! pushes it inside its comfort radius it *thrusts away* to re-open the gap before
//! resuming its aim. Always turns to face the Core so its sweep tracks. Ported
//! from rainboids' sniper/kite behaviour, retargeted onto the Core.

use crate::components::{Core, Enemy, EnemyKind, FaceTarget, SpeedMul, Velocity};
use crate::systems::enemy::EnemyStats;
use crate::systems::steering::{approach, arrive, flee};
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;
use std::f32::consts::TAU;

// ── Stats ─────────────────────────────────────────────────────────────────────

/// JS SENTINEL: health 10, size 41 (radius ~20), speed 2.5 px/frame @60 fps.
pub fn stats() -> EnemyStats {
    EnemyStats {
        health: 10.0,
        radius: 20.0,
        speed: 150.0,
        fire_cooldown: Some(2.2),
    }
}

// ── Shape ─────────────────────────────────────────────────────────────────────

/// Orbital shield-sentinel: nested hex rings + 6 emitter arms + solid inner hull.
/// Radially symmetric, so the Y-flip is a no-op. HDR green edge.
pub fn shape() -> Shape {
    let r = stats().radius;
    let size = r * 0.8;
    let hex_pts = |radius: f32, offset: f32| -> [Vec2; 6] {
        std::array::from_fn(|i| {
            let a = (i as f32 / 6.0) * TAU + offset;
            Vec2::new(a.cos() * radius, a.sin() * radius)
        })
    };

    let outer = hex_pts(size * 1.2, 0.0);
    let mut path = ShapePath::new().move_to(outer[0]);
    for &p in &outer[1..] {
        path = path.line_to(p);
    }
    path = path.close();

    let inner = hex_pts(size * 0.88, std::f32::consts::FRAC_PI_6);
    path = path.move_to(inner[0]);
    for &p in &inner[1..] {
        path = path.line_to(p);
    }
    path = path.close();

    for i in 0..6_usize {
        let a = (i as f32 / 6.0) * TAU;
        let from = Vec2::new(a.cos() * size * 0.28, a.sin() * size * 0.28);
        let to = Vec2::new(a.cos() * size * 0.75, a.sin() * size * 0.75);
        path = path.move_to(from).line_to(to);
    }

    let hull = hex_pts(size * 0.4, 0.0);
    path = path.move_to(hull[0]);
    for &p in &hull[1..] {
        path = path.line_to(p);
    }
    let path = path.close();

    ShapeBuilder::with(&path)
        .fill(Color::linear_rgb(0.0, 0.02, 0.01))
        .stroke((Color::linear_rgb(0.4, 8.0, 0.8), 2.0))
        .build()
}

// ── AI / kite ─────────────────────────────────────────────────────────────────

const STANDOFF: f32 = 280.0;
const PANIC_RADIUS: f32 = 200.0; // inside this it kites away hard
const ACCEL_FRAC: f32 = 0.06;
const KITE_SPEED_MUL: f32 = 1.27; // faster while fleeing than while holding
const STRAFE_SPEED: f32 = 45.0; // gentle lateral drift while holding station

pub fn ai(
    mut commands: Commands,
    time: Res<Time>,
    core: Query<&Transform, (With<Core>, Without<Enemy>)>,
    mut enemies: Query<(Entity, &Transform, &mut Velocity, &Enemy, Option<&SpeedMul>), With<Enemy>>,
) {
    let Ok(core_tf) = core.single() else {
        return;
    };
    let core_pos = core_tf.translation.truncate();
    let t = time.elapsed_secs();
    let base = stats().speed;

    for (e, tf, mut vel, enemy, sm) in &mut enemies {
        if enemy.kind != EnemyKind::Sentinel {
            continue;
        }
        let spd = base * sm.map_or(1.0, |s| s.0);
        let pos = tf.translation.truncate();
        let to_core = core_pos - pos;
        let dist = to_core.length();
        let radial = if dist > 1.0 { to_core / dist } else { Vec2::X };
        let tangent = Vec2::new(-radial.y, radial.x);

        let desired = if dist < PANIC_RADIUS {
            flee(pos, core_pos, spd * KITE_SPEED_MUL)
        } else if dist > STANDOFF + 40.0 {
            arrive(pos, core_pos - radial * STANDOFF, spd, 80.0)
        } else {
            let side = if (t * 0.5 + pos.x * 0.01).sin() > 0.0 { 1.0 } else { -1.0 };
            tangent * side * STRAFE_SPEED
        };
        vel.0 = approach(vel.0, desired, spd * ACCEL_FRAC);

        // Always aim at the Core so the sweep tracks it.
        commands.entity(e).insert(FaceTarget(core_pos));
    }
}
