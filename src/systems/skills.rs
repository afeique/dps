//! Active defense skills bound to keyboard keys. Registers three on-demand
//! abilities for the player ship — Dash (LeftShift), Shield Burst (C), and
//! Bomb (X) — each gated by its own cooldown timer. Cooldowns tick every
//! frame regardless of whether the ability fires. The system is a no-op when
//! no player ship exists (e.g. during GameOver).

use crate::components::*;
use crate::messages::Death;
use crate::resources::Score;
use bevy::prelude::*;

/// Per-ability cooldown state. All fields are seconds remaining; 0 means ready.
#[derive(Resource, Debug)]
pub struct Skills {
    /// Remaining cooldown for Dash (LeftShift). Ready at 0.
    pub dash_cd: f32,
    /// Remaining cooldown for Shield Burst (C). Ready at 0.
    pub shield_cd: f32,
    /// Remaining cooldown for Bomb (X). Ready at 0.
    pub bomb_cd: f32,
}

impl Default for Skills {
    fn default() -> Self {
        Self {
            dash_cd: 0.0,
            shield_cd: 0.0,
            bomb_cd: 0.0,
        }
    }
}

/// Ticks all skill cooldowns and fires abilities on key-just-pressed.
///
/// Ability summary:
/// * **Dash** (`ShiftLeft`, 2 s CD) — forward impulse + 0.3 s i-frames.
/// * **Shield Burst** (`C`, 8 s CD) — a brief 0.8 s invulnerability bubble.
///   (The shield is now a passive damage-reduction %, so there is no pool to
///   "refill" — this is a deliberate-invuln defensive pop.)
/// * **Bomb** (`X`, 15 s CD) — despawn every enemy and enemy bullet;
///   each enemy triggers a `Death` message so explosion FX fire normally.
pub fn use_skills(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    time: Res<Time>,
    mut commands: Commands,
    mut skills: ResMut<Skills>,
    mut deaths: MessageWriter<Death>,
    mut score: ResMut<Score>,
    mut player: Query<(Entity, &mut Velocity, &Transform), With<Ship>>,
    enemies: Query<(Entity, &Transform, &Enemy, Option<&Boss>)>,
    enemy_bullets: Query<(Entity, &Bullet)>,
) {
    let dt = time.delta_secs();

    // Tick all cooldowns unconditionally — even if no player exists this frame.
    skills.dash_cd = (skills.dash_cd - dt).max(0.0);
    skills.shield_cd = (skills.shield_cd - dt).max(0.0);
    skills.bomb_cd = (skills.bomb_cd - dt).max(0.0);

    // Gamepad triggers (first connected pad): LT = dash, LB = shield, North = bomb.
    let pad = gamepads.iter().next();
    let dash_btn = pad.is_some_and(|gp| gp.just_pressed(GamepadButton::LeftTrigger2));
    let shield_btn = pad.is_some_and(|gp| gp.just_pressed(GamepadButton::LeftTrigger));
    let bomb_btn = pad.is_some_and(|gp| gp.just_pressed(GamepadButton::North));

    // Bail if the player ship is absent (GameOver, not yet spawned, etc.).
    let Ok((player_entity, mut vel, tf)) = player.single_mut() else {
        return;
    };

    // --- DASH (LeftShift) ------------------------------------------------
    // Strong forward impulse along the ship's facing direction + brief i-frames.
    if (keys.just_pressed(KeyCode::ShiftLeft) || dash_btn) && skills.dash_cd <= 0.0 {
        let forward = (tf.rotation * Vec3::Y).truncate();
        vel.0 += forward * 600.0;
        commands
            .entity(player_entity)
            .insert(Invulnerable { seconds: 0.3 });
        skills.dash_cd = 2.0;
    }

    // --- SHIELD BURST (C) ------------------------------------------------
    // A brief invulnerability bubble (no pool to refill — the shield is now a
    // passive damage-reduction %).
    if (keys.just_pressed(KeyCode::KeyC) || shield_btn) && skills.shield_cd <= 0.0 {
        commands
            .entity(player_entity)
            .insert(Invulnerable { seconds: 0.8 });
        skills.shield_cd = 8.0;
    }

    // --- BOMB (X) --------------------------------------------------------
    // Clear the field: emit Death for every enemy (triggers FX + drops), then
    // despawn it. Also hard-despawn all enemy-faction bullets immediately.
    if (keys.just_pressed(KeyCode::KeyX) || bomb_btn) && skills.bomb_cd <= 0.0 {
        for (enemy_entity, enemy_tf, enemy, boss) in &enemies {
            deaths.write(Death {
                entity: enemy_entity,
                position: enemy_tf.translation.truncate(),
                kind: Some(enemy.kind),
                boss_tier: boss.map_or(0, |b| b.tier),
            });
            score.kills += 1;
            commands.entity(enemy_entity).despawn();
        }
        for (bullet_entity, bullet) in &enemy_bullets {
            if bullet.kind == BulletKind::Enemy {
                commands.entity(bullet_entity).despawn();
            }
        }
        skills.bomb_cd = 15.0;
    }
}
