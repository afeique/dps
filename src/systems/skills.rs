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
    /// Remaining cooldown for EMP Pulse (V). Ready at 0.
    pub emp_cd: f32,
    /// Remaining cooldown for Bulwark (G). Ready at 0.
    pub bulwark_cd: f32,
    /// Remaining cooldown for Repair Nanites (H). Ready at 0.
    pub repair_cd: f32,
}

impl Default for Skills {
    fn default() -> Self {
        Self {
            dash_cd: 0.0,
            shield_cd: 0.0,
            bomb_cd: 0.0,
            emp_cd: 0.0,
            bulwark_cd: 0.0,
            repair_cd: 0.0,
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
/// * **Bulwark** (`G`, 20 s CD) — a 4 s window of 50% incoming-damage resist
///   (the `Bulwark` component, read by `apply_damage`).
/// * **Repair Nanites** (`H`, 25 s CD) — regen 3 HP/s for 5 s (the `Repairing`
///   component, applied by `tick_repair`).
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
    skills.bulwark_cd = (skills.bulwark_cd - dt).max(0.0);
    skills.repair_cd = (skills.repair_cd - dt).max(0.0);

    // Gamepad triggers (first connected pad): LT = dash, LB = shield, North = bomb,
    // DPadDown = bulwark, DPadUp = repair (East is EMP).
    let pad = gamepads.iter().next();
    let dash_btn = pad.is_some_and(|gp| gp.just_pressed(GamepadButton::LeftTrigger2));
    let shield_btn = pad.is_some_and(|gp| gp.just_pressed(GamepadButton::LeftTrigger));
    let bomb_btn = pad.is_some_and(|gp| gp.just_pressed(GamepadButton::North));
    let bulwark_btn = pad.is_some_and(|gp| gp.just_pressed(GamepadButton::DPadDown));
    let repair_btn = pad.is_some_and(|gp| gp.just_pressed(GamepadButton::DPadUp));

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

    // --- BULWARK (G) -----------------------------------------------------
    // A 4 s window of halved incoming damage (spec III.4). The Bulwark
    // component is read by apply_damage; tick_bulwark expires it.
    if (keys.just_pressed(KeyCode::KeyG) || bulwark_btn) && skills.bulwark_cd <= 0.0 {
        commands
            .entity(player_entity)
            .insert(Bulwark { seconds: 4.0 });
        skills.bulwark_cd = 20.0;
    }

    // --- REPAIR NANITES (H) ----------------------------------------------
    // Regenerate 3 HP/s for 5 s (spec III.4). `tick_repair` applies + expires.
    if (keys.just_pressed(KeyCode::KeyH) || repair_btn) && skills.repair_cd <= 0.0 {
        commands.entity(player_entity).insert(Repairing {
            seconds: 5.0,
            rate: 3.0,
        });
        skills.repair_cd = 25.0;
    }
}

/// Apply each active `Repairing` window's regen and expire it.
pub fn tick_repair(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Health, &mut Repairing)>,
) {
    let dt = time.delta_secs();
    for (e, mut hp, mut rep) in &mut q {
        hp.current = (hp.current + rep.rate * dt).min(hp.max);
        rep.seconds -= dt;
        if rep.seconds <= 0.0 {
            commands.entity(e).remove::<Repairing>();
        }
    }
}

/// Count down the Bulwark window and remove it on expiry (mirrors
/// `damage::tick_invulnerability`).
pub fn tick_bulwark(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Bulwark)>,
) {
    for (e, mut b) in &mut q {
        b.seconds -= time.delta_secs();
        if b.seconds <= 0.0 {
            commands.entity(e).remove::<Bulwark>();
        }
    }
}

/// EMP Pulse radius (spec III.4: `200 + 60*WIDE_BAND`; base = 200).
pub const EMP_RADIUS: f32 = 200.0;
/// EMP stun duration (spec III.4: 2000 ms).
const EMP_STUN_SECS: f32 = 2.0;

/// **EMP Pulse** (`V` / gamepad East, 22 s CD, spec III.4): stun every enemy
/// within `EMP_RADIUS` of the ship for `EMP_STUN_SECS` (no damage). Distinct
/// from Bomb (which clears the field) — EMP disables, it doesn't destroy.
pub fn emp_pulse(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    time: Res<Time>,
    mut commands: Commands,
    mut skills: ResMut<Skills>,
    player: Query<&Transform, With<Ship>>,
    enemies: Query<(Entity, &Transform), With<Enemy>>,
) {
    skills.emp_cd = (skills.emp_cd - time.delta_secs()).max(0.0);

    let pad = gamepads.iter().any(|gp| gp.just_pressed(GamepadButton::East));
    if !(keys.just_pressed(KeyCode::KeyV) || pad) || skills.emp_cd > 0.0 {
        return;
    }
    let Ok(ptf) = player.single() else {
        return;
    };
    let center = ptf.translation.truncate();
    for (e, etf) in &enemies {
        if etf.translation.truncate().distance(center) <= EMP_RADIUS {
            commands.entity(e).insert(Stunned { secs: EMP_STUN_SECS });
        }
    }
    skills.emp_cd = 22.0;
}
