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
use crate::messages::Fire;
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

/// HP-threshold boss rage (spec IV.7, one-shot): when a boss drops to ≤33% of
/// its max HP it rages — a 1.5 s invulnerability window, a permanent ×0.66 fire
/// cooldown, and an immediate 16-bullet circular tantrum. (Deferred from the
/// spec: the 24-frame telegraph, homing bullets, screen flash/shake, the red
/// aura, and the per-tier pair/formation links.)
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
        // Activate rage once.
        commands
            .entity(e)
            .insert(Raged)
            .insert(Invulnerable { seconds: 1.5 });
        if let Some(mut fc) = fc {
            fc.cooldown *= 0.66;
            fc.timer = 0.0; // fire again immediately
        }
        // 16-bullet circular tantrum.
        let pos = tf.translation.truncate();
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
    let (hp_mul, sz_mul, _sp_mul) = boss_tier_mul(tier);
    let promo = if tier > 0 { Promo::Boss(tier) } else { Promo::None };
    spawn_enemy(commands, kind, pos, hp_mul, sz_mul, promo);
}

/// Spawn a mid-wave **mini-boss** promotion of `kind` (spec V.6).
pub fn spawn_mini_boss(commands: &mut Commands, kind: EnemyKind, pos: Vec2) {
    spawn_enemy(
        commands,
        kind,
        pos,
        MINI_BOSS_HP_MUL,
        MINI_BOSS_SZ_MUL,
        Promo::Mini,
    );
}

/// Core spawn used by all variants: build the enemy with HP/size multipliers and
/// the given promotion marker.
fn spawn_enemy(
    commands: &mut Commands,
    kind: EnemyKind,
    pos: Vec2,
    hp_mul: f32,
    sz_mul: f32,
    promo: Promo,
) {
    let st = stats_for(kind);

    let mut e = commands.spawn((
        Enemy { kind },
        AiState::default(),
        Velocity::default(),
        Collider { radius: st.radius * sz_mul },
        Health::new(st.health * hp_mul),
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
