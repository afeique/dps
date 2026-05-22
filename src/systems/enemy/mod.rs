//! Per-enemy modules (mirrors `js/modules/enemy/*`). Each kind module exposes
//! `shape()` (lyon silhouette), `stats()` (base stats from `enemy-data.js`),
//! and `ai()` (a FixedUpdate movement system filtering its own kind). `spawn`
//! dispatches on `EnemyKind` to build the entity. Generic aimed firing lives in
//! `systems::enemy_fire`; per-kind fire patterns are a later refinement.
//!
//! Ported so far: Drifter (in `render::shapes` + `systems::enemy_ai`), Hunter,
//! Guardian, Wasp. Stalker / Prowler / Weaver / Sentinel / Tangerine / Titan
//! are subsequent increments; they fall back to the drifter visual for now.

pub mod firing;
pub mod guardian;
pub mod hunter;
pub mod prowler;
pub mod sentinel;
pub mod stalker;
pub mod tangerine;
pub mod titan;
pub mod wasp;
pub mod weaver;

use crate::components::*;
use crate::messages::{Death, Fire};
use crate::render::shapes;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::Shape;

/// Base stats for an enemy kind (mapped from `enemy-data.js`). `speed` is in
/// world-units/second; `fire_cooldown` is `Some(seconds)` if the kind fires.
#[derive(Clone, Copy)]
pub struct EnemyStats {
    pub health: f32,
    pub radius: f32,
    pub speed: f32,
    pub fire_cooldown: Option<f32>,
}

fn shape_for(kind: EnemyKind) -> Shape {
    match kind {
        EnemyKind::Hunter => hunter::shape(),
        EnemyKind::Guardian => guardian::shape(),
        EnemyKind::Wasp => wasp::shape(),
        EnemyKind::Stalker => stalker::shape(),
        EnemyKind::Prowler => prowler::shape(),
        EnemyKind::Weaver => weaver::shape(),
        EnemyKind::Sentinel => sentinel::shape(),
        EnemyKind::Tangerine => tangerine::shape(),
        EnemyKind::Titan => titan::shape(),
        EnemyKind::Drifter => shapes::drifter_star(18.0),
    }
}

fn stats_for(kind: EnemyKind) -> EnemyStats {
    match kind {
        EnemyKind::Hunter => hunter::stats(),
        EnemyKind::Guardian => guardian::stats(),
        EnemyKind::Wasp => wasp::stats(),
        EnemyKind::Stalker => stalker::stats(),
        EnemyKind::Prowler => prowler::stats(),
        EnemyKind::Weaver => weaver::stats(),
        EnemyKind::Sentinel => sentinel::stats(),
        EnemyKind::Tangerine => tangerine::stats(),
        EnemyKind::Titan => titan::stats(),
        EnemyKind::Drifter => EnemyStats {
            // Spec roster: Drifter HP 9, radius 38 (the lyon silhouette is built
            // at 18 px; collider matches the original visual for now).
            health: 9.0,
            radius: 18.0,
            speed: 60.0,
            fire_cooldown: Some(1.4),
        },
    }
}

/// Activate boss rage on `e` (spec IV.7): a 1.5 s invulnerability window, a
/// permanent ×0.66 fire cooldown, and an immediate 16-bullet circular tantrum
/// from `pos`. Shared by the HP-threshold trigger (`boss_rage`) and the
/// pair-link trigger (`boss_pair_rage`).
fn activate_rage(
    commands: &mut Commands,
    fire: &mut MessageWriter<Fire>,
    e: Entity,
    pos: Vec2,
    fc: Option<Mut<FireCooldown>>,
) {
    commands
        .entity(e)
        .insert(Raged)
        .insert(Invulnerable { seconds: 1.5 });
    if let Some(mut fc) = fc {
        fc.cooldown *= 0.66;
        fc.timer = 0.0; // fire again immediately
    }
    for i in 0..16 {
        let a = i as f32 / 16.0 * std::f32::consts::TAU;
        let dir = Vec2::new(a.cos(), a.sin());
        fire.write(Fire {
            origin: pos + dir * 24.0,
            dir,
            damage: 3.0,
            speed: 280.0,
            faction: Faction::Enemy,
        });
    }
}

/// HP-threshold boss rage (spec IV.7, one-shot): a boss rages when it drops to
/// ≤33% of its max HP. (Deferred from the spec: the 24-frame telegraph, homing
/// bullets, screen flash/shake, the red aura, and tier-3+ formations.)
pub fn boss_rage(
    mut commands: Commands,
    mut fire: MessageWriter<Fire>,
    mut bosses: Query<
        (Entity, &Transform, &Health, Option<&mut FireCooldown>),
        (With<Boss>, Without<Raged>),
    >,
) {
    for (e, tf, hp, fc) in &mut bosses {
        if hp.current > hp.max * 0.33 {
            continue;
        }
        activate_rage(&mut commands, &mut fire, e, tf.translation.truncate(), fc);
    }
}

