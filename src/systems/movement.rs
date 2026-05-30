//! Movement: apply intent to the ship, integrate velocity for everything,
//! confine the player. Runs in `FixedUpdate`, so `Time` is the fixed dt.

use crate::components::{Intent, Ship, Velocity};
use crate::resources::PlayBounds;
use crate::systems::items::{AffixKind, Equipment};
use crate::systems::shop::{momentum_bonus, UpgradeId, Upgrades};
use bevy::prelude::*;

/// Fraction of velocity **retained per second** while coasting — the "space
/// drift" of the inertial model. 0.65 ⇒ the ship sheds ~35%/s with no input
/// (half-life ~1.6 s), so it glides and carries momentum instead of stopping
/// dead. Lower = draggier/tighter, higher = floatier.
const DAMPING: f32 = 0.65;

/// One step of inertial "space" movement: accelerate by `thrust_dir × accel × dt`
/// (thrust_dir is the input, length ≤ 1), apply the framerate-independent
/// [`DAMPING`] drag, then hard-cap the speed to `max_speed`. The ship keeps its
/// momentum — releasing input coasts rather than snapping to a stop, and an
/// external impulse (a collision bounce, a knockback) persists and drifts off.
#[inline]
pub fn inertial_velocity(vel: Vec2, thrust_dir: Vec2, accel: f32, max_speed: f32, dt: f32) -> Vec2 {
    let mut v = vel + thrust_dir * accel * dt;
    v *= DAMPING.powf(dt);
    v.clamp_length_max(max_speed)
}

/// Turn the ship's `Intent` into movement + rotation.
pub fn ship_control(
    time: Res<Time>,
    upgrades: Res<Upgrades>,
    equipment: Res<Equipment>,
    meta: Res<crate::meta::Meta>,
    // Seconds of continuous movement, for the Momentum speed ramp.
    mut sustained: Local<f32>,
    mut q: Query<(&Ship, &Intent, &mut Velocity, &mut Transform)>,
) {
    let dt = time.delta_secs();
    let momentum = upgrades.owned(UpgradeId::Momentum);
    // Equipped SPEED affixes + account SP SPEED raise top speed by a flat
    // fraction (spec VI.5 / sp-stats.js).
    let item_speed =
        equipment.affix_total(AffixKind::Speed) / 100.0 + meta.sp_value("SPEED") / 100.0;
    for (ship, intent, mut vel, mut tf) in &mut q {
        // Momentum passive: top speed ramps with sustained movement (spec VI.3).
        if intent.move_dir.length_squared() > 0.01 {
            *sustained += dt;
        } else {
            *sustained = 0.0;
        }
        let top_speed =
            ship.max_speed * (1.0 + momentum_bonus(*sustained, momentum) + item_speed);

        // Inertial "space" control (WASD / left-stick, screen-space, independent
        // of facing): the input *thrusts* the ship and momentum persists, with a
        // gentle drag so it drifts to rest rather than stopping the instant input
        // releases — floaty space movement. `move_dir` (len ≤1) scales the thrust;
        // the speed is hard-capped at `top_speed`.
        vel.0 = inertial_velocity(vel.0, intent.move_dir, ship.thrust, top_speed, dt);

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
    // Springier than before so hitting the wall in the floaty model reads as a
    // momentum-preserving bounce rather than a dead stop.
    for (mut tf, mut vel) in &mut q {
        if tf.translation.x.abs() > bounds.half.x {
            tf.translation.x = tf.translation.x.clamp(-bounds.half.x, bounds.half.x);
            vel.0.x *= -0.7;
        }
        if tf.translation.y.abs() > bounds.half.y {
            tf.translation.y = tf.translation.y.clamp(-bounds.half.y, bounds.half.y);
            vel.0.y *= -0.7;
        }
    }
}
