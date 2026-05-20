//! Per-enemy modules (mirrors `js/modules/enemy/*`). Each kind module exposes
//! `shape()` (lyon silhouette), `stats()` (base stats from `enemy-data.js`),
//! and `ai()` (a FixedUpdate movement system filtering its own kind). `spawn`
//! dispatches on `EnemyKind` to build the entity. Generic aimed firing lives in
//! `systems::enemy_fire`; per-kind fire patterns are a later refinement.
//!
//! Ported so far: Drifter (in `render::shapes` + `systems::enemy_ai`), Hunter,
//! Guardian, Wasp. Stalker / Prowler / Weaver / Sentinel / Tangerine / Titan
//! are subsequent increments; they fall back to the drifter visual for now.

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
            health: 30.0,
            radius: 18.0,
            speed: 60.0,
            fire_cooldown: Some(1.4),
        },
    }
}

/// Spawn one enemy of `kind` at world position `pos`.
pub fn spawn(commands: &mut Commands, kind: EnemyKind, pos: Vec2) {
    let st = stats_for(kind);
    let mut e = commands.spawn((
        Enemy { kind },
        AiState::default(),
        Velocity::default(),
        Collider { radius: st.radius },
        Health::new(st.health),
        Faction::Enemy,
        shape_for(kind),
        Transform::from_translation(pos.extend(0.0)),
    ));

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