/// Boss-pair rage link (spec IV.7, tier 2): when *any* boss dies, every
/// surviving un-raged boss immediately rages.
pub fn boss_pair_rage(
    mut deaths: MessageReader<Death>,
    mut commands: Commands,
    mut fire: MessageWriter<Fire>,
    mut bosses: Query<(Entity, &Transform, Option<&mut FireCooldown>), (With<Boss>, Without<Raged>)>,
) {
    if !deaths.read().any(|d| d.boss_tier > 0) {
        return;
    }
    for (e, tf, fc) in &mut bosses {
        activate_rage(&mut commands, &mut fire, e, tf.translation.truncate(), fc);
    }
}

/// Roster point value for destroying this kind (spec IV.1 "Points" column).
pub fn points(kind: EnemyKind) -> u64 {
    match kind {
        EnemyKind::Hunter => 120,
        EnemyKind::Guardian => 200,
        EnemyKind::Wasp => 100,
        EnemyKind::Stalker => 130,
        EnemyKind::Drifter => 180,
        EnemyKind::Prowler => 240,
        EnemyKind::Weaver => 160,
        EnemyKind::Sentinel => 220,
        EnemyKind::Tangerine => 160,
        EnemyKind::Titan => 320,
    }
}

/// Per-tier boss point value (spec IV.7 — overrides the base roster points).
pub fn boss_points(tier: u8) -> u64 {
    match tier {
        1 => 500,
        2 => 1000,
        3 => 1750,
        4 => 3000,
        _ => 0,
    }
}

/// `ENEMY_DROP_PROFILES` gold-budget multiplier (spec VI.5). A boss overrides
/// the per-kind profile with the 2.4× boss budget.
pub fn drop_budget_mul(kind: EnemyKind, boss: bool) -> f32 {
    if boss {
        return 2.4; // boss profile
    }
    match kind {
        // grunt
        EnemyKind::Hunter | EnemyKind::Wasp | EnemyKind::Stalker => 0.75,
        // standard
        EnemyKind::Drifter | EnemyKind::Weaver | EnemyKind::Tangerine => 1.0,
        // tanky
        EnemyKind::Guardian | EnemyKind::Prowler | EnemyKind::Sentinel => 1.4,
        // miniboss
        EnemyKind::Titan => 1.8,
    }
}

/// Boss-tier stat multipliers `(hp, size, speed)` from `BOSS_TIER_STATS`
/// (port spec IV.7). Tier 0 = a normal (non-boss) enemy.
pub fn boss_tier_mul(tier: u8) -> (f32, f32, f32) {
    match tier {
        1 => (4.0, 1.35, 1.00),
        2 => (5.0, 1.45, 1.05),
        3 => (6.0, 1.55, 1.10),
        4 => (8.0, 1.75, 1.15),
        _ => (1.0, 1.0, 1.0),
    }
}

/// Mini-boss HP / size overlay (spec V.6: HP×1.7, radius×1.25).
const MINI_BOSS_HP_MUL: f32 = 1.7;
const MINI_BOSS_SZ_MUL: f32 = 1.25;

/// Mid-wave mini-boss promotion chance for a group at `wave`
/// (spec V.6: `min(0.45, 0.06 + (wave−4)*0.025)`, 0 below wave 4).
pub fn mini_boss_chance(wave: u64) -> f32 {
    if wave < 4 {
        0.0
    } else {
        (0.06 + (wave - 4) as f32 * 0.025).min(0.45)
    }
}

/// Campaign difficulty progress for a wave: `t = (w−1)/29`, clamped to `[0,1]`.
fn difficulty_t(wave: u64) -> f32 {
    (((wave.max(1) - 1) as f32) / 29.0).clamp(0.0, 1.0)
}

/// Campaign-wide enemy HP multiplier by wave (spec V.4): `1 + t*8 + t^2.5*6.5`
/// (W1 1.0× → W30 15.5×).
pub fn difficulty_hp_mul(wave: u64) -> f32 {
    let t = difficulty_t(wave);
    1.0 + t * 8.0 + t.powf(2.5) * 6.5
}

/// Campaign-wide enemy point multiplier by wave (spec V.4): `1 + t^1.4*5.5`
/// (W1 1.0× → W30 6.5×).
pub fn difficulty_points_mul(wave: u64) -> f32 {
    let t = difficulty_t(wave);
    1.0 + t.powf(1.4) * 5.5
}

/// Campaign-wide enemy *movement*-speed multiplier by wave (spec V.4
/// `0.55 + t^1.5*1.2`), **normalized to W1 = 1.0** (the port's per-kind speeds
/// are the W1-effective values) → W30 ≈ 3.18×.
pub fn difficulty_speed_mul(wave: u64) -> f32 {
    let t = difficulty_t(wave);
    (0.55 + t.powf(1.5) * 1.2) / 0.55
}

