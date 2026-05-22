//! Primary weapons. `player_fire` ticks the active weapon's cooldown and emits
//! `Fire` messages in the weapon's pattern; `spawn_bullets` turns each `Fire`
//! into a bullet entity (and stamps player bullets with the active weapon's
//! radius + piercing). Splitting fire-intent from bullet-spawning keeps weapon
//! logic independent of how projectiles are realized.
//!
//! # The five primaries (port spec III.1, from `combat/weapon-data.js`)
//!
//! | Kind            | fireRate | dmg  | speed× | size× | count | spread | pierce |
//! |-----------------|----------|------|--------|-------|-------|--------|--------|
//! | PulseCannon     | 400 ms   | 1.2  | 1.0    | 1.0   | 1     | 0      | 0      |
//! | StormNeedles    | 130 ms   | 0.4  | 1.1    | 0.5   | 1     | ±0.10  | 0      |
//! | ScatterShot     | 700 ms   | 0.42 | 0.9    | 0.6   | 5     | 0.4    | 0      |
//! | RailDriver      | 1200 ms  | 3.0  | 1.4    | 1.2   | 1     | 0      | 99     |
//! | ClusterLauncher | 800 ms   | 50   | 1.0    | 1.4   | 1     | 0      | 0      |
//!
//! Faithful so far: exact fireRate / damage / speed× / size× / projectile
//! count / spread / piercing, plus Storm Needles' per-shot random "cone of
//! fire" jitter. Deferred (noted in the spec): the 8-trait upgrade economy,
//! Rail's double-helix strands, and Cluster's bomb→sub-bomblet detonation
//! (Cluster currently fires one large high-damage bolt). `speed×`/`size×`
//! multiply dps base units, not the JS px/tick (timestep alignment is a
//! separate increment).

use crate::components::*;
use crate::messages::Fire;
use crate::render::bullets::BulletAssets;
use bevy::prelude::*;
use bevy_hanabi::prelude::ParticleEffect;

// ─── Base units (dps scale; spec multipliers apply on top) ──────────────────

/// Base player bullet speed (world-units/sec). Spec `bulletSpeed×` scales this.
const BASE_BULLET_SPEED: f32 = 950.0;
/// Base player bullet radius (px). Spec `bulletSize×` scales this.
const BASE_BULLET_RADIUS: f32 = 3.0;

// ─── Weapon kinds + per-kind stats ──────────────────────────────────────────

/// The five primary weapons (`PRIMARY_WEAPONS`). Selectable at runtime; the
/// real game gates them behind `unlockWave`, deferred to the shop increment.
#[derive(Clone, Copy, PartialEq, Default)]
pub enum WeaponKind {
    #[default]
    PulseCannon,
    StormNeedles,
    ScatterShot,
    RailDriver,
    ClusterLauncher,
}

impl WeaponKind {
    fn next(self) -> Self {
        match self {
            Self::PulseCannon => Self::StormNeedles,
            Self::StormNeedles => Self::ScatterShot,
            Self::ScatterShot => Self::RailDriver,
            Self::RailDriver => Self::ClusterLauncher,
            Self::ClusterLauncher => Self::PulseCannon,
        }
    }

    /// Human-readable name (HUD).
    pub fn name(self) -> &'static str {
        match self {
            Self::PulseCannon => "Pulse Cannon",
            Self::StormNeedles => "Storm Needles",
            Self::ScatterShot => "Scatter Shot",
            Self::RailDriver => "Rail Driver",
            Self::ClusterLauncher => "Cluster Launcher",
        }
    }
}

/// Resolved per-shot stats for a weapon kind.
struct WeaponStats {
    /// Seconds between shots (fireRate ms / 1000).
    cooldown: f32,
    /// Damage per bullet.
    damage: f32,
    /// Bullet speed, world-units/sec.
    speed: f32,
    /// Bullet collision/visual radius, px.
    radius: f32,
    /// Projectiles per shot.
    count: u32,
    /// Total fan width (radians) across which `count` shots are evenly spread.
    spread: f32,
    /// Per-shot random angular jitter (radians) — Storm Needles' cone of fire.
    jitter: f32,
    /// Extra targets each bullet passes through (0 = dies on first hit).
    pierce: u32,
}

/// Per-kind stats (port spec III.1).
fn stats(kind: WeaponKind) -> WeaponStats {
    match kind {
        WeaponKind::PulseCannon => WeaponStats {
            cooldown: 0.40, damage: 1.2, speed: BASE_BULLET_SPEED * 1.0,
            radius: BASE_BULLET_RADIUS * 1.0, count: 1, spread: 0.0, jitter: 0.0, pierce: 0,
        },
        WeaponKind::StormNeedles => WeaponStats {
            cooldown: 0.13, damage: 0.4, speed: BASE_BULLET_SPEED * 1.1,
            radius: BASE_BULLET_RADIUS * 0.5, count: 1, spread: 0.0, jitter: 0.10, pierce: 0,
        },
        WeaponKind::ScatterShot => WeaponStats {
            cooldown: 0.70, damage: 0.42, speed: BASE_BULLET_SPEED * 0.9,
            radius: BASE_BULLET_RADIUS * 0.6, count: 5, spread: 0.4, jitter: 0.025, pierce: 0,
        },
        WeaponKind::RailDriver => WeaponStats {
            cooldown: 1.20, damage: 3.0, speed: BASE_BULLET_SPEED * 1.4,
            radius: BASE_BULLET_RADIUS * 1.2, count: 1, spread: 0.0, jitter: 0.0, pierce: 99,
        },
        WeaponKind::ClusterLauncher => WeaponStats {
            cooldown: 0.80, damage: 50.0, speed: BASE_BULLET_SPEED * 1.0,
            radius: BASE_BULLET_RADIUS * 1.4, count: 1, spread: 0.0, jitter: 0.0, pierce: 0,
        },
    }
}

