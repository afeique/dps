//! Shared bullet render assets. Bullet *spawning* (reading `Fire`) lives in
//! `systems::weapons`; this just caches one mesh + the per-team materials so
//! every shot reuses handles instead of allocating a mesh + material per
//! bullet (the old per-shot churn). Bright HDR-emissive colors → bloom glow.

use bevy::prelude::*;

/// Cached handles for the bullet layer, built once at startup.
#[derive(Resource)]
pub struct BulletAssets {
    /// Unit circle (radius 1); bullets scale it via `Transform`.
    pub circle: Handle<Mesh>,
    /// Player bullet body — bright yellow (`#FFFF00`).
    pub player_body: Handle<ColorMaterial>,
    /// Player bullet white-hot core (child of the body).
    pub player_core: Handle<ColorMaterial>,
    /// Enemy bullet body — hot magenta.
    pub enemy_body: Handle<ColorMaterial>,
}

pub fn setup_bullet_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.insert_resource(BulletAssets {
        circle: meshes.add(Circle::new(1.0)),
        player_body: materials.add(ColorMaterial::from(Color::linear_rgb(9.0, 8.0, 1.0))),
        player_core: materials.add(ColorMaterial::from(Color::linear_rgb(10.0, 10.0, 9.0))),
        enemy_body: materials.add(ColorMaterial::from(Color::linear_rgb(9.0, 0.7, 3.0))),
    });
}
