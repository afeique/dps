//! Weapons. `player_fire` ticks the cooldown and emits a `Fire` message when
//! the player is firing; `spawn_bullets` turns each `Fire` into a glowing
//! bullet entity. Splitting fire-intent from bullet-spawning keeps weapon
//! logic independent of how projectiles are realized.
//!
//! # Primary weapon varieties
//!
//! Five kinds approximating the JS `PRIMARY_WEAPONS` table in
//! `rainboids/js/modules/combat/weapon-data.js`:
//!
//! | WeaponKind    | JS source          | Pattern                         |
//! |---------------|--------------------|---------------------------------|
//! | Single        | PULSE_CANNON       | 1 forward shot                  |
//! | Twin          | TWIN_CANNON        | 2 shots offset ±6 px laterally  |
//! | TripleSpread  | SCATTER_GUN (3-shot)| 3 shots fanned ±12°            |
//! | Rapid         | STORM_NEEDLES      | 1 shot, half cooldown cadence   |
//! | WideArc       | SCATTER_GUN (5-shot)| 5 shots fanned ±28°           |
//!
//! Simplifications vs the JS source:
//! - Piercing, homing, upgrades, stun/knockback: not yet implemented.
//! - STORM_NEEDLES has random jitter per shot; Rapid instead uses a fixed
//!   half-cooldown cadence with no angular jitter (cleaner feel at this stage).
//! - SCATTER_GUN has `spreadAngle: 0.4 rad` (≈23°) for all 5 pellets; we
//!   expose that as WideArc (±28°, 5 shots) and give TripleSpread a tighter
//!   ±12° for a focused 3-shot feel.
//! - RAIL_DRIVER (slow, high-damage piercing) is not represented yet.
//! - CLUSTER_LAUNCHER requires a separate projectile type; out of scope here.

use crate::components::*;
use crate::messages::Fire;
use crate::render::bullets::BulletAssets;
use bevy::prelude::*;
use bevy_hanabi::prelude::ParticleEffect;

// ─── Weapon variety ──────────────────────────────────────────────────────────

/// The five primary-weapon archetypes selectable at runtime.
#[derive(Clone, Copy, PartialEq, Default)]
pub enum WeaponKind {
    #[default]
    Single,
    Twin,
    TripleSpread,
    Rapid,
    WideArc,
}

impl WeaponKind {
    /// Return the next kind in the cycle (wraps around).
    fn next(self) -> Self {
        match self {
            Self::Single       => Self::Twin,
            Self::Twin         => Self::TripleSpread,
            Self::TripleSpread => Self::Rapid,
            Self::Rapid        => Self::WideArc,
            Self::WideArc      => Self::Single,
        }
    }
}

/// Resource tracking which primary weapon the player currently has active.
/// Registered by the parent (app.rs) via `init_resource::<CurrentWeapon>()`.
#[derive(Resource, Default)]
pub struct CurrentWeapon(pub WeaponKind);

// ─── Helper ──────────────────────────────────────────────────────────────────

/// Rotate a unit (or any) `Vec2` by `angle` radians counter-clockwise.
#[inline]
fn rotate(v: Vec2, angle: f32) -> Vec2 {
    let (s, c) = angle.sin_cos();
    Vec2::new(c * v.x - s * v.y, s * v.x + c * v.y)
}

// ─── Systems ─────────────────────────────────────────────────────────────────

/// Cycle or directly select the active weapon kind.
///
/// - **Tab** / **Q** — step to the next `WeaponKind`.
/// - **Digit1..Digit5** — select directly (1 = Single … 5 = WideArc).
///
/// Registered in Update by the parent (app.rs).
pub fn cycle_weapon(keys: Res<ButtonInput<KeyCode>>, mut cur: ResMut<CurrentWeapon>) {
    // Direct selection via number keys.
    if keys.just_pressed(KeyCode::Digit1) {
        cur.0 = WeaponKind::Single;
    } else if keys.just_pressed(KeyCode::Digit2) {
        cur.0 = WeaponKind::Twin;
    } else if keys.just_pressed(KeyCode::Digit3) {
        cur.0 = WeaponKind::TripleSpread;
    } else if keys.just_pressed(KeyCode::Digit4) {
        cur.0 = WeaponKind::Rapid;
    } else if keys.just_pressed(KeyCode::Digit5) {
        cur.0 = WeaponKind::WideArc;
    } else if keys.just_pressed(KeyCode::Tab) || keys.just_pressed(KeyCode::KeyQ) {
        cur.0 = cur.0.next();
    }
}

