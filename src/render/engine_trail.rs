//! Player engine thrust trail (graphical parity with rainboids' moving-ship
//! trails). While the ship is moving, `emit_engine_trail` puffs small HDR
//! exhaust motes out behind it (opposite its velocity); they drift, shrink over
//! their `Lifetime` (`fade_engine_exhaust`), and despawn via `tick_lifetimes`.
//! Pure presentation — the motes carry `Velocity` (so `integrate` drifts them)
//! but no `Collider`, and aren't `Bullet`s, so they touch nothing.

use crate::components::{Lifetime, Ship, Velocity};
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

/// An exhaust mote; holds its spawn lifetime so the fade can scale by remaining.
#[derive(Component)]
pub struct EngineExhaust {
    pub max_life: f32,
}

/// Min ship speed (world u/s) to emit a trail (avoids puffing while idle).
const EMIT_SPEED: f32 = 40.0;
/// Seconds between motes (caps the live count).
const EMIT_INTERVAL: f32 = 0.018;
/// Mote lifetime (s).
const MOTE_LIFE: f32 = 0.42;

/// A small filled disc (HDR cyan-white engine glow; bloom flares it).
fn mote_shape() -> Shape {
    let r = 3.2_f32;
    let mut path = ShapePath::new();
    for i in 0..12 {
        let a = i as f32 / 12.0 * std::f32::consts::TAU;
        let p = Vec2::new(a.cos() * r, a.sin() * r);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    ShapeBuilder::with(&path.close())
        .fill(Color::linear_rgb(2.5, 6.0, 9.0))
        .build()
}

/// Emit exhaust motes behind the moving ship, throttled by `EMIT_INTERVAL`.
pub fn emit_engine_trail(
    time: Res<Time>,
    mut commands: Commands,
    mut accum: Local<f32>,
    ship: Query<(&Transform, &Velocity), With<Ship>>,
) {
    let Ok((tf, vel)) = ship.single() else {
        return;
    };
    let speed = vel.0.length();
    if speed < EMIT_SPEED {
        *accum = 0.0;
        return;
    }
    *accum += time.delta_secs();
    if *accum < EMIT_INTERVAL {
        return;
    }
    *accum = 0.0;

    let back = -vel.0 / speed; // unit vector opposite travel
    // Small perpendicular jitter so the plume has width.
    let perp = Vec2::new(-back.y, back.x);
    let jitter = ((time.elapsed_secs() * 53.0).sin()) * 4.0;
    let pos = tf.translation.truncate() + back * 16.0 + perp * jitter;

    commands.spawn((
        EngineExhaust { max_life: MOTE_LIFE },
        mote_shape(),
        Transform::from_translation(pos.extend(-0.1)), // just behind the ship
        Velocity(back * 70.0),
        Lifetime { seconds: MOTE_LIFE },
    ));
}

/// Shrink each mote toward nothing as its lifetime runs out.
pub fn fade_engine_exhaust(mut q: Query<(&EngineExhaust, &Lifetime, &mut Transform)>) {
    for (mote, life, mut tf) in &mut q {
        let frac = (life.seconds / mote.max_life).clamp(0.0, 1.0);
        tf.scale = Vec3::splat(frac);
    }
}
