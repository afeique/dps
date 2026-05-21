//! Deep-space nebula — a MIX of two layers (port spec VII.5):
//!
//!  1. a baked **rich domain-warped fbm** wispy field — the original JWST look
//!     (teal/gold filaments, hot cores, dust lanes), but baked once to a
//!     high-res texture at startup so it costs one texture sample per frame
//!     instead of the ~15 fps live full-screen shader;
//!  2. a few smooth **gaussian cloud blobs** (shaped by analytic sine lobes,
//!     no value noise → no faceting) for soft broad color volume on top.
//!
//! Both layers are dim and sit behind the starfield (z −60..−57 vs stars at
//! −50..−45) so stars + gameplay read clearly on top, and both stay < 1.0 so
//! they never feed the (threshold-1.0) bloom.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// Baked wispy-fbm texture resolution (high enough that the stretch to the
/// field stays smooth; 6 octaves resolve without grain at this size).
const FBM_SIZE: u32 = 1024;
/// Gaussian accent-cloud texture resolution (small — magnified + soft).
const CLOUD_SIZE: u32 = 256;
/// Screen-covering quad size (the fbm layer fills this).
const FIELD_W: f32 = 2200.0;
const FIELD_H: f32 = 1300.0;

/// Gaussian color-accent clouds: `(x, y, w, h, rotation°, tex idx, tint, alpha)`.
/// Dim, few, and broad — they add soft color between the fbm filaments.
const CLOUDS: &[(f32, f32, f32, f32, f32, usize, [f32; 3], f32)] = &[
    (-360.0, 200.0, 820.0, 600.0, 18.0, 0, [0.10, 0.45, 0.62], 0.11),
    (360.0, -180.0, 760.0, 640.0, -14.0, 1, [0.62, 0.34, 0.10], 0.10),
    (150.0, 250.0, 640.0, 520.0, 42.0, 2, [0.42, 0.14, 0.42], 0.08),
    (-300.0, -230.0, 600.0, 660.0, 64.0, 1, [0.16, 0.26, 0.55], 0.09),
];

pub fn spawn_nebula(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    // Perf diagnostic: `DPS_NO_NEBULA=1` skips the nebula entirely.
    if std::env::var("DPS_NO_NEBULA").is_ok() {
        return;
    }

    // Layer 1 — wispy fbm base, full-screen, dim.
    let fbm = images.add(bake_fbm(FBM_SIZE));
    commands.spawn((
        Sprite {
            image: fbm,
            custom_size: Some(Vec2::new(FIELD_W, FIELD_H)),
            // Dim tint keeps the wisps subtle and < 1.0 (no bloom); the texture
            // alpha leaves dark gaps so stars show through.
            color: Color::linear_rgba(0.30, 0.30, 0.30, 0.72),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, -60.0),
    ));

    // Layer 2 — a few smooth gaussian color accents in front of the fbm.
    let clouds: Vec<Handle<Image>> = (0..3)
        .map(|s| images.add(bake_cloud(CLOUD_SIZE, s as f32 * 2.3 + 0.7)))
        .collect();
    for (i, &(x, y, w, h, rot, tex, [r, g, b], a)) in CLOUDS.iter().enumerate() {
        commands.spawn((
            Sprite {
                image: clouds[tex].clone(),
                custom_size: Some(Vec2::new(w, h)),
                color: Color::linear_rgba(r, g, b, a),
                ..default()
            },
            Transform::from_xyz(x, y, -58.0 + i as f32 * 0.2)
                .with_rotation(Quat::from_rotation_z(rot.to_radians())),
        ));
    }
}

// ── shared smooth value-noise fbm (for the wispy layer) ─────────────────────

