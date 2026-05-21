//! Movement: apply intent to the ship, integrate velocity for everything,
//! confine the player. Runs in `FixedUpdate`, so `Time` is the fixed dt.

use crate::components::{Intent, Ship, Velocity};
use crate::resources::PlayBounds;
use bevy::prelude::*;

/// Turn the ship's `Intent` into acceleration + rotation.
pub fn ship_control(time: Res<Time>, mut q: Query<(&Ship, &Intent, &mut Velocity, &mut Transform)>) {
    let dt = time.delta_secs();
    for (ship, intent, mut vel, mut tf) in &mut q {
        // Move: accelerate in the WASD / left-stick screen-space direction,
        // independent of facing (twin-stick).
        vel.0 += intent.move_dir * (ship.thrust * dt);

        // Mild drag + hard speed cap.
        vel.0 *= 0.985;
        if vel.0.length() > ship.max_speed {
            vel.0 = vel.0.normalize() * ship.max_speed;
        }

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
