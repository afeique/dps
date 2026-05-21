//! Deep-space nebula backdrop (`docs/port-plan.md` §3.4). The procedural
//! JWST-style gas clouds (teal/gold domain-warped fbm, dust lanes, hot cores)
//! are **baked once to a texture at startup** and shown as a single
//! screen-covering sprite. The per-frame cost is one texture sample instead of
//! the ~60 noise evaluations per pixel the live shader did every frame — that
//! full-screen procedural shader was tanking the framerate (~15 fps). An HDR
//! color tint pushes the bright baked cores past 1.0 so the camera bloom still
//! lights them.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// Baked texture resolution (square). The nebula is soft, so this need not
/// match the framebuffer; it's stretched across the screen with linear filter.
const BAKE_SIZE: u32 = 1024;
/// Noise-domain scale (matched the old shader's `params.y`).
const SCALE: f32 = 4.0;

pub fn spawn_nebula(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    // Perf diagnostic: `DPS_NO_NEBULA=1` skips the nebula entirely.
    if std::env::var("DPS_NO_NEBULA").is_ok() {
        return;
    }
    let image = images.add(bake_nebula(BAKE_SIZE));
    commands.spawn((
        Sprite {
            image,
            custom_size: Some(Vec2::new(2200.0, 1300.0)),
            // HDR tint: lifts the brightest baked pixels (cores) past 1.0 so
            // they bloom, while dim gas stays sub-threshold.
            color: Color::linear_rgb(1.6, 1.6, 1.6),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, -60.0),
    ));
}

// ── CPU bake (mirrors the former nebula.wgsl, evaluated once at startup) ────

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

fn fbm(p0: Vec2) -> f32 {
    let mut p = p0;
    let mut v = 0.0;
    let mut amp = 0.6;
    // Same column-major rotation as the old shader's mat2x2(1.6,1.2,-1.2,1.6).
    let m = Mat2::from_cols(Vec2::new(1.6, 1.2), Vec2::new(-1.2, 1.6));
    // 5 octaves: 1024px bake gives enough headroom for the extra detail without
    // aliasing into grain when stretched to screen.
    for _ in 0..5 {
        v += amp * vnoise(p);
        p = m * p;
        amp *= 0.5;
    }
    v
}

fn bake_nebula(size: u32) -> Image {
    let n = size as usize;
    let mut data = vec![0u8; n * n * 4];
    let teal = Vec3::new(0.0, 0.62, 0.85);
    let gold = Vec3::new(1.0, 0.42, 0.06);

    for y in 0..n {
        for x in 0..n {
            // Aspect-correct the noise domain so a noise cell is square *on screen*
            // after the texture is stretched onto the 2200×1300 quad (aspect ≈ 1.692).
            // Without this, circular cloud features are stretched ~1.7× horizontally.
            let aspect = 2200.0_f32 / 1300.0;
            let uv = Vec2::new(
                x as f32 / size as f32 * aspect,
                y as f32 / size as f32,
            ) * SCALE;

            let warp = Vec2::new(
                fbm(uv * 1.1 + Vec2::new(0.0, 1.7)),
                fbm(uv * 1.1 + Vec2::new(5.2, 9.3)),
            );
            let density = fbm(uv * 1.6 + warp * 2.5);
            // Full fbm for region/hue/dust (rich, vivid — free at bake time).
            let region = fbm(uv * 0.35 + Vec2::new(11.3, 4.7));
            let hue = fbm(uv * 0.28 + Vec2::new(20.0, -7.0));
            let dust = fbm(uv * 0.8 + Vec2::new(30.0, 12.0));

            // Slightly wider smoothstep range for softer alpha feathering on cloud edges.
            let neb = smoothstep(0.40, 0.98, density) * smoothstep(0.35, 0.80, region);
            let base = teal.lerp(gold, smoothstep(0.45, 0.85, hue));
            let mut col = base * (neb * 1.6);
            let core = smoothstep(0.80, 1.0, density) * neb;
            col += Vec3::new(1.6, 1.35, 1.05) * core;
            col *= 0.35 + 0.65 * smoothstep(0.30, 0.70, dust);

            let idx = (y * n + x) * 4;
            data[idx] = (col.x.clamp(0.0, 1.0) * 255.0) as u8;
            data[idx + 1] = (col.y.clamp(0.0, 1.0) * 255.0) as u8;
            data[idx + 2] = (col.z.clamp(0.0, 1.0) * 255.0) as u8;
            data[idx + 3] = (neb.clamp(0.0, 1.0) * 255.0) as u8;
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
