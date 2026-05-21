//! Deep-space nebula backdrop — smooth gaussian cloud blobs.
//!
//! The earlier full-screen turbulent-fbm nebula looked jagged / torn up. Per
//! the port spec (VII.5) the live nebula is a handful of HUGE low-alpha cloud
//! sprites layered in different teal / gold / rose tints (JWST multi-hue). We
//! bake a few cloud textures once and spawn a few dim, varied, low-alpha
//! sprites behind the starfield.
//!
//! Smoothness: the shape is a gaussian falloff whose radius is gently modulated
//! by a few **analytic sine lobes** (C-infinity smooth → organic but never
//! faceted). Value-noise modulation was tried first and produced triangular
//! facets — `exp()` amplifies the curvature discontinuities at the noise
//! lattice into visible creases — so we avoid noise here entirely.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// Baked cloud texture resolution (small — it's magnified and soft).
const CLOUD_SIZE: u32 = 256;

/// One nebula cloud: `(x, y, width, height, rotation°, texture idx, tint rgb, alpha)`.
/// Spread across (and a bit beyond) the ~1280×720 view with dark gaps between.
/// Tints are dim (RGB < 1) so the clouds stay a subtle backdrop; they're below
/// the bloom threshold so they never glow/bloom.
const CLOUDS: &[(f32, f32, f32, f32, f32, usize, [f32; 3], f32)] = &[
    (-380.0, 180.0, 960.0, 700.0, 18.0, 0, [0.12, 0.55, 0.78], 0.52),
    (340.0, -170.0, 880.0, 760.0, -14.0, 1, [0.85, 0.46, 0.12], 0.46),
    (120.0, 260.0, 760.0, 580.0, 42.0, 2, [0.55, 0.18, 0.52], 0.40),
    (-320.0, -240.0, 700.0, 780.0, 68.0, 1, [0.20, 0.32, 0.70], 0.42),
    (480.0, 230.0, 620.0, 540.0, -32.0, 0, [0.12, 0.55, 0.78], 0.40),
    (-560.0, -40.0, 580.0, 660.0, 10.0, 2, [0.85, 0.46, 0.12], 0.36),
    (40.0, -300.0, 820.0, 520.0, 54.0, 1, [0.55, 0.18, 0.52], 0.36),
    (280.0, 70.0, 540.0, 500.0, -50.0, 0, [0.20, 0.32, 0.70], 0.44),
];

pub fn spawn_nebula(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    // Perf diagnostic: `DPS_NO_NEBULA=1` skips the nebula entirely.
    if std::env::var("DPS_NO_NEBULA").is_ok() {
        return;
    }

    // A few cloud variants with different lobe phases → different organic shapes.
    let textures: Vec<Handle<Image>> = (0..3)
        .map(|s| images.add(bake_cloud(CLOUD_SIZE, s as f32 * 2.3 + 0.7)))
        .collect();

    for (i, &(x, y, w, h, rot, tex, [r, g, b], a)) in CLOUDS.iter().enumerate() {
        commands.spawn((
            Sprite {
                image: textures[tex].clone(),
                custom_size: Some(Vec2::new(w, h)),
                color: Color::linear_rgba(r, g, b, a),
                ..default()
            },
            // All behind the starfield (z -50..-45); slight z spread to layer.
            Transform::from_xyz(x, y, -60.0 + i as f32 * 0.2)
                .with_rotation(Quat::from_rotation_z(rot.to_radians())),
        ));
    }
}

// ── CPU bake: one smooth gaussian cloud texture ─────────────────────────────

#[inline]
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn bake_cloud(size: u32, phase: f32) -> Image {
    let n = size as usize;
    let mut data = vec![0u8; n * n * 4];

    for y in 0..n {
        for x in 0..n {
            let px = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0; // -1..1
            let py = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let r = (px * px + py * py).sqrt();
            let ang = py.atan2(px);

            // Smooth organic lobing — analytic sines, so no faceting. A few
            // harmonics at different phases make a non-circular, cloud-like edge.
            let lobe = 1.0
                + 0.18 * (ang * 2.0 + phase).sin()
                + 0.11 * (ang * 3.0 - phase * 1.7).sin()
                + 0.07 * (ang * 5.0 + phase * 0.6).sin();
            let rr = r / lobe.max(0.45);

            // Gaussian core + edge vignette → alpha is 0 by the sprite boundary.
            let gaussian = (-rr * rr * 2.4).exp();
            let vignette = smoothstep(1.0, 0.5, r);
            let alpha = (gaussian * vignette).clamp(0.0, 1.0);

            let idx = (y * n + x) * 4;
            data[idx] = 255; // white; tinted per-sprite via Sprite.color
            data[idx + 1] = 255;
            data[idx + 2] = 255;
            data[idx + 3] = (alpha * 255.0) as u8;
        }
    }

    let mut image = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::linear();
    image
}
