//! Weapons. `player_fire` ticks the cooldown and emits a `Fire` message when
//! the player is firing; `spawn_bullets` turns each `Fire` into a glowing
//! bullet entity. Splitting fire-intent from bullet-spawning keeps weapon
//! logic independent of how projectiles are realized (Phase 3 power weapons,
//! multishot, homing, etc. just emit different `Fire` patterns).

use crate::components::*;
use crate::messages::Fire;
use crate::render::glow;
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
            });
        }
    }
}

/// Spawn one glowing bullet per `Fire` message.
// TODO(Phase 2): share a single bullet mesh/material handle instead of
// creating one per shot (cache in a Resource set up at startup).
pub fn spawn_bullets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut fire: MessageReader<Fire>,
) {
    for shot in fire.read() {
        let mesh = meshes.add(Circle::new(3.0));
        let mat = materials.add(ColorMaterial::from(glow(9.0, 6.5, 2.0))); // hot gold

        commands.spawn((
            Bullet {
                kind: BulletKind::Player,
                damage: shot.damage,
            },
            Velocity(shot.dir * shot.speed),
            Collider { radius: 3.0 },
            Faction::Player,
            Lifetime { seconds: 1.5 },
            Mesh2d(mesh),
            MeshMaterial2d(mat),
            Transform::from_translation(shot.origin.extend(0.0)),
        ));
    }
}
