//! Powerup pickups: spawn rare gems on enemy death, collect them for
//! field-clearing or stat boosts. Mirrors `systems::drops` structure. Gems
//! drift via the shared `integrate` system and expire via `tick_lifetime`.

use crate::components::*;
use crate::messages::Death;
use crate::resources::Score;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;
use std::f32::consts::TAU;

// ─── Kind + Component ────────────────────────────────────────────────────────

/// The three powerup categories that can drop from enemies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerupKind {
    /// Instantly heals HP to full. (Was `ShieldRestore`; the shield is now a
    /// passive damage-reduction %, so there is no pool to restore — spec II.2.)
    Repair,
    /// Grants one extra spare health tank.
    ExtraLife,
    /// Clears all enemies and enemy bullets on screen.
    Bomb,
}

/// Marks a powerup gem on the field.
#[derive(Component, Debug, Clone, Copy)]
pub struct Powerup {
    pub kind: PowerupKind,
}

// ─── Shape builder ───────────────────────────────────────────────────────────

/// Build a small hexagonal gem for `kind`, filled+stroked with a
/// kind-specific HDR-emissive color so Bloom turns it into a glowing crystal.
///
/// | Kind           | Color (linear RGB)         |
/// |----------------|----------------------------|
/// | Repair         | cyan   `(1.0, 7.0, 9.0)`  |
/// | ExtraLife      | green  `(1.0, 9.0, 2.0)`  |
/// | Bomb           | orange `(9.0, 2.0, 0.5)`  |
pub fn shape(kind: PowerupKind) -> Shape {
    const R: f32 = 7.0;

    // Regular hexagon, flat-top orientation (first vertex at angle 0).
    let mut path = ShapePath::new();
    for i in 0..6_u32 {
        let angle = (i as f32 / 6.0) * TAU;
        let p = Vec2::new(angle.cos() * R, angle.sin() * R);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    let path = path.close();

    let color = match kind {
        PowerupKind::Repair    => Color::linear_rgb(1.0, 7.0, 9.0), // HDR cyan
        PowerupKind::ExtraLife => Color::linear_rgb(1.0, 9.0, 2.0), // HDR green
        PowerupKind::Bomb      => Color::linear_rgb(9.0, 2.0, 0.5), // HDR red-orange
    };

    ShapeBuilder::with(&path)
        .fill(color)
        .stroke((color, 1.5))
        .build()
}

// ─── Spawn powerups ──────────────────────────────────────────────────────────

/// Spawn a powerup gem at the position of a `Death` — but only rarely.
///
/// Selection rule (fully deterministic, no RNG):
/// - Drop only when `death.entity.to_bits() as u64 % 7 == 0`  (~14 % rate).
/// - Kind cycles through the three variants: `(bits / 7) % 3`.
/// - Drift direction is derived from the same bits so gems spread naturally.
pub fn spawn_powerups(mut commands: Commands, mut deaths: MessageReader<Death>) {
    for death in deaths.read() {
        let bits = death.entity.to_bits() as u64;
        if bits % 7 != 0 {
            continue; // most deaths produce no powerup
        }

        let kind = match (bits / 7) % 3 {
            0 => PowerupKind::Repair,
            1 => PowerupKind::ExtraLife,
            _ => PowerupKind::Bomb,
        };

        // Deterministic slow drift (Wang-style hash → [-1,1]² * speed).
        let h1 = (bits as u32).wrapping_mul(2_654_435_761).wrapping_add(0x9e37_79b9);
        let h2 = h1.wrapping_mul(2_246_822_519).wrapping_add(0x27d4_eb2f);
        let fx = ((h1 >> 8) & 0xFFFF) as f32 / 32767.5 - 1.0;
        let fy = ((h2 >> 8) & 0xFFFF) as f32 / 32767.5 - 1.0;
        let drift = Vec2::new(fx, fy) * 18.0; // gentle drift, world-units/sec

        commands.spawn((
            Powerup { kind },
            shape(kind),
            Transform::from_xyz(death.position.x, death.position.y, 0.6),
            Velocity(drift),
            Collider { radius: 10.0 },
            Lifetime { seconds: 14.0 },
        ));
    }
}

// ─── Collect powerups ────────────────────────────────────────────────────────

/// Circle-overlap player vs every powerup gem. On contact, apply the effect
/// and despawn the gem.
///
/// Effects:
/// - `Repair`    → `hp.current = hp.max`
/// - `ExtraLife` → `lives.count += 1`
/// - `Bomb`      → write `Death` for every live enemy, despawn enemy
///   bullets of kind `BulletKind::Enemy`, increment `score.kills`
pub fn collect_powerups(
    mut commands: Commands,
    mut score: ResMut<Score>,
    mut deaths: MessageWriter<Death>,
    mut player: Query<(&Transform, &Collider, &mut Health, &mut Lives), With<Ship>>,
    powerups: Query<(Entity, &Transform, &Collider, &Powerup)>,
    enemies: Query<(Entity, &Transform, &Enemy, Option<&Boss>)>,
    enemy_bullets: Query<(Entity, &Bullet)>,
) {
    let Ok((ptf, pc, mut hp, mut lives)) = player.single_mut() else {
        return; // no player (GameOver or not yet spawned) — skip cleanly
    };
    let player_pos = ptf.translation.truncate();

    for (gem_e, gtf, gc, powerup) in &powerups {
        let reach = pc.radius + gc.radius;
        let d2 = player_pos.distance_squared(gtf.translation.truncate());
        if d2 > reach * reach {
            continue;
        }

        match powerup.kind {
            PowerupKind::Repair => {
                hp.current = hp.max;
            }
            PowerupKind::ExtraLife => {
                lives.count += 1;
            }
            PowerupKind::Bomb => {
                // Broadcast Death for every enemy so explosions + drops fire normally.
                for (enemy_e, enemy_tf, enemy, boss) in &enemies {
                    deaths.write(Death {
                        entity: enemy_e,
                        position: enemy_tf.translation.truncate(),
                        kind: Some(enemy.kind),
                        boss_tier: boss.map_or(0, |b| b.tier),
                    });
                    score.kills = score.kills.saturating_add(1);
                    commands.entity(enemy_e).despawn();
                }
                // Clear enemy bullets.
                for (bullet_e, bullet) in &enemy_bullets {
                    if bullet.kind == BulletKind::Enemy {
                        commands.entity(bullet_e).despawn();
                    }
                }
            }
        }

        commands.entity(gem_e).despawn();
    }
}
