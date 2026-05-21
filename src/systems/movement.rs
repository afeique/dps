//! Movement: apply intent to the ship, integrate velocity for everything,
//! confine the player. Runs in `FixedUpdate`, so `Time` is the fixed dt.

use crate::components::{Intent, Ship, Velocity};
use crate::resources::PlayBounds;
use bevy::prelude::*;

/// Turn the ship's `Intent` into acceleration + rotation.
pub fn ship_control(time: Res<Time>, mut q: Query<(&Ship, &Intent, &mut Velocity, &mut Transform)>) {
    let dt = time.delta_secs();
    for (ship, intent, mut vel, mut tf) in &mut q {
        // Rotation: face the mouse aim point when active, else strafe-rotate.
        if intent.aim_active {
            let to_aim = intent.aim - tf.translation.truncate();
            if to_aim.length_squared() > 1.0 {
                let desired = to_aim.to_angle();
                let current = (tf.rotation * Vec3::Y).truncate().to_angle();
                // Shortest signed delta, wrapped to (-PI, PI].
                let delta = (desired - current + std::f32::consts::PI)
                    .rem_euclid(std::f32::consts::TAU)
                    - std::f32::consts::PI;
                let max = ship.turn_rate * dt;
                tf.rotate_z(delta.clamp(-max, max));
            }
        } else {
            tf.rotate_z(-intent.strafe * ship.turn_rate * dt);
        }

        // Thrust accelerates along the facing.
        let forward = (tf.rotation * Vec3::Y).truncate();
        vel.0 += forward * (intent.thrust * ship.thrust * dt);

        // Mild drag + hard speed cap (tune against the JS feel in Phase 3).
        vel.0 *= 0.985;
        if vel.0.length() > ship.max_speed {
            vel.0 = vel.0.normalize() * ship.max_speed;
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
