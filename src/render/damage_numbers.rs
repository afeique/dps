//! Floating combat damage numbers (spec VIII.1, simplified).
//!
//! Presentation-only: `spawn_damage_numbers` reads `Damage` events (its own
//! cursor, independent of `apply_damage`), places a world-space `Text2d` at the
//! target — gold for enemy hits, red for player hits — and `float_damage_numbers`
//! drifts it up while fading it out, then despawns it. Lethal hits whose target
//! is already despawned this tick are skipped (the explosion covers the kill).
//! Deferred from spec: crit gold / heal green / per-pattern styling.

use crate::components::Ship;
use crate::messages::Damage;
use bevy::prelude::*;

/// A rising, fading damage number.
#[derive(Component)]
pub struct DamageNumber {
    life: f32,
    max_life: f32,
}

/// Upward drift (px/sec) and lifetime (sec).
const RISE: f32 = 42.0;
const LIFE: f32 = 0.8;

/// Spawn a number per `Damage` event at the target's position.
pub fn spawn_damage_numbers(
    mut commands: Commands,
    mut dmg: MessageReader<Damage>,
    targets: Query<(&Transform, Has<Ship>)>,
) {
    for ev in dmg.read() {
        let Ok((tf, is_player)) = targets.get(ev.target) else {
            continue; // target gone this tick (a lethal hit) — skip
        };
        let n = ev.amount.round() as i64;
        if n <= 0 {
            continue; // sub-1 DoT ticks don't spam numbers
        }
        let color = if is_player {
            Color::srgb(1.0, 0.3, 0.25) // red — player took damage
        } else {
            Color::srgb(1.0, 0.85, 0.3) // gold — enemy hit
        };
        let pos = tf.translation.truncate() + Vec2::new(0.0, 14.0);
        commands.spawn((
            DamageNumber {
                life: LIFE,
                max_life: LIFE,
            },
            Text2d::new(format!("{n}")),
            TextFont {
                font_size: 14.0,
                ..default()
            },
            TextColor(color),
            Transform::from_translation(pos.extend(3.0)),
        ));
    }
}

/// Drift each number up, fade it by remaining life, and despawn at end of life.
pub fn float_damage_numbers(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Transform, &mut TextColor, &mut DamageNumber)>,
) {
    let dt = time.delta_secs();
    for (e, mut tf, mut color, mut dn) in &mut q {
        tf.translation.y += RISE * dt;
        dn.life -= dt;
        let a = (dn.life / dn.max_life).clamp(0.0, 1.0);
        color.0.set_alpha(a);
        if dn.life <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}