#[inline]
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Cheap arithmetic hash (Dave Hoskins' hash12).
#[inline]
fn hash2(p: Vec2) -> f32 {
    let mut p3 = (Vec3::new(p.x, p.y, p.x) * 0.1031).fract();
    let d = p3.dot(Vec3::new(p3.y, p3.z, p3.x) + Vec3::splat(33.33));
    p3 += Vec3::splat(d);
    ((p3.x + p3.y) * p3.z).fract()
}

fn vnoise(p: Vec2) -> f32 {
    let i = p.floor();
    let f = p.fract();
    let u = f * f * (Vec2::splat(3.0) - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + Vec2::new(1.0, 0.0));
    let c = hash2(i + Vec2::new(0.0, 1.0));
    let d = hash2(i + Vec2::new(1.0, 1.0));
    let ab = a + (b - a) * u.x;
    let cd = c + (d - c) * u.x;
    ab + (cd - ab) * u.y
}

/// 5-octave fbm with rotated octaves — smooth at FBM_SIZE (no sub-texel grain).
fn fbm(p0: Vec2) -> f32 {
    let mut p = p0;
    let mut v = 0.0;
    let mut amp = 0.6;
    let m = Mat2::from_cols(Vec2::new(1.6, 1.2), Vec2::new(-1.2, 1.6));
    for _ in 0..5 {
        v += amp * vnoise(p);
        p = m * p;
        amp *= 0.5;
    }
    v
}

/// Bake the rich domain-warped JWST nebula (the early "wispy" look) once.
fn bake_fbm(size: u32) -> Image {
    let n = size as usize;
    let mut data = vec![0u8; n * n * 4];
    let aspect = FIELD_W / FIELD_H; // un-stretch the wide quad
    let teal = Vec3::new(0.0, 0.62, 0.85);
    let gold = Vec3::new(1.0, 0.42, 0.06);

    for y in 0..n {
        for x in 0..n {
            let uv = Vec2::new(x as f32 / size as f32 * aspect, y as f32 / size as f32) * 3.5;

            // Domain warp → turbulent wispy filaments.
            let warp = Vec2::new(
                fbm(uv * 1.1 + Vec2::new(0.0, 1.7)),
                fbm(uv * 1.1 + Vec2::new(5.2, 9.3)),
            );
            let density = fbm(uv * 1.6 + warp * 2.5);
            let region = fbm(uv * 0.35 + Vec2::new(11.3, 4.7));
            let hue = fbm(uv * 0.28 + Vec2::new(20.0, -7.0));
            let dust = fbm(uv * 0.8 + Vec2::new(30.0, 12.0));

            // Gas only where detail density AND region are high. Higher floors
            // → more empty dark sky between the wisps so the stars read.
            let neb = smoothstep(0.56, 0.96, density) * smoothstep(0.50, 0.82, region);
            let base = teal.lerp(gold, smoothstep(0.45, 0.85, hue));
            let mut col = base * (neb * 1.4);
            // Soft cores — kept low so they don't punch bright hotspots that
            // out-shine the stars (the nebula is a backdrop, not a foreground).
            let core = smoothstep(0.82, 1.0, density) * neb;
            col += Vec3::new(0.5, 0.42, 0.32) * core;
            // Dark dust lanes.
            col *= 0.4 + 0.6 * smoothstep(0.30, 0.70, dust);

            let idx = (y * n + x) * 4;
            data[idx] = (col.x.clamp(0.0, 1.0) * 255.0) as u8;
            data[idx + 1] = (col.y.clamp(0.0, 1.0) * 255.0) as u8;
            data[idx + 2] = (col.z.clamp(0.0, 1.0) * 255.0) as u8;
            data[idx + 3] = (neb.clamp(0.0, 1.0) * 255.0) as u8; // coverage → dark gaps
        }
    }

    finish_image(size, data)
}

// ── smooth gaussian accent cloud (analytic sine lobes, no noise) ────────────

fn bake_cloud(size: u32, phase: f32) -> Image {
    let n = size as usize;
    let mut data = vec![0u8; n * n * 4];

    for y in 0..n {
        for x in 0..n {
            let px = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let py = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let r = (px * px + py * py).sqrt();
            let ang = py.atan2(px);

            // Smooth organic lobing — analytic sines, so it can never facet.
            let lobe = 1.0
                + 0.18 * (ang * 2.0 + phase).sin()
                + 0.11 * (ang * 3.0 - phase * 1.7).sin()
                + 0.07 * (ang * 5.0 + phase * 0.6).sin();
            let rr = r / lobe.max(0.45);
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

    finish_image(size, data)
}

fn finish_image(size: u32, data: Vec<u8>) -> Image {
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
