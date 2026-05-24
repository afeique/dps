//! Movement: apply intent to the ship, integrate velocity for everything,
//! confine the player. Runs in `FixedUpdate`, so `Time` is the fixed dt.

use crate::components::{Intent, Ship, Velocity};
use crate::resources::PlayBounds;
use crate::systems::items::{AffixKind, Equipment};
use crate::systems::shop::{momentum_bonus, UpgradeId, Upgrades};
use bevy::prelude::*;

/// Velocity response rate per unit of `Ship.thrust` (1/sec). Tuned so the base
/// thrust (1100) yields a tight ~22/s tracking: the ship reaches — and, when
/// WASD is released, sheds — speed in ~30 ms, so control is precise and almost
/// momentum-free. The Afterburner upgrade (+thrust) makes it snappier still.
const RESPONSE_K: f32 = 0.02;

/// Track `current` velocity toward `target` with framerate-independent
/// exponential smoothing at `response` (1/sec). Higher response = tighter,
/// lower-momentum control; the result never overshoots `target`.
#[inline]
pub fn tracked_velocity(current: Vec2, target: Vec2, response: f32, dt: f32) -> Vec2 {
    let t = 1.0 - (-response * dt).exp();
    current.lerp(target, t)
}

/// Turn the ship's `Intent` into movement + rotation.
pub fn ship_control(
    time: Res<Time>,
    upgrades: Res<Upgrades>,
    equipment: Res<Equipment>,
    // Seconds of continuous movement, for the Momentum speed ramp.
    mut sustained: Local<f32>,
    mut q: Query<(&Ship, &Intent, &mut Velocity, &mut Transform)>,
) {
    let dt = time.delta_secs();
    let momentum = upgrades.owned(UpgradeId::Momentum);
    // Equipped SPEED affixes raise top speed by a flat fraction (spec VI.5).
    let item_speed = equipment.affix_total(AffixKind::Speed) / 100.0;
    for (ship, intent, mut vel, mut tf) in &mut q {
        // Momentum passive: top speed ramps with sustained movement (spec VI.3).
        if intent.move_dir.length_squared() > 0.01 {
            *sustained += dt;
        } else {
            *sustained = 0.0;
        }
        let top_speed =
            ship.max_speed * (1.0 + momentum_bonus(*sustained, momentum) + item_speed);

        // Tight twin-stick control (WASD / left-stick, screen-space, independent
        // of facing): track velocity toward the input target rather than
        // accelerating + coasting. `move_dir` is clamped to len ≤1, so the target
        // tops out at `top_speed`; releasing input sheds speed just as fast, so
        // there's almost no glide — precise, low-momentum control.
        let target = intent.move_dir * top_speed;
        vel.0 = tracked_velocity(vel.0, target, ship.thrust * RESPONSE_K, dt);

        // Face the mouse aim point instantly. Forward is +Y, so rotate by
        // (aim_angle - PI/2); the player then fires along this facing.
        if intent.aim_active {
            let to_aim = intent.aim - tf.translation.truncate();
            if to_aim.length_squared() > 0.01 {
                tf.rotation = Quat::from_rotation_z(to_aim.to_angle() - std::f32::consts::FRAC_PI_2);
            }
        }
    }
}

/// Integrate velocity into position for every mover (ship, enemies, bullets).
pub fn integrate(time: Res<Time>, mut q: Query<(&mut Transform, &Velocity)>) {
    let dt = time.delta_secs();
    for (mut tf, vel) in &mut q {
        tf.translation.x += vel.0.x * dt;
        tf.translation.y += vel.0.y * dt;
    }
}

/// Keep the player inside the play box (soft bounce). Enemies bounce in
/// `enemy_ai`; bullets despawn at the edge in `cleanup`.
pub fn confine_player(
    bounds: Res<PlayBounds>,
    mut q: Query<(&mut Transform, &mut Velocity), With<Ship>>,
) {
    for (mut tf, mut vel) in &mut q {
        if tf.translation.x.abs() > bounds.half.x {
            tf.translation.x = tf.translation.x.clamp(-bounds.half.x, bounds.half.x);
            vel.0.x *= -0.5;
        }
        if tf.translation.y.abs() > bounds.half.y {
            tf.translation.y = tf.translation.y.clamp(-bounds.half.y, bounds.half.y);
            vel.0.y *= -0.5;
        }
    }
}
