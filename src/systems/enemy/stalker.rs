//! Stalker enemy — an aggressive **zigzag striker**: instead of gliding around a
//! ring it *thrusts* in sharp darts, picking a new heading toward the Core
//! (offset alternately left/right by ~0.6 rad) every cycle and accelerating hard
//! along it. It holds a sniper standoff (backs off if it drifts too close) and
//! turns to **face the Core** while it darts so its charged shots track. Ported
//! from rainboids' `arc`/zigzag movement, retargeted onto the Core.
//!
//! `AiState.phase` is the per-dart timer; `AiState.wander` is the current dart
//! heading (its ±offset sign flips each dart → a jagged closing zigzag).

use crate::components::{AiState, Core, Enemy, EnemyKind, FaceTarget, SpeedMul, Velocity};
use crate::systems::enemy::EnemyStats;
use crate::systems::steering::{approach, flee, seek};
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

// ── Stats ─────────────────────────────────────────────────────────────────────

/// JS STALKER: health 7, size 38 (radius 19), speed 3.1 px/frame @60 fps.
pub fn stats() -> EnemyStats {
    EnemyStats {
        health: 7.0,
        radius: 19.0,
        speed: 180.0,
        fire_cooldown: Some(2.0),
    }
}

// ── Shape ─────────────────────────────────────────────────────────────────────

/// Cloaked stealth interceptor silhouette — mantis blade wings, plasma cyan
/// edges. Authored in Canvas2D (+Y down, nose +X) then rotated 90° so the nose
/// points +Y (Bevy forward): bevy_x = −canvas_y, bevy_y = +canvas_x.
pub fn shape() -> Shape {
    let s = stats().radius * 0.92;
    let cv = |cx: f32, cy: f32| -> Vec2 { Vec2::new(-cy, cx) };

    let hull = [
        cv(s * 1.3, 0.0),
        cv(s * 0.4, -s * 0.22),
        cv(-s * 0.5, -s * 0.18),
        cv(-s * 0.75, 0.0),
        cv(-s * 0.5, s * 0.18),
        cv(s * 0.4, s * 0.22),
    ];
    let blade_up = [
        cv(s * 0.55, -s * 0.2),
        cv(s * 1.05, -s * 0.85),
        cv(s * 0.05, -s * 1.1),
        cv(-s * 0.45, -s * 0.55),
        cv(-s * 0.35, -s * 0.18),
    ];
    let blade_dn = [
        cv(s * 0.55, s * 0.2),
        cv(s * 1.05, s * 0.85),
        cv(s * 0.05, s * 1.1),
        cv(-s * 0.45, s * 0.55),
        cv(-s * 0.35, s * 0.18),
    ];

    let mut path = ShapePath::new().move_to(hull[0]);
    for &p in &hull[1..] {
        path = path.line_to(p);
    }
    let path = path.close();
    let mut path = path.move_to(blade_up[0]);
    for &p in &blade_up[1..] {
        path = path.line_to(p);
    }
    let path = path.close();
    let mut path = path.move_to(blade_dn[0]);
    for &p in &blade_dn[1..] {
        path = path.line_to(p);
    }
    let path = path.close();

    ShapeBuilder::with(&path)
        .fill(Color::linear_rgb(0.0, 0.05, 0.06))
        .stroke((Color::linear_rgb(0.4, 8.0, 9.0), 1.5))
        .build()
}

// ── AI / zigzag-thrust ─────────────────────────────────────────────────────────

const STANDOFF: f32 = 240.0;
const ACCEL_FRAC: f32 = 0.08; // snappy — sharp darts, not lazy drift
const DART_INTERVAL: f32 = 0.85;
const ZIG_ANGLE: f32 = 0.6; // ±rad off the core bearing

pub fn ai(
    mut commands: Commands,
    time: Res<Time>,
    core: Query<&Transform, (With<Core>, Without<Enemy>)>,
    mut enemies: Query<
        (Entity, &Transform, &mut Velocity, &mut AiState, &Enemy, Option<&SpeedMul>),
        With<Enemy>,
    >,
) {
    let Ok(core_tf) = core.single() else {
        return;
    };
    let core_pos = core_tf.translation.truncate();
    let dt = time.delta_secs();
    let base = stats().speed;

    for (e, tf, mut vel, mut state, enemy, sm) in &mut enemies {
        if enemy.kind != EnemyKind::Stalker && enemy.kind != EnemyKind::FrostLance {
            continue;
        }
        let spd = base * sm.map_or(1.0, |s| s.0);
        let pos = tf.translation.truncate();
        let to_core = core_pos - pos;
        let dist = to_core.length();
        let bearing = if dist > 1.0 { to_core / dist } else { Vec2::X };

        state.phase -= dt;
        if state.phase <= 0.0 || state.wander == Vec2::ZERO {
            state.phase = DART_INTERVAL;
            // Alternate the zig sign off a position+time hash → a jagged closing zigzag.
            let h = ((pos.x * 0.07 + pos.y * 0.11 + state.phase).sin() * 43758.5).fract();
            let sign = if h < 0.5 { 1.0 } else { -1.0 };
            state.wander = Vec2::from_angle(sign * ZIG_ANGLE).rotate(bearing);
        }

        // Too close → back off (sniper standoff); else dart along the zig heading.
        let desired = if dist < STANDOFF * 0.8 {
            flee(pos, core_pos, spd)
        } else {
            seek(pos, pos + state.wander * 120.0, spd)
        };
        vel.0 = approach(vel.0, desired, spd * ACCEL_FRAC);

        // Turn to face the Core while darting so its charged shots lead onto it.
        commands.entity(e).insert(FaceTarget(core_pos));
    }
}
