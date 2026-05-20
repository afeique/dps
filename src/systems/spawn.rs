//! Spawn the Phase 1 slice on entering `Playing`: a player ship and one
//! drifter enemy, both as glowing placeholder polygons. Phase 2 swaps the
//! meshes for lyon-tessellated silhouettes; Phase 3 adds a wave manager
//! (port of `js/modules/wave/*`) instead of hard-coding spawns.

use crate::components::*;
use crate::render::glow;
use bevy::prelude::*;

pub fn spawn_player(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let mesh = meshes.add(RegularPolygon::new(18.0, 3));
    let mat = materials.add(ColorMaterial::from(glow(4.0, 1.0, 7.0))); // prism violet

    commands.spawn((
        Ship::default(),
        Intent::default(),
        Weapon::default(),
        Velocity::default(),
        Collider { radius: 16.0 },
        Health::new(100.0),
        Faction::Player,
        Mesh2d(mesh),
        MeshMaterial2d(mat),
        Transform::from_xyz(0.0, -140.0, 0.0),
    ));
}

pub fn spawn_enemy(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let mesh = meshes.add(RegularPolygon::new(20.0, 6));
    let mat = materials.add(ColorMaterial::from(glow(0.4, 6.0, 7.0))); // prism cyan

    commands.spawn((
        Enemy {
            kind: EnemyKind::Drifter,
        },
        AiState::default(),
        FireCooldown {
            cooldown: 1.4,
            timer: 1.0,
        },
        Velocity(Vec2::new(55.0, -18.0)),
        Collider { radius: 18.0 },
        Health::new(30.0),
        Faction::Enemy,
        Mesh2d(mesh),
        MeshMaterial2d(mat),
        Transform::from_xyz(160.0, 130.0, 0.0),
    ));
}
