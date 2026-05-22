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

/// Spawn one enemy of `kind` at world position `pos` (non-boss).
pub fn spawn(commands: &mut Commands, kind: EnemyKind, pos: Vec2) {
    spawn_tiered(commands, kind, pos, 0);
}

/// Spawn one enemy, applying a boss-tier HP/size overlay when `tier > 0`.
/// (Speed-scaling is a follow-up — AI reads a fixed per-kind `stats().speed`,
/// so it needs an entity-stored multiplier, tracked separately.)
pub fn spawn_tiered(commands: &mut Commands, kind: EnemyKind, pos: Vec2, tier: u8) {
    let st = stats_for(kind);
    let (hp_mul, sz_mul, _sp_mul) = boss_tier_mul(tier);

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

    if tier > 0 {
        e.insert(Boss { tier });
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
