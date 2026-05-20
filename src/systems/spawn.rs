//! Spawn the Phase 1 slice on entering `Playing`: a player ship and one
//! drifter enemy. Phase 2 swapped the placeholder `RegularPolygon` meshes for
//! lyon-tessellated silhouettes ported from `js/modules/render/shapes.js` (see
//! `crate::render::shapes`). Phase 3 adds a wave manager (port of
//! `js/modules/wave/*`) instead of hard-coding spawns.

use crate::components::*;
use crate::render::shapes;
use bevy::prelude::*;

pub fn spawn_player(mut commands: Commands) {
    commands
        .spawn((
            Ship::default(),
            Intent::default(),
            Weapon::default(),
            Velocity::default(),
            Collider { radius: 20.0 },
            Health::new(100.0),
            Faction::Player,
            shapes::ship_hull(),
            Transform::from_xyz(0.0, -140.0, 0.0),
        ))
        .with_children(|ship| {
            ship.spawn((
                shapes::ship_cockpit(),
                Transform::from_translation(shapes::SHIP_COCKPIT_OFFSET.extend(1.0)),
            ));
        });
}

pub fn spawn_enemy(mut commands: Commands) {
    let radius = 18.0;
    commands
        .spawn((
            Enemy {
                kind: EnemyKind::Drifter,
            },
            AiState::default(),
            FireCooldown {
                cooldown: 1.4,
                timer: 1.0,
            },
            Velocity(Vec2::new(55.0, -18.0)),
            Collider { radius },
            Health::new(30.0),
            Faction::Enemy,
            shapes::drifter_star(radius),
            Transform::from_xyz(160.0, 130.0, 0.0),
        ))
        .with_children(|enemy| {
            enemy.spawn((shapes::drifter_core(radius), Transform::from_xyz(0.0, 0.0, 1.0)));
        });
}
