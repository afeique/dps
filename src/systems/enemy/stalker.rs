//! Stalker AI — an aggressive **zigzag striker**: instead of gliding around a
//! ring it *thrusts* in sharp darts, picking a new heading toward the Core
//! (offset alternately left/right by ~0.6 rad) every cycle and accelerating hard
//! along it. It holds a minimum standoff (it's a sniper, not a rammer), backing
//! off if it drifts too close, and turns to **face the Core** while it darts so
//! its charged shots track. Port of rainboids' `zigzag` movement, retargeted.
//!
//! `AiState.phase` is the per-dart timer; `AiState.wander` is the current dart
//! heading. The ±offset sign flips each dart (stored in the heading itself), so
//! the path reads as a jagged zigzag closing on the Core.

use crate::components::*;
use crate::systems::steering::{approach, flee, seek};
use bevy::prelude::*;

const STANDOFF: f32 = 240.0;
const MAX_SPEED: f32 = 215.0;
const ACCEL: f32 = 14.0; // snappy — sharp darts, not lazy drift
const DART_INTERVAL: f32 = 0.85;
const ZIG_ANGLE: f32 = 0.6; // ±rad off the core bearing

pub fn ai(
    mut commands: Commands,
    time: Res<Time>,
    core: Query<&Transform, (With<Core>, Without<Enemy>)>,
    mut enemies: Query<(Entity, &Transform, &mut Velocity, &mut AiState, &Enemy), With<Enemy>>,
) {
    let Ok(core_tf) = core.single() else {
        return;
    };
    let core_pos = core_tf.translation.truncate();
    let dt = time.delta_secs();

    for (e, tf, mut vel, mut state, enemy) in &mut enemies {
        if enemy.kind != EnemyKind::Stalker && enemy.kind != EnemyKind::FrostLance {
            continue;
        }
        let pos = tf.translation.truncate();
        let to_core = core_pos - pos;
        let dist = to_core.length();
        let bearing = if dist > 1.0 { to_core / dist } else { Vec2::X };

        state.phase -= dt;
        if state.phase <= 0.0 || state.wander == Vec2::ZERO {
            state.phase = DART_INTERVAL;
            // Alternate the zig sign off a position+time hash so consecutive
            // darts swing opposite ways → a jagged closing zigzag.
            let h = ((pos.x * 0.07 + pos.y * 0.11 + state.phase).sin() * 43758.5).fract();
            let sign = if h < 0.5 { 1.0 } else { -1.0 };
            state.wander = Vec2::from_angle(sign * ZIG_ANGLE).rotate(bearing);
        }

        // Too close → back off (sniper standoff); else dart along the zig heading.
        let desired = if dist < STANDOFF * 0.8 {
            flee(pos, core_pos, MAX_SPEED)
        } else {
            seek(pos, pos + state.wander * 120.0, MAX_SPEED)
        };
        vel.0 = approach(vel.0, desired, ACCEL);

        // Turn to face the Core while darting so its charged shots lead onto it.
        commands.entity(e).insert(FaceTarget(core_pos));
    }
}
