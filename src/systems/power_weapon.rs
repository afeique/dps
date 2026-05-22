//! Power weapons (spec III.3) — energy-gated secondary fire.
//!
//! `KeyE` / gamepad **West** fires the active power weapon; `KeyF` cycles which
//! one. Each shot spends energy (built +4 per landed hit, cap 100 — see
//! `EnergyMeter`) and starts a short anti-spam cooldown floor.
//!
//! Ported so far (both fire `Bullet`s, so the existing `bullet_hits_enemy`
//! handles damage — no new collision systems):
//!   * **MissileSalvo** — 3 homing missiles fanned ±25° (cost 55).
//!   * **ChargeShot**   — one big fast piercing bolt (cost 20). The JS hold-to-
//!                        charge timing is simplified to an instant heavy shot.
//!
//! Still to port (need new entity types + collision passes): Nova Blast, Mine
//! Layer, Lance Beam, Arc Lightning — subsequent commits.
//!
//! Public surface for `app.rs`: resources `PowerWeapon`; systems
//! `fire_power_weapon`, `cycle_power_weapon`, `homing_steer`, `reset_energy`.

use crate::components::*;
use crate::messages::Damage;
use crate::resources::EnergyMeter;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

// ─── Power-weapon kinds ──────────────────────────────────────────────────────

/// The six power weapons (`POWER_WEAPONS`). Only the two bullet-based kinds are
/// implemented yet; the rest cycle through but fall back to a basic shot until
/// their dedicated mechanics land.
#[derive(Clone, Copy, PartialEq, Default, Debug)]
pub enum PowerWeaponKind {
    #[default]
    MissileSalvo,
    ChargeShot,
    NovaBlast,
    MineLayer,
    LanceBeam,
    ArcLightning,
}

impl PowerWeaponKind {
    fn next(self) -> Self {
        match self {
            Self::MissileSalvo => Self::ChargeShot,
            Self::ChargeShot => Self::NovaBlast,
            Self::NovaBlast => Self::MineLayer,
            Self::MineLayer => Self::LanceBeam,
            Self::LanceBeam => Self::ArcLightning,
            Self::ArcLightning => Self::MissileSalvo,
        }
    }

    /// Energy cost per fire (`POWER_ENERGY_COST`, spec III.3).
    fn energy_cost(self) -> f32 {
        match self {
            Self::ChargeShot => 20.0,
            Self::MineLayer => 25.0,
            Self::ArcLightning => 30.0,
            Self::NovaBlast => 45.0,
            Self::MissileSalvo => 55.0,
            Self::LanceBeam => 60.0,
        }
    }

    /// Anti-spam cooldown floor (seconds).
    fn cooldown(self) -> f32 {
        match self {
            Self::ChargeShot => 0.30,
            Self::MissileSalvo => 0.80,
            Self::NovaBlast => 1.00,
            Self::MineLayer => 0.50,
            Self::LanceBeam => 0.80,
            Self::ArcLightning => 0.80,
        }
    }
}

/// Active power weapon + its cooldown countdown. (`init_resource` in app.rs.)
#[derive(Resource, Default)]
pub struct PowerWeapon {
    pub kind: PowerWeaponKind,
    pub cooldown: f32,
}

// ─── Components ───────────────────────────────────────────────────────────────

/// Marks a homing missile; carries its max angular turn rate (rad/sec).
#[derive(Component)]
pub struct Homing {
    pub turn_rate: f32,
}

/// A seeker mine: arms after a short delay, detonates (AoE) when an enemy
/// enters its trigger radius or its lifetime lapses (spec III.3, simplified —
/// no homing/magnetic pull or turret fire yet).
#[derive(Component)]
pub struct Mine {
    /// Counts down to 0; the mine is inert until armed.
    arm_timer: f32,
    /// Enemy within this distance triggers detonation.
    trigger_radius: f32,
    /// Detonation damages all enemies within this distance.
    blast_radius: f32,
    damage: f32,
    /// Self-detonation lifetime.
    life: f32,
}

