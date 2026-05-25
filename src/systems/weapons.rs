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

use crate::combat::element::{Element, ElementSet};
use crate::components::*;
use crate::messages::Fire;
use crate::render::bullets::BulletAssets;
use crate::systems::power_weapon::Homing;
use crate::systems::shop::{homing_turn_rate, overcharge_interval, UpgradeId, Upgrades};
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
    /// VOID slow pull-orb — drags nearby enemies as it flies (W).
    GravityLance,
}

impl WeaponKind {
    fn next(self) -> Self {
        match self {
            Self::PulseCannon => Self::StormNeedles,
            Self::StormNeedles => Self::ScatterShot,
            Self::ScatterShot => Self::RailDriver,
            Self::RailDriver => Self::ClusterLauncher,
            Self::ClusterLauncher => Self::GravityLance,
            Self::GravityLance => Self::PulseCannon,
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
            Self::GravityLance => "Gravity Lance",
        }
    }

    /// The weapon's base element (W; attunements layer on top later). Only
    /// Gravity Lance is non-Kinetic (`WEAPON_ELEMENTS`, weapon-data.js).
    pub fn element(self) -> Element {
        match self {
            Self::GravityLance => Element::Void,
            _ => Element::Kinetic,
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
        // Slow VOID orb that pierces + pulls (weapon-data.js: bulletSpeed 0.6,
        // pierce 6, pull r150 / strength 0.35). The pull rides on `GravityBullet`.
        WeaponKind::GravityLance => WeaponStats {
            cooldown: 0.72, damage: 0.6, speed: BASE_BULLET_SPEED * 0.55,
            radius: BASE_BULLET_RADIUS * 1.15, count: 1, spread: 0.0, jitter: 0.0, pierce: 6,
        },
    }
}

/// Resource tracking the active primary weapon. `init_resource` in app.rs.
#[derive(Resource, Default)]
pub struct CurrentWeapon(pub WeaponKind);

/// The active weapon's equipped attunement elements (W1). Empty = fall back to
/// the weapon's base element. The BUILD tree (gold-unlocked) populates this
/// later; for now a debug key (`T`) cycles a single element so player elemental
/// offense is playable. Read by `spawn_bullets`.
#[derive(Resource, Default)]
pub struct Attunements(pub ElementSet);

/// Resolve a player bullet's element set (`resolveBulletElements` semantics):
/// equipped `attune` elements if any, else the weapon's `base` element. (Element
/// override — Elemental Infusion / Overdrive — layers on top in a later phase.)
pub fn resolve_player_bullet_set(attune: ElementSet, base: Element) -> ElementSet {
    if attune.is_empty() {
        ElementSet::single(base)
    } else {
        attune
    }
}

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

/// `_RAPID` trait: fireRate `×(1−0.12)^stacks` (spec III.2).
#[inline]
pub fn rapid_cooldown_mult(stacks: u32) -> f32 {
    0.88_f32.powi(stacks as i32)
}

/// Damage multiplier on an Overcharge bullet (spec VI.3: ×3).
pub const OVERCHARGE_MULT: f32 = 3.0;

/// Whether the `tally`-th player bullet is overcharged: true when `interval > 0`
/// and `tally` is a multiple of it (every Nth bullet, spec VI.3).
#[inline]
pub fn is_overcharged(tally: u32, interval: u32) -> bool {
    interval > 0 && tally % interval == 0
}

/// `_MULTI` trait fan width for a total projectile `count`: `min(0.8,
/// 0.12*(count−1))` rad (spec III.2); 0 for a single shot.
#[inline]
pub fn multishot_fan(count: u32) -> f32 {
    if count > 1 {
        (0.12 * (count as f32 - 1.0)).min(0.8)
    } else {
        0.0
    }
}

/// Gravity Lance orbs drag enemies within `pull_radius` toward themselves each
/// tick (W). The bullet `&Transform` (immut) and enemy `&mut Transform` are kept
/// disjoint by the `Without` filters (a bullet is never an enemy) → no B0001.
pub fn gravity_pull(
    time: Res<Time>,
    bullets: Query<(&Transform, &GravityBullet), Without<Enemy>>,
    mut enemies: Query<&mut Transform, (With<Enemy>, Without<GravityBullet>)>,
) {
    let dt = time.delta_secs();
    for (btf, gb) in &bullets {
        let center = btf.translation.truncate();
        let r2 = gb.pull_radius * gb.pull_radius;
        for mut etf in &mut enemies {
            let epos = etf.translation.truncate();
            let to = center - epos;
            let d2 = to.length_squared();
            if d2 <= r2 && d2 > 1.0 {
                let step = (to.normalize() * gb.pull_strength * dt).clamp_length_max(to.length());
                etf.translation.x += step.x;
                etf.translation.y += step.y;
            }
        }
    }
}

// ─── Systems ─────────────────────────────────────────────────────────────────

/// The attunement cycle order: off → the 6 non-Kinetic elements → off.
pub const ATTUNE_CYCLE: [Option<Element>; 7] = [
    None,
    Some(Element::Pyro),
    Some(Element::Cryo),
    Some(Element::Volt),
    Some(Element::Toxic),
    Some(Element::Void),
    Some(Element::Radiant),
];

/// The next single-element attunement after the current one (pure; drives the
/// debug cycle + testable without input). Matches by the lone set element.
pub fn next_attunement(current: ElementSet) -> ElementSet {
    let idx = ATTUNE_CYCLE
        .iter()
        .position(|o| match o {
            None => current.is_empty(),
            Some(e) => current.len() == 1 && current.contains(*e),
        })
        .unwrap_or(0);
    match ATTUNE_CYCLE[(idx + 1) % ATTUNE_CYCLE.len()] {
        None => ElementSet::EMPTY,
        Some(e) => ElementSet::single(e),
    }
}

/// DEBUG (until the BUILD tree gold-gates attunements): `T` cycles the active
/// attunement element through the 6 + off, so player elemental offense — and the
/// whole reaction/status engine — is playable now.
pub fn cycle_attunement(keys: Res<ButtonInput<KeyCode>>, mut attune: ResMut<Attunements>) {
    if keys.just_pressed(KeyCode::KeyT) {
        attune.0 = next_attunement(attune.0);
    }
}

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
    upgrades: Res<Upgrades>,
    mut fire: MessageWriter<Fire>,
    mut q: Query<(&Intent, &mut Weapon, &Transform, Option<&Overdrive>), With<Ship>>,
) {
    let dt = time.delta_secs();
    let st = stats(cur.0);
    let t = time.elapsed_secs();

    // Live primary-weapon traits (spec III.2): faster fire + extra fanned shots.
    let rapid = upgrades.owned(UpgradeId::RapidFire);
    let count = st.count + upgrades.owned(UpgradeId::Multishot);
    // The fan is the wider of the weapon's own spread and the `_MULTI` fan.
    let fan = st.spread.max(multishot_fan(count));

    for (intent, mut weapon, tf, overdrive) in &mut q {
        weapon.timer = (weapon.timer - dt).max(0.0);
        if !intent.firing || weapon.timer > 0.0 {
            continue;
        }
        // Overdrive (W): faster fire + harder hits while the buff is active.
        let od = overdrive.is_some();
        let cd_mult = if od { OVERDRIVE_FIRE_MULT } else { 1.0 };
        let dmg = st.damage * if od { OVERDRIVE_DMG_MULT } else { 1.0 };
        weapon.timer = st.cooldown * rapid_cooldown_mult(rapid) * cd_mult;

        let fwd = (tf.rotation * Vec3::Y).truncate().normalize_or_zero();
        let nose = tf.translation.truncate() + fwd * 20.0;

        let shoot = |dir: Vec2, fire: &mut MessageWriter<Fire>| {
            fire.write(Fire {
                origin: nose,
                dir: dir.normalize_or_zero(),
                damage: dmg,
                speed: st.speed,
                faction: Faction::Player,
                homing: false,
            });
        };

        if count <= 1 {
            // Single shot (+ optional cone-of-fire jitter for Storm Needles).
            let j = jitter_rand(t * 91.7) * st.jitter;
            shoot(rotate(fwd, j), &mut fire);
        } else {
            // Even fan across [-fan/2, +fan/2] with small per-pellet jitter.
            let half = fan * 0.5;
            for i in 0..count {
                let f = i as f32 / (count - 1) as f32; // 0..1
                let base = -half + f * fan;
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
    attune: Res<Attunements>,
    upgrades: Res<Upgrades>,
    wave: Res<crate::systems::wave::Wave>,
    // Running count of player bullets fired — drives the Overcharge cadence.
    mut shot_tally: Local<u32>,
    mut fire: MessageReader<Fire>,
) {
    let pst = stats(cur.0);
    // Live primary-weapon traits (spec III.2): +1 pierce / +2.2 px radius per stack
    // / homing (steered by power_weapon::homing_steer toward the nearest enemy).
    let player_pierce = pst.pierce + upgrades.owned(UpgradeId::Piercing);
    let player_radius = pst.radius + 2.2 * upgrades.owned(UpgradeId::BigShot) as f32;
    let player_homing = homing_turn_rate(upgrades.owned(UpgradeId::HomingShot));
    // OVERCHARGE ROUNDS: every Nth player bullet deals ×3 (spec VI.3).
    let oc_interval = overcharge_interval(upgrades.owned(UpgradeId::Overcharge));
    // Enemy bullets speed up across the campaign (spec V.4, normalized to W1=1.0).
    let enemy_speed_mul = crate::systems::enemy::difficulty_bullet_speed_mul(wave.number() as u64);
    for shot in fire.read() {
        let (kind, radius, pierce, body, life, speed_mul) = match shot.faction {
            Faction::Player => (BulletKind::Player, player_radius, player_pierce, assets.player_body.clone(), 1.5, 1.0),
            Faction::Enemy => (BulletKind::Enemy, 4.0, 0, assets.enemy_body.clone(), 3.0, enemy_speed_mul),
        };

        // Overcharge: count player bullets and triple every Nth.
        let mut damage = shot.damage;
        if shot.faction == Faction::Player {
            *shot_tally += 1;
            if is_overcharged(*shot_tally, oc_interval) {
                damage *= OVERCHARGE_MULT;
            }
        }

        let mut bullet = commands.spawn((
            Bullet { kind, damage, pierce },
            Velocity(shot.dir * shot.speed * speed_mul),
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
            // Element tag (W): equipped attunements if any, else the weapon's
            // base element (resolve_player_bullet_set). Resist + reactions are
            // applied in `bullet_hits_enemy` from this set.
            bullet.insert(BulletElements(resolve_player_bullet_set(attune.0, cur.0.element())));
            // Gravity Lance orbs pull nearby enemies as they fly (W).
            if cur.0 == WeaponKind::GravityLance {
                bullet.insert(GravityBullet { pull_radius: 150.0, pull_strength: 60.0 });
            }
            // `_HOMING` trait: tag the bullet so homing_steer curves it.
            if player_homing > 0.0 {
                bullet.insert(Homing { turn_rate: player_homing });
            }
        } else if shot.homing {
            // Raged-boss bullets curve toward the player (spec IV.7 / IV.5).
            bullet.insert(RageHoming {
                turn_rate: crate::systems::enemy::RAGE_HOMING_TURN,
            });
        }
    }
}
