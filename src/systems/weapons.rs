//! Weapons. `player_fire` ticks the cooldown and emits a `Fire` message when
//! the player is firing; `spawn_bullets` turns each `Fire` into a glowing
//! bullet entity. Splitting fire-intent from bullet-spawning keeps weapon
//! logic independent of how projectiles are realized (Phase 3 power weapons,
//! multishot, homing, etc. just emit different `Fire` patterns).

use crate::components::*;
use crate::messages::Fire;
use crate::render::bullets::BulletAssets;
use bevy::prelude::*;

/// Tick the player's weapon cooldown; emit `Fire` while held + ready.
pub fn player_fire(
    time: Res<Time>,
    mut fire: MessageWriter<Fire>,
    mut q: Query<(&Intent, &mut Weapon, &Transform), With<Ship>>,
) {
    let dt = time.delta_secs();
    for (intent, mut weapon, tf) in &mut q {
        weapon.timer = (weapon.timer - dt).max(0.0);
        if intent.firing && weapon.timer <= 0.0 {
            weapon.timer = weapon.cooldown;
            let dir = (tf.rotation * Vec3::Y).truncate().normalize_or_zero();
            fire.write(Fire {
                origin: tf.translation.truncate() + dir * 20.0,
                dir,
                damage: weapon.damage,
                speed: weapon.bullet_speed,
                faction: Faction::Player,
            });
        }
    }
}

/// Spawn one bullet per `Fire` message from the shared `BulletAssets` (one
/// mesh + per-team materials, scaled per shot — no per-shot asset churn).
/// Player shots get a bright white-hot core child; enemy shots are magenta.
pub fn spawn_bullets(mut commands: Commands, assets: Res<BulletAssets>, mut fire: MessageReader<Fire>) {
    for shot in fire.read() {
        // Per-team body material + collider radius + lifetime.
        let (kind, radius, body, life) = match shot.faction {
            Faction::Player => (BulletKind::Player, 3.0, assets.player_body.clone(), 1.5),
            Faction::Enemy => (BulletKind::Enemy, 4.0, assets.enemy_body.clone(), 3.0),
        };

        let mut bullet = commands.spawn((
            Bullet {
                kind,
                damage: shot.damage,
            },
            Velocity(shot.dir * shot.speed),
            Collider { radius },
            shot.faction,
            Lifetime { seconds: life },
            Mesh2d(assets.circle.clone()),
            MeshMaterial2d(body),
            Transform::from_translation(shot.origin.extend(0.0)).with_scale(Vec3::splat(radius)),
        ));

        if shot.faction == Faction::Player {
            let (circle, core) = (assets.circle.clone(), assets.player_core.clone());
            bullet.with_children(|b| {
                // local scale 0.5 → core radius = bullet radius * 0.5; z in front.
                b.spawn((
                    Mesh2d(circle),
                    MeshMaterial2d(core),
                    Transform::from_xyz(0.0, 0.0, 0.5).with_scale(Vec3::splat(0.5)),
                ));
            });
        }
    }
}