/// An expanding Nova Blast shockwave ring. Damages each enemy once as the ring
/// front sweeps over it, then despawns at `max_radius` (spec III.3).
#[derive(Component)]
pub struct NovaRing {
    center: Vec2,
    radius: f32,
    max_radius: f32,
    /// Expansion rate, world-units/sec.
    speed: f32,
    damage: f32,
    /// Half-width of the damaging front (`RING_WIDTH`).
    band: f32,
    /// Enemies already hit by this ring (no double-damage).
    hit: Vec<Entity>,
}

// ─── Shapes ───────────────────────────────────────────────────────────────────

/// A sharp dart silhouette for missiles (nose-up, +Y forward). HDR-emissive hot
/// orange so bloom gives it a fiery glow.
pub fn missile_shape() -> Shape {
    let nose = Vec2::new(0.0, 8.0);
    let right = Vec2::new(3.0, -2.0);
    let tail = Vec2::new(0.0, -5.0);
    let left = Vec2::new(-3.0, -2.0);
    let path = ShapePath::new()
        .move_to(nose)
        .line_to(right)
        .line_to(tail)
        .line_to(left)
        .close();
    ShapeBuilder::with(&path)
        .fill(Color::linear_rgb(0.08, 0.02, 0.0))
        .stroke((Color::linear_rgb(9.0, 3.0, 0.5), 1.5))
        .build()
}

/// A big bright cyan bolt for Charge Shot (and the placeholder power shots): an
/// HDR-emissive octagon so bloom gives it a hot glow.
pub fn charge_shape() -> Shape {
    let r = 9.0_f32;
    let mut path = ShapePath::new();
    for i in 0..8 {
        let a = i as f32 / 8.0 * std::f32::consts::TAU;
        let p = Vec2::new(a.cos() * r, a.sin() * r);
        if i == 0 {
            path = path.move_to(p);
        } else {
            path = path.line_to(p);
        }
    }
    ShapeBuilder::with(&path.close())
        .fill(Color::linear_rgb(0.6, 4.0, 5.0))
        .stroke((Color::linear_rgb(2.0, 9.0, 9.0), 2.0))
        .build()
}

/// A small spiky mine silhouette (HDR red so bloom gives it a warning glow).
pub fn mine_shape() -> Shape {
    let r = 7.0_f32;
    let spike = 12.0_f32;
    let mut path = ShapePath::new();
    for i in 0..8 {
        let a = i as f32 / 8.0 * std::f32::consts::TAU;
        let inner = Vec2::new(a.cos() * r, a.sin() * r);
        let mid = (a + std::f32::consts::TAU / 16.0).rem_euclid(std::f32::consts::TAU);
        let tip = Vec2::new(mid.cos() * spike, mid.sin() * spike);
        if i == 0 {
            path = path.move_to(inner);
        } else {
            path = path.line_to(inner);
        }
        path = path.line_to(tip);
    }
    ShapeBuilder::with(&path.close())
        .fill(Color::linear_rgb(0.4, 0.02, 0.0))
        .stroke((Color::linear_rgb(9.0, 1.0, 0.2), 1.5))
        .build()
}

/// Spawn a seeker mine at `pos` (used by the fire path + tests).
pub fn lay_mine(commands: &mut Commands, pos: Vec2) {
    commands.spawn((
        Mine {
            arm_timer: 0.6,
            trigger_radius: 60.0,
            blast_radius: 90.0,
            damage: 30.0,
            life: 12.0,
        },
        mine_shape(),
        Transform::from_translation(pos.extend(0.4)),
    ));
}

