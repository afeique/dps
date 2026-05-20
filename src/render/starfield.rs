//! Parallax starfield (`docs/port-plan.md` §3.4). Three depth layers of stars
//! drift downward at different speeds — nearer layers are bigger, brighter, and
//! faster — giving a parallax sense of forward flight. Stars live on a far
//! background z behind all gameplay and wrap vertically. Scatter is a cheap
//! deterministic hash, so the field is dependency-free and stable across runs.

use crate::resources::PlayBounds;
use bevy::prelude::*;

/// A background star, drifting down at `drift` world-units/second.
#[derive(Component)]
pub struct Star {
    pub drift: f32,
}

/// Field extends 15% past the play bounds so wrap-around stays off-screen.
const FIELD_MARGIN: f32 = 1.15;

/// Cheap deterministic [0, 1) hash (murmur-style finalizer) — no `rand` dep.
fn rand01(seed: u32) -> f32 {
    let mut x = seed.wrapping_add(0x9E37_79B9);
    x = (x ^ (x >> 16)).wrapping_mul(0x85EB_CA6B);
    x = (x ^ (x >> 13)).wrapping_mul(0xC2B2_AE35);
    x ^= x >> 16;
    x as f32 / u32::MAX as f32
}

pub fn spawn_starfield(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    bounds: Res<PlayBounds>,
) {
    let star = meshes.add(Circle::new(1.0));
    let field = bounds.half * FIELD_MARGIN;

    // (count, base scale, drift speed, color) — far/dim/slow → near/bright/fast.
    let layers = [
        (140u32, 0.7f32, 8.0f32, Color::linear_rgb(0.25, 0.25, 0.35)),
        (80, 1.0, 20.0, Color::linear_rgb(0.6, 0.6, 0.8)),
        (40, 1.5, 42.0, Color::linear_rgb(1.6, 1.6, 2.2)), // nearest blooms a touch
    ];

    let mut seed = 1u32;
    for (li, &(count, scale, drift, color)) in layers.iter().enumerate() {
        let mat = materials.add(ColorMaterial::from(color));
        let z = -50.0 + li as f32; // all behind gameplay (z >= 0)
        for _ in 0..count {
            let x = (rand01(seed) * 2.0 - 1.0) * field.x;
            let y = (rand01(seed + 1) * 2.0 - 1.0) * field.y;
            let s = scale * (0.6 + rand01(seed + 2) * 0.8); // size jitter
            seed += 3;
            commands.spawn((
                Star { drift },
                Mesh2d(star.clone()),
                MeshMaterial2d(mat.clone()),
                Transform::from_xyz(x, y, z).with_scale(Vec3::splat(s)),
            ));
        }
    }
}

/// Drift stars down, wrapping back to the top when they leave the field.
pub fn drift_stars(time: Res<Time>, bounds: Res<PlayBounds>, mut q: Query<(&Star, &mut Transform)>) {
    let dt = time.delta_secs();
    let field_h = bounds.half.y * FIELD_MARGIN;
    let span = field_h * 2.0;
    for (star, mut tf) in &mut q {
        tf.translation.y -= star.drift * dt;
        if tf.translation.y < -field_h {
            tf.translation.y += span;
        }
    }
}
