//! Active defense skills bound to keyboard keys. Registers three on-demand
//! abilities for the player ship — Dash (LeftShift), Shield Burst (C), and
//! Bomb (X) — each gated by its own cooldown timer. Cooldowns tick every
//! frame regardless of whether the ability fires. The system is a no-op when
//! no player ship exists (e.g. during GameOver).

use crate::components::*;
use crate::messages::Death;
use crate::resources::{EnergyMeter, Score};
use crate::systems::shop::{kinetic_battery_refund, phase_echo_secs, UpgradeId, Upgrades};
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;
use std::f32::consts::TAU;

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
    /// Remaining cooldown for Deflector Orbs (J). Ready at 0.
    pub deflector_cd: f32,
    /// Remaining cooldown for Tractor Shield (K). Ready at 0.
    pub tractor_cd: f32,
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
            deflector_cd: 0.0,
            tractor_cd: 0.0,
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
/// * **Deflector Orbs** (`J`, 15 s CD) — 3 orbiting orbs that each absorb 3
///   enemy bullets (`cast_deflectors`/`orbit_deflectors`/`deflector_blocks`).
/// * **Tractor Shield** (`K`, 18 s CD) — a 4 s forward-arc field that absorbs
///   enemy bullets into coins (`TractorShield`, `tractor_absorb`).
pub fn use_skills(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    time: Res<Time>,
    // Optional so headless tests (no audio assets) still run the skill logic.
    sfx: Option<Res<crate::audio::Sfx>>,
    mut energy: ResMut<EnergyMeter>,
    mut commands: Commands,
    mut skills: ResMut<Skills>,
    mut deaths: MessageWriter<Death>,
    mut score: ResMut<Score>,
    upgrades: Res<Upgrades>,
    mut player: Query<(Entity, &mut Velocity, &Transform), With<Ship>>,
    enemies: Query<(Entity, &Transform, &Enemy, Option<&Boss>, Has<MiniBoss>)>,
    enemy_bullets: Query<(Entity, &Bullet)>,
) {
    let dt = time.delta_secs();

    // Tick all cooldowns unconditionally — even if no player exists this frame.
    skills.dash_cd = (skills.dash_cd - dt).max(0.0);
    skills.shield_cd = (skills.shield_cd - dt).max(0.0);
    skills.bomb_cd = (skills.bomb_cd - dt).max(0.0);
    skills.bulwark_cd = (skills.bulwark_cd - dt).max(0.0);
    skills.repair_cd = (skills.repair_cd - dt).max(0.0);
    skills.tractor_cd = (skills.tractor_cd - dt).max(0.0);

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
        // Phase Echo extends the dash i-frames (+2 s/stack, spec VI.3).
        let invuln = 0.3 + phase_echo_secs(upgrades.owned(UpgradeId::PhaseEcho));
        commands
            .entity(player_entity)
            .insert(Invulnerable { seconds: invuln });
        skills.dash_cd = 2.0;
        // KINETIC BATTERY: a dash refunds power-weapon energy.
        energy.gain(kinetic_battery_refund(upgrades.owned(UpgradeId::KineticBattery)));
        if let Some(sfx) = &sfx {
            commands.spawn((AudioPlayer::new(sfx.dash.clone()), PlaybackSettings::DESPAWN));
        }
    }

    // --- SHIELD BURST (C) ------------------------------------------------
    // A brief invulnerability bubble (no pool to refill — the shield is now a
    // passive damage-reduction %).
    if (keys.just_pressed(KeyCode::KeyC) || shield_btn) && skills.shield_cd <= 0.0 {
        commands
            .entity(player_entity)
            .insert(Invulnerable { seconds: 0.8 });
        skills.shield_cd = 8.0;
        if let Some(sfx) = &sfx {
            commands.spawn((AudioPlayer::new(sfx.shield.clone()), PlaybackSettings::DESPAWN));
        }
    }

    // --- BOMB (X) --------------------------------------------------------
    // Clear the field: emit Death for every enemy (triggers FX + drops), then
    // despawn it. Also hard-despawn all enemy-faction bullets immediately.
    if (keys.just_pressed(KeyCode::KeyX) || bomb_btn) && skills.bomb_cd <= 0.0 {
        if let Some(sfx) = &sfx {
            commands.spawn((AudioPlayer::new(sfx.bomb.clone()), PlaybackSettings::DESPAWN));
        }
        for (enemy_entity, enemy_tf, enemy, boss, mini_boss) in &enemies {
            deaths.write(Death {
                entity: enemy_entity,
                position: enemy_tf.translation.truncate(),
                kind: Some(enemy.kind),
                boss_tier: boss.map_or(0, |b| b.tier),
                mini_boss,
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
        if let Some(sfx) = &sfx {
            commands.spawn((AudioPlayer::new(sfx.shield.clone()), PlaybackSettings::DESPAWN));
        }
    }

    // --- REPAIR NANITES (H) ----------------------------------------------
    // Regenerate 3 HP/s for 5 s (spec III.4). `tick_repair` applies + expires.
    if (keys.just_pressed(KeyCode::KeyH) || repair_btn) && skills.repair_cd <= 0.0 {
        commands.entity(player_entity).insert(Repairing {
            seconds: 5.0,
            rate: 3.0,
        });
        skills.repair_cd = 25.0;
        if let Some(sfx) = &sfx {
            commands.spawn((AudioPlayer::new(sfx.ability.clone()), PlaybackSettings::DESPAWN));
        }
    }

    // --- TRACTOR SHIELD (K) ----------------------------------------------
    // A 4 s window absorbing forward-arc enemy bullets into coins (spec III.4).
    if (keys.just_pressed(KeyCode::KeyK) || keys.just_pressed(KeyCode::KeyL))
        && skills.tractor_cd <= 0.0
    {
        commands
            .entity(player_entity)
            .insert(TractorShield { seconds: 4.0 });
        skills.tractor_cd = 18.0;
        if let Some(sfx) = &sfx {
            commands.spawn((AudioPlayer::new(sfx.ability.clone()), PlaybackSettings::DESPAWN));
        }
    }
}

/// Count down the Tractor Shield window and remove it on expiry.
pub fn tick_tractor(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut TractorShield)>,
) {
    for (e, mut t) in &mut q {
        t.seconds -= time.delta_secs();
        if t.seconds <= 0.0 {
            commands.entity(e).remove::<TractorShield>();
        }
    }
}

/// Is `to_bullet` (player→bullet) inside the tractor capture cone — within
/// `range` and within `half_arc` of `facing`? `facing` is normalized.
pub fn in_tractor_arc(facing: Vec2, to_bullet: Vec2, half_arc: f32, range: f32) -> bool {
    let dist = to_bullet.length();
    if dist < 1e-4 {
        return true; // sitting on the ship — capture it
    }
    if dist > range {
        return false;
    }
    facing.dot(to_bullet / dist) >= half_arc.cos()
}

/// While Tractor Shield is up, absorb forward-arc enemy bullets into coins
/// (spec III.4). Runs in the collision group, before the bullet can hit the ship.
pub fn tractor_absorb(
    mut commands: Commands,
    mut score: ResMut<Score>,
    player: Query<&Transform, (With<Ship>, With<TractorShield>)>,
    bullets: Query<(Entity, &Transform, &Bullet)>,
) {
    let Ok(ptf) = player.single() else {
        return; // tractor not active
    };
    let ppos = ptf.translation.truncate();
    let facing = (ptf.rotation * Vec3::Y).truncate().normalize_or_zero();
    for (be, btf, bullet) in &bullets {
        if bullet.kind != BulletKind::Enemy {
            continue;
        }
        let to_bullet = btf.translation.truncate() - ppos;
        if in_tractor_arc(facing, to_bullet, TRACTOR_HALF_ARC, TRACTOR_RANGE) {
            commands.entity(be).despawn();
            score.gold = score.gold.saturating_add(TRACTOR_COINS);
        }
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
        // Over-fill allowed; overheal_to_tanks converts the overflow (spec II.2).
        hp.current += rep.rate * dt;
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

/// Number of deflector orbs cast (spec III.4: `3 + EXTRA_ORB`; base 3).
const DEFLECTOR_COUNT: u32 = 3;
/// Bullets each orb blocks before popping (spec III.4: `3 + 2*HARDENED`; base 3).
const DEFLECTOR_BLOCKS: u32 = 3;
/// Orb orbit angular speed (rad/sec).
const DEFLECTOR_OMEGA: f32 = 2.2;

/// A small filled cyan orb (`#44ddff`, HDR for bloom).
fn orb_shape() -> Shape {
    let r = 6.0_f32;
    let mut path = ShapePath::new();
    for i in 0..16 {
        let a = i as f32 / 16.0 * TAU;
        let p = Vec2::new(a.cos() * r, a.sin() * r);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    ShapeBuilder::with(&path.close())
        .fill(Color::linear_rgb(1.0, 7.0, 9.0))
        .build()
}

/// **Deflector Orbs** (`J` / gamepad DPadRight, 15 s CD, spec III.4):
/// spawn `DEFLECTOR_COUNT` orbs that orbit the ship for 5 s, each absorbing
/// `DEFLECTOR_BLOCKS` enemy bullets.
pub fn cast_deflectors(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    time: Res<Time>,
    sfx: Option<Res<crate::audio::Sfx>>,
    mut commands: Commands,
    mut skills: ResMut<Skills>,
    player: Query<&Transform, With<Ship>>,
) {
    skills.deflector_cd = (skills.deflector_cd - time.delta_secs()).max(0.0);

    let pad = gamepads.iter().any(|gp| gp.just_pressed(GamepadButton::DPadRight));
    if !(keys.just_pressed(KeyCode::KeyJ) || pad) || skills.deflector_cd > 0.0 {
        return;
    }
    let Ok(ptf) = player.single() else {
        return;
    };
    spawn_deflector_orbs(&mut commands, ptf.translation.truncate());
    skills.deflector_cd = 15.0;
    if let Some(sfx) = &sfx {
        commands.spawn((AudioPlayer::new(sfx.ability.clone()), PlaybackSettings::DESPAWN));
    }
}

/// Spawn `DEFLECTOR_COUNT` orbs orbiting `center`, each absorbing
/// `DEFLECTOR_BLOCKS` enemy bullets for 5 s. Shared by the legacy `J` keybind
/// (`cast_deflectors`) and the 4-slot loadout (`systems::abilities`).
pub fn spawn_deflector_orbs(commands: &mut Commands, center: Vec2) {
    for i in 0..DEFLECTOR_COUNT {
        let phase = i as f32 / DEFLECTOR_COUNT as f32 * TAU;
        let pos = center + Vec2::new(phase.cos(), phase.sin()) * DEFLECTOR_RADIUS;
        commands.spawn((
            DeflectorOrb {
                blocks: DEFLECTOR_BLOCKS,
                phase,
            },
            orb_shape(),
            Transform::from_translation(pos.extend(0.6)),
            Collider { radius: 8.0 },
            Lifetime { seconds: 5.0 },
        ));
    }
}

/// Keep deflector orbs circling the ship each tick.
pub fn orbit_deflectors(
    time: Res<Time>,
    player: Query<&Transform, (With<Ship>, Without<DeflectorOrb>)>,
    mut orbs: Query<(&DeflectorOrb, &mut Transform)>,
) {
    let Ok(ptf) = player.single() else {
        return;
    };
    let center = ptf.translation.truncate();
    let t = time.elapsed_secs();
    for (orb, mut tf) in &mut orbs {
        let a = orb.phase + t * DEFLECTOR_OMEGA;
        let pos = center + Vec2::new(a.cos(), a.sin()) * DEFLECTOR_RADIUS;
        tf.translation.x = pos.x;
        tf.translation.y = pos.y;
    }
}

/// Resolve deflector-orb hits: an enemy bullet overlapping an orb is absorbed
/// (despawned), spending one of the orb's blocks; the orb pops at 0.
pub fn deflector_blocks(
    mut commands: Commands,
    mut orbs: Query<(Entity, &Transform, &Collider, &mut DeflectorOrb)>,
    bullets: Query<(Entity, &Transform, &Collider, &Bullet)>,
) {
    for (orb_e, otf, oc, mut orb) in &mut orbs {
        let opos = otf.translation.truncate();
        for (be, btf, bc, bullet) in &bullets {
            if bullet.kind != BulletKind::Enemy {
                continue;
            }
            let reach = oc.radius + bc.radius;
            if opos.distance_squared(btf.translation.truncate()) <= reach * reach {
                commands.entity(be).despawn();
                orb.blocks = orb.blocks.saturating_sub(1);
                if orb.blocks == 0 {
                    commands.entity(orb_e).despawn();
                    break;
                }
            }
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