/// A unit-radius ring (no fill) for the Nova shockwave; scaled by the ring's
/// live radius each frame. Stroke is thin at unit scale so the scaled-up ring
/// stays a few px wide. HDR-emissive amber → bloom halo.
pub fn nova_ring_shape() -> Shape {
    let r = 1.0_f32;
    let mut path = ShapePath::new();
    let segs = 48;
    for i in 0..segs {
        let a = i as f32 / segs as f32 * std::f32::consts::TAU;
        let p = Vec2::new(a.cos() * r, a.sin() * r);
        if i == 0 {
            path = path.move_to(p);
        } else {
            path = path.line_to(p);
        }
    }
    ShapeBuilder::with(&path.close())
        .stroke((Color::linear_rgb(9.0, 5.0, 1.0), 0.02))
        .build()
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

#[inline]
fn rotate(v: Vec2, angle: f32) -> Vec2 {
    let (s, c) = angle.sin_cos();
    Vec2::new(c * v.x - s * v.y, s * v.x + c * v.y)
}

/// Steer `current` velocity toward `desired_dir` by at most `max_angle` rad,
/// preserving speed.
#[inline]
fn steer_toward(current: Vec2, desired_dir: Vec2, max_angle: f32) -> Vec2 {
    let speed = current.length();
    if speed < 1e-6 {
        return current;
    }
    let cur_dir = current / speed;
    let angle = {
        let cross = cur_dir.x * desired_dir.y - cur_dir.y * desired_dir.x;
        let dot = cur_dir.dot(desired_dir);
        cross.atan2(dot)
    };
    rotate(cur_dir, angle.clamp(-max_angle, max_angle)) * speed
}

// ─── Systems ─────────────────────────────────────────────────────────────────

/// Reset energy to 0 at the start of a run (`OnEnter(Playing)`).
pub fn reset_energy(mut energy: ResMut<EnergyMeter>, mut pw: ResMut<PowerWeapon>) {
    energy.current = 0.0;
    pw.cooldown = 0.0;
}

/// Cycle the active power weapon with `KeyF`.
pub fn cycle_power_weapon(keys: Res<ButtonInput<KeyCode>>, mut pw: ResMut<PowerWeapon>) {
    if keys.just_pressed(KeyCode::KeyF) {
        pw.kind = pw.kind.next();
    }
}

/// Tick cooldown; on `KeyE` / West, if energy ≥ cost, spend it and fire the
/// active power weapon.
pub fn fire_power_weapon(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    time: Res<Time>,
    mut pw: ResMut<PowerWeapon>,
    mut energy: ResMut<EnergyMeter>,
    mut commands: Commands,
    player: Query<&Transform, With<Ship>>,
) {
    pw.cooldown = (pw.cooldown - time.delta_secs()).max(0.0);

    let pad_fire = gamepads.iter().any(|gp| gp.just_pressed(GamepadButton::West));
    if !keys.just_pressed(KeyCode::KeyE) && !pad_fire {
        return;
    }
    if pw.cooldown > 0.0 {
        return;
    }

    let Ok(tf) = player.single() else {
        return;
    };

    let cost = pw.kind.energy_cost();
    if !energy.try_spend(cost) {
        return; // not enough energy
    }
    pw.cooldown = pw.kind.cooldown();

    let fwd = (tf.rotation * Vec3::Y).truncate().normalize_or_zero();
    let nose = tf.translation.truncate() + fwd * 22.0;

    match pw.kind {
        PowerWeaponKind::MissileSalvo => {
            const SPREAD: f32 = 25.0 * std::f32::consts::PI / 180.0;
            for offset in [-SPREAD, 0.0, SPREAD] {
                let dir = rotate(fwd, offset);
                commands.spawn((
                    Bullet { kind: BulletKind::Player, damage: 22.0, pierce: 0 },
                    Velocity(dir * 420.0),
                    Collider { radius: 5.0 },
                    Faction::Player,
                    Homing { turn_rate: 4.0 },
                    Lifetime { seconds: 2.5 },
                    missile_shape(),
                    Transform::from_translation(nose.extend(1.0)),
                ));
            }
        }
        PowerWeaponKind::NovaBlast => {
            let center = tf.translation.truncate();
            commands.spawn((
                NovaRing {
                    center,
                    radius: 0.0,
                    max_radius: 320.0,
                    speed: 533.0, // 320 px over the ~0.6 s ring duration
                    damage: 8.0,
                    band: 30.0,
                    hit: Vec::new(),
                },
                nova_ring_shape(),
                Transform::from_translation(center.extend(0.5)).with_scale(Vec3::splat(0.001)),
            ));
        }
        PowerWeaponKind::MineLayer => {
            lay_mine(&mut commands, tf.translation.truncate());
        }
        // ChargeShot (and, for now, Lance/Arc) fire one big fast piercing bolt
        // so every power weapon does *something* while its bespoke mechanic is
        // pending.
        _ => {
            commands.spawn((
                Bullet { kind: BulletKind::Player, damage: 60.0, pierce: 3 },
                Velocity(fwd * 1100.0),
                Collider { radius: 9.0 },
                Faction::Player,
                Lifetime { seconds: 1.5 },
                charge_shape(),
                Transform::from_translation(nose.extend(1.0)),
            ));
        }
    }
}

/// Tick mines: arm them, detonate (AoE) on an enemy entering the trigger
/// radius or on lifetime end, and despawn. Runs in the FixedUpdate collision
/// group so detonation `Damage` reaches `apply_damage`.
pub fn update_mines(
    time: Res<Time>,
    mut commands: Commands,
    mut dmg: MessageWriter<Damage>,
    enemies: Query<(Entity, &Transform, &Collider), With<Enemy>>,
    mut mines: Query<(Entity, &mut Mine, &Transform)>,
) {
    let dt = time.delta_secs();
    for (mine_e, mut mine, mtf) in &mut mines {
        mine.arm_timer = (mine.arm_timer - dt).max(0.0);
        mine.life -= dt;
        let pos = mtf.translation.truncate();
        let armed = mine.arm_timer <= 0.0;
        if !armed {
            continue;
        }

        let triggered = enemies
            .iter()
            .any(|(_, etf, ec)| etf.translation.truncate().distance(pos) <= mine.trigger_radius + ec.radius);

        if triggered || mine.life <= 0.0 {
            // Detonate: damage everything in the blast radius.
            for (e, etf, ec) in &enemies {
                if etf.translation.truncate().distance(pos) <= mine.blast_radius + ec.radius {
                    dmg.write(Damage { target: e, amount: mine.damage });
                }
            }
            commands.entity(mine_e).despawn();
        }
    }
}

/// Does an enemy disc (`enemy_pos`, `enemy_r`) overlap a Nova ring's damaging
/// front — the annulus `[radius−band, radius+band]` around `center`?
pub fn nova_band_hits(center: Vec2, radius: f32, band: f32, enemy_pos: Vec2, enemy_r: f32) -> bool {
    let d = enemy_pos.distance(center);
    d + enemy_r >= radius - band && d - enemy_r <= radius + band
}

/// Expand each Nova ring, damage enemies its front sweeps over (once each), and
/// despawn it at full radius. Runs in the FixedUpdate collision group so its
/// `Damage` messages reach `apply_damage`.
pub fn update_nova(
    time: Res<Time>,
    mut commands: Commands,
    mut dmg: MessageWriter<Damage>,
    enemies: Query<(Entity, &Transform, &Collider), With<Enemy>>,
    mut rings: Query<(Entity, &mut NovaRing, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (ring_e, mut ring, mut tf) in &mut rings {
        ring.radius += ring.speed * dt;

        for (e, etf, ec) in &enemies {
            if ring.hit.contains(&e) {
                continue;
            }
            // The enemy's disc overlaps the damaging front band.
            if nova_band_hits(ring.center, ring.radius, ring.band, etf.translation.truncate(), ec.radius) {
                dmg.write(Damage { target: e, amount: ring.damage });
                ring.hit.push(e);
            }
        }

        tf.translation = ring.center.extend(0.5);
        tf.scale = Vec3::splat(ring.radius.max(0.001));

        if ring.radius >= ring.max_radius {
            commands.entity(ring_e).despawn();
        }
    }
}

/// Curve homing missiles toward the nearest living enemy each frame.
pub fn homing_steer(
    time: Res<Time>,
    enemies: Query<&Transform, With<Enemy>>,
    mut missiles: Query<(&mut Velocity, &Transform, &Homing)>,
) {
    let dt = time.delta_secs();
    for (mut vel, mtf, homing) in &mut missiles {
        let origin = mtf.translation.truncate();
        let target = enemies
            .iter()
            .map(|etf| etf.translation.truncate())
            .min_by(|a, b| {
                a.distance_squared(origin)
                    .partial_cmp(&b.distance_squared(origin))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        let Some(target_pos) = target else { continue };
        let to_target = (target_pos - origin).normalize_or_zero();
        if to_target == Vec2::ZERO {
            continue;
        }
        vel.0 = steer_toward(vel.0, to_target, homing.turn_rate * dt);
    }
}