/// Campaign-wide enemy *bullet*-speed multiplier by wave (spec V.4 campaignMul
/// `1.15 + t^1.4*1.9`), **normalized to W1 = 1.0** — the port's per-kind bullet
/// speeds are already the W1-effective values, so only the relative ramp
/// applies (W1 1.0× → W30 ≈2.65×).
pub fn difficulty_bullet_speed_mul(wave: u64) -> f32 {
    let t = difficulty_t(wave);
    (1.15 + t.powf(1.4) * 1.9) / 1.15
}

/// What promotion (if any) an enemy spawns with.
#[derive(Clone, Copy)]
enum Promo {
    None,
    Boss(u8),
    Mini,
}

/// Spawn one enemy of `kind` at world position `pos` (non-boss).
pub fn spawn(commands: &mut Commands, kind: EnemyKind, pos: Vec2) {
    spawn_tiered(commands, kind, pos, 0);
}

/// Spawn one enemy, applying a boss-tier HP/size overlay when `tier > 0`.
/// (Speed-scaling is a follow-up — AI reads a fixed per-kind `stats().speed`,
/// so it needs an entity-stored multiplier, tracked separately.)
pub fn spawn_tiered(commands: &mut Commands, kind: EnemyKind, pos: Vec2, tier: u8) {
    let (hp_mul, sz_mul, sp_mul) = boss_tier_mul(tier);
    let promo = if tier > 0 { Promo::Boss(tier) } else { Promo::None };
    spawn_enemy(commands, kind, pos, hp_mul, sz_mul, sp_mul, promo);
}

/// Spawn a mid-wave **mini-boss** promotion of `kind` (spec V.6 — no speed buff).
pub fn spawn_mini_boss(commands: &mut Commands, kind: EnemyKind, pos: Vec2) {
    spawn_enemy(
        commands,
        kind,
        pos,
        MINI_BOSS_HP_MUL,
        MINI_BOSS_SZ_MUL,
        1.0,
        Promo::Mini,
    );
}

/// Wave-aware spawn used by the campaign: applies the V.4 HP difficulty curve on
/// top of the tier / mini-boss HP overlay. `mini` promotes to a mini-boss
/// (ignoring `tier`); otherwise `tier` selects the boss overlay (0 = normal).
pub fn spawn_for_wave(
    commands: &mut Commands,
    kind: EnemyKind,
    pos: Vec2,
    tier: u8,
    mini: bool,
    wave: u64,
) {
    let diff_hp = difficulty_hp_mul(wave);
    let diff_sp = difficulty_speed_mul(wave);
    if mini {
        spawn_enemy(
            commands,
            kind,
            pos,
            MINI_BOSS_HP_MUL * diff_hp,
            MINI_BOSS_SZ_MUL,
            diff_sp,
            Promo::Mini,
        );
    } else {
        let (hp_mul, sz_mul, sp_mul) = boss_tier_mul(tier);
        let promo = if tier > 0 { Promo::Boss(tier) } else { Promo::None };
        spawn_enemy(commands, kind, pos, hp_mul * diff_hp, sz_mul, sp_mul * diff_sp, promo);
    }
}

/// Core spawn used by all variants: build the enemy with HP/size multipliers and
/// the given promotion marker.
fn spawn_enemy(
    commands: &mut Commands,
    kind: EnemyKind,
    pos: Vec2,
    hp_mul: f32,
    sz_mul: f32,
    speed_mul: f32,
    promo: Promo,
) {
    let st = stats_for(kind);

    let mut e = commands.spawn((
        Enemy { kind },
        AiState::default(),
        Velocity::default(),
        Collider { radius: st.radius * sz_mul },
        Health::new(st.health * hp_mul),
        SpeedMul(speed_mul),
        Faction::Enemy,
        shape_for(kind),
        Transform::from_translation(pos.extend(0.0)).with_scale(Vec3::splat(sz_mul)),
    ));

    match promo {
        Promo::Boss(tier) => {
            e.insert(Boss { tier });
        }
        Promo::Mini => {
            e.insert(MiniBoss);
        }
        Promo::None => {}
    }

    if let Some(cd) = st.fire_cooldown {
        e.insert(FireCooldown {
            cooldown: cd,
            timer: cd * 0.5,
        });
    }

    // The Drifter keeps its original drift velocity + white-hot core child.
    if kind == EnemyKind::Drifter {
        e.insert(Velocity(Vec2::new(55.0, -18.0)));
        e.with_children(|c| {
            c.spawn((
                shapes::drifter_core(18.0),
                Transform::from_xyz(0.0, 0.0, 1.0),
            ));
        });
    }
}
