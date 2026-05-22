//! Rendering setup.
//!
//! Phase 1: an HDR `Camera2d` with bloom, and a `glow()` helper for bright
//! emissive colors so silhouettes get a real glow halo. Phase 2 replaces the
//! placeholder `RegularPolygon` meshes with lyon-tessellated ports of
//! `js/modules/render/shapes.js`, and adds the particle/bullet pipelines.

pub mod bullets;
pub mod explosion;
pub mod hud;
pub mod nebula;
pub mod screenshot;
pub mod shapes;
pub mod starfield;

use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::post_process::bloom::{Bloom, BloomPrefilter};
use bevy::prelude::*;
use bevy::render::view::Hdr;

/// Spawn the single 2D camera with HDR + bloom enabled. Bloom is what turns
/// the over-bright emissive colors below into glow.
pub fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        Hdr,
        Tonemapping::TonyMcMapface,
        // Dither the tonemap output — without it the TonyMcMapface 3D LUT
        // posterizes smooth dim gradients (the nebula) into triangular facets.
        DebandDither::Enabled,
        // Only HDR-emissive gameplay (>1.0) blooms; the dim nebula clouds
        // (<1.0) never feed bloom. Feeding those broad, dim, full-screen clouds
        // through bloom was what produced the rectangular blocks AND the
        // triangular bloom-upsampling facets. With the threshold, the clouds
        // render as plain smooth sprites and only the neon bullets/ship/
        // explosions glow.
        Bloom {
            intensity: 0.22,
            prefilter: BloomPrefilter {
                threshold: 1.0,
                threshold_softness: 0.5,
            },
            ..Bloom::NATURAL
        },
    ));
}

/// Build an over-bright (HDR, components > 1.0) emissive color. With the
/// camera's bloom enabled this produces a glow halo around the mesh.
pub fn glow(r: f32, g: f32, b: f32) -> Color {
    Color::linear_rgb(r, g, b)
}
