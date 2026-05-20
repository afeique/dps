//! Procedural deep-space nebula (`docs/port-plan.md` §3.1/§3.4). One large
//! background quad rendered with a custom `Material2d` whose WGSL shader
//! (`nebula.wgsl`, embedded in the binary) builds domain-warped fractal-noise
//! gas clouds in a JWST-style palette — teal filaments, warm gold, dark dust
//! lanes, and hot near-white cores. Output is HDR (> 1.0) so the camera bloom
//! lights the bright wisps. The shader animates from the global time uniform,
//! so there is no per-entity drift. Sits at z = -60, behind the starfield.

use bevy::asset::embedded_asset;
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};

/// Embeds the nebula WGSL in the binary and registers the material pipeline.
pub struct NebulaPlugin;

impl Plugin for NebulaPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "nebula.wgsl");
        app.add_plugins(Material2dPlugin::<NebulaMaterial>::default());
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct NebulaMaterial {
    /// x = brightness, y = noise scale, z = scroll speed, w = unused.
    #[uniform(0)]
    pub params: Vec4,
}

impl Material2d for NebulaMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://dps/render/nebula.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

/// Spawn the full-field nebula quad behind the starfield.
pub fn spawn_nebula(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<NebulaMaterial>>,
) {
    let mesh = meshes.add(Rectangle::new(2200.0, 1300.0));
    let material = materials.add(NebulaMaterial {
        params: Vec4::new(0.85, 4.0, 0.015, 0.0),
    });
    commands.spawn((
        Mesh2d(mesh),
        MeshMaterial2d(material),
        Transform::from_xyz(0.0, 0.0, -60.0),
    ));
}