/// Resource tracking the active primary weapon. `init_resource` in app.rs.
#[derive(Resource, Default)]
pub struct CurrentWeapon(pub WeaponKind);

// ─── Helpers ────────────────────────────────────────────────────────────────

#[inline]
fn rotate(v: Vec2, angle: f32) -> Vec2 {
    let (s, c) = angle.sin_cos();
    Vec2::new(c * v.x - s * v.y, s * v.x + c * v.y)
}

/// Cheap unseeded pseudo-random in [-1, 1) from a float seed (matches the JS's
/// non-deterministic `Math.random` jitter; gameplay RNG isn't seeded — spec I.3).
#[inline]
fn jitter_rand(seed: f32) -> f32 {
    let r = (seed.sin() * 43758.5453).fract(); // [0, 1)
    r * 2.0 - 1.0
}

// ─── Systems ─────────────────────────────────────────────────────────────────

/// Cycle (Tab / Q) or directly select (1–5) the active weapon.
pub fn cycle_weapon(keys: Res<ButtonInput<KeyCode>>, mut cur: ResMut<CurrentWeapon>) {
    if keys.just_pressed(KeyCode::Digit1) {
        cur.0 = WeaponKind::PulseCannon;
    } else if keys.just_pressed(KeyCode::Digit2) {
        cur.0 = WeaponKind::StormNeedles;
    } else if keys.just_pressed(KeyCode::Digit3) {
        cur.0 = WeaponKind::ScatterShot;
    } else if keys.just_pressed(KeyCode::Digit4) {
        cur.0 = WeaponKind::RailDriver;
    } else if keys.just_pressed(KeyCode::Digit5) {
        cur.0 = WeaponKind::ClusterLauncher;
    } else if keys.just_pressed(KeyCode::Tab) || keys.just_pressed(KeyCode::KeyQ) {
        cur.0 = cur.0.next();
    }
}

/// Tick the active weapon's cooldown; emit its `Fire` pattern while held.
pub fn player_fire(
    time: Res<Time>,
    cur: Res<CurrentWeapon>,
    mut fire: MessageWriter<Fire>,
    mut q: Query<(&Intent, &mut Weapon, &Transform), With<Ship>>,
) {
    let dt = time.delta_secs();
    let st = stats(cur.0);
    let t = time.elapsed_secs();

    for (intent, mut weapon, tf) in &mut q {
        weapon.timer = (weapon.timer - dt).max(0.0);
        if !intent.firing || weapon.timer > 0.0 {
            continue;
        }
        weapon.timer = st.cooldown;

        let fwd = (tf.rotation * Vec3::Y).truncate().normalize_or_zero();
        let nose = tf.translation.truncate() + fwd * 20.0;

        let shoot = |dir: Vec2, fire: &mut MessageWriter<Fire>| {
            fire.write(Fire {
                origin: nose,
                dir: dir.normalize_or_zero(),
                damage: st.damage,
                speed: st.speed,
                faction: Faction::Player,
            });
        };

        if st.count <= 1 {
            // Single shot (+ optional cone-of-fire jitter for Storm Needles).
            let j = jitter_rand(t * 91.7) * st.jitter;
            shoot(rotate(fwd, j), &mut fire);
        } else {
            // Even fan across [-spread/2, +spread/2] with small per-pellet jitter.
            let n = st.count;
            let half = st.spread * 0.5;
            for i in 0..n {
                let f = i as f32 / (n - 1) as f32; // 0..1
                let base = -half + f * st.spread;
                let j = jitter_rand(t * 53.3 + i as f32 * 12.9) * st.jitter;
                shoot(rotate(fwd, base + j), &mut fire);
            }
        }
    }
}

/// Spawn one bullet per `Fire`. Player bullets take the active weapon's radius
/// + piercing (looked up here, so the `Fire` message stays weapon-agnostic and
/// enemy fire is unaffected); enemy bullets keep the magenta faction defaults.
pub fn spawn_bullets(
    mut commands: Commands,
    assets: Res<BulletAssets>,
    cur: Res<CurrentWeapon>,
    mut fire: MessageReader<Fire>,
) {
    let pst = stats(cur.0);
    for shot in fire.read() {
        let (kind, radius, pierce, body, life) = match shot.faction {
            Faction::Player => (BulletKind::Player, pst.radius, pst.pierce, assets.player_body.clone(), 1.5),
            Faction::Enemy => (BulletKind::Enemy, 4.0, 0, assets.enemy_body.clone(), 3.0),
        };

        let mut bullet = commands.spawn((
            Bullet { kind, damage: shot.damage, pierce },
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
                b.spawn((
                    Mesh2d(circle),
                    MeshMaterial2d(core),
                    Transform::from_xyz(0.0, 0.0, 0.5).with_scale(Vec3::splat(0.5)),
                ));
                b.spawn((ParticleEffect::new(trail), Transform::default()));
            });
        }
    }
}
