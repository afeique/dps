//! Rendering setup.
//!
//! Phase 1: an HDR `Camera2d` with bloom, and a `glow()` helper for bright
//! emissive colors so silhouettes get a real glow halo. Phase 2 replaces the
//! placeholder `RegularPolygon` meshes with lyon-tessellated ports of
//! `js/modules/render/shapes.js`, and adds the particle/bullet pipelines.

use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::Hdr;

/// Spawn the single 2D camera with HDR + bloom enabled. Bloom is what turns
/// the over-bright emissive colors below into glow.
pub fn spawn_camera(mut commands: Commands) {
    commands.spawn((Camera2d, Hdr, Tonemapping::TonyMcMapface, Bloom::NATURAL));
}

/// Build an over-bright (HDR, components > 1.0) emissive color. With the
/// camera's bloom enabled this produces a glow halo around the mesh.
pub fn glow(r: f32, g: f32, b: f32) -> Color {
    Color::linear_rgb(r, g, b)
}