/// Tick the player's weapon cooldown; emit a `Fire` pattern while held + ready.
///
/// The number and spread of `Fire` messages emitted depends on `CurrentWeapon`:
///
/// - `Single`       — 1 forward shot.
/// - `Twin`         — 2 shots laterally offset by ±6 px (no angular spread).
/// - `TripleSpread` — 3 shots fanned at 0° and ±12°.
/// - `Rapid`        — 1 forward shot; timer resets to `cooldown * 0.5` for a
///                    faster effective fire rate.
/// - `WideArc`      — 5 shots fanned at 0°, ±14°, and ±28°.
pub fn player_fire(
    time: Res<Time>,
    cur: Res<CurrentWeapon>,
    mut fire: MessageWriter<Fire>,
    mut q: Query<(&Intent, &mut Weapon, &Transform), With<Ship>>,
) {
    let dt = time.delta_secs();
    for (intent, mut weapon, tf) in &mut q {
        weapon.timer = (weapon.timer - dt).max(0.0);
        if !intent.firing || weapon.timer > 0.0 {
            continue;
        }

        // Reset cooldown — Rapid fires at double cadence.
        weapon.timer = match cur.0 {
            WeaponKind::Rapid => weapon.cooldown * 0.5,
            _                 => weapon.cooldown,
        };

        // Muzzle anchor: ship nose (20 px along facing direction).
        let fwd   = (tf.rotation * Vec3::Y).truncate().normalize_or_zero();
        let right = Vec2::new(fwd.y, -fwd.x); // 90° clockwise from fwd
        let nose  = tf.translation.truncate() + fwd * 20.0;

        let dmg   = weapon.damage;
        let spd   = weapon.bullet_speed;

        // Convenience closure — writes one Fire message.
        let mut shoot = |origin: Vec2, dir: Vec2| {
            fire.write(Fire {
                origin,
                dir: dir.normalize_or_zero(),
                damage: dmg,
                speed: spd,
                faction: Faction::Player,
            });
        };

        match cur.0 {
            WeaponKind::Single | WeaponKind::Rapid => {
                shoot(nose, fwd);
            }

            WeaponKind::Twin => {
                // Two parallel shots, offset ±6 px laterally — no angular divergence.
                shoot(nose + right * 6.0, fwd);
                shoot(nose - right * 6.0, fwd);
            }

            WeaponKind::TripleSpread => {
                // Centre shot plus two wings at ±12°.
                shoot(nose, fwd);
                shoot(nose, rotate(fwd,  12_f32.to_radians()));
                shoot(nose, rotate(fwd, -12_f32.to_radians()));
            }

            WeaponKind::WideArc => {
                // Five shots: centre, ±14°, ±28°.
                shoot(nose, fwd);
                shoot(nose, rotate(fwd,  14_f32.to_radians()));
                shoot(nose, rotate(fwd, -14_f32.to_radians()));
                shoot(nose, rotate(fwd,  28_f32.to_radians()));
                shoot(nose, rotate(fwd, -28_f32.to_radians()));
            }
        }
    }
}

/// Spawn one bullet per `Fire` message from the shared `BulletAssets` (one
/// mesh + per-team materials, scaled per shot — no per-shot asset churn).
/// Player shots get a bright white-hot core child + GPU particle trail;
/// enemy shots are magenta.
///
/// **Behavior is unchanged from the original.** Enemy fire (emitted by other
/// systems) also produces `Fire` messages that this function consumes — both
/// factions are handled here.
pub fn spawn_bullets(mut commands: Commands, assets: Res<BulletAssets>, mut fire: MessageReader<Fire>) {
    for shot in fire.read() {
        // Per-team body material + collider radius + lifetime.
        let (kind, radius, body, life) = match shot.faction {
            Faction::Player => (BulletKind::Player, 3.0, assets.player_body.clone(), 1.5),
            Faction::Enemy  => (BulletKind::Enemy,  4.0, assets.enemy_body.clone(),  3.0),
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
            let trail = assets.player_trail.clone();
            bullet.with_children(|b| {
                // local scale 0.5 → core radius = bullet radius * 0.5; z in front.
                b.spawn((
                    Mesh2d(circle),
                    MeshMaterial2d(core),
                    Transform::from_xyz(0.0, 0.0, 0.5).with_scale(Vec3::splat(0.5)),
                ));
                // GPU particle trail.  SimulationSpace::Global keeps emitted
                // particles in world space, so they linger behind the moving
                // bullet as a streak rather than riding along with it.
                b.spawn((ParticleEffect::new(trail), Transform::default()));
            });
        }
    }
}
