//! Enemy firing. `enemy_fire` ticks each enemy's `FireCooldown` and emits a
//! `Fire` message aimed at the player when the cooldown expires. Per-kind
//! firing patterns for all ten enemy kinds are a Phase 3 port from
//! `js/modules/enemy/*`; this is the generic aimed-shot stand-in for Phase 1.

use crate::components::{Enemy, Faction, FireCooldown, Ship};
use crate::messages::Fire;
use bevy::prelude::*;

/// Tick enemy fire cooldowns; emit one `Fire` per enemy that is ready,
/// aimed at the current player position. Returns early if the player is absent.
pub fn enemy_fire(
    time: Res<Time>,
    mut fire: MessageWriter<Fire>,
    player_q: Query<&Transform, With<Ship>>,
    mut q: Query<(&Transform, &mut FireCooldown), With<Enemy>>,
) {
    let Ok(player_tf) = player_q.single() else { return; };
    let player_pos = player_tf.translation.truncate();

    let dt = time.delta_secs();
    for (tf, mut fc) in &mut q {
        fc.timer = (fc.timer - dt).max(0.0);
        if fc.timer <= 0.0 {
            let enemy_pos = tf.translation.truncate();
            let dir = (player_pos - enemy_pos).normalize_or_zero();
            if dir == Vec2::ZERO {
                // Enemy is exactly on the player; leave cooldown at 0 and
                // retry next tick once there is a nonzero separation.
                continue;
            }
            fc.timer = fc.cooldown;
            fire.write(Fire {
                origin: enemy_pos + dir * 22.0,
                dir,
                damage: 12.0,
                speed: 320.0,
                faction: Faction::Enemy,
            });
        }
    }
}
