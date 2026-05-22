//! Deep-space nebula — a faithful port of the Rainboids WebGL nebula
//! (`game-engine.js::_populateWebGLNebula`, port spec VII.5).
//!
//! Instead of one full-screen backdrop, the nebula is a scatter of large,
//! low-alpha, tinted **cloud sprites** grouped into a few JWST-style "regions",
//! plus a little drift haze. Each region picks a `{core, mid, edge}` palette and
//! lays down layered clouds from three baked textures:
//!
//!   * `cloud`  — soft wide gaussian backbone (the outer halo + drift haze)
//!   * `wispy`  — anisotropic fbm filaments / streamers (the mid-body)
//!   * `core`   — dense bright core / ionization front
//!
//! The clouds parallax very slowly with the player (like the far starfield) and
//! sit behind the stars (z −62..−56 vs stars −50..−45), dim and tinted < 1.0 so
//! the starfield + gameplay read on top and nothing feeds the threshold-1.0
//! bloom. Placement is seeded (deterministic) so the backdrop is stable per run.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use std::f32::consts::TAU;

use crate::components::Ship;
use crate::resources::PlayBounds;

/// Baked cloud-texture resolution (small — magnified + soft + dim).
const TEX_SIZE: u32 = 256;
/// How far past the play-bounds the nebula field extends (matches starfield).
const FIELD_MARGIN: f32 = 1.30;

/// A nebula cloud sprite: parallax-drifts with the player and rotates slowly.
#[derive(Component)]
pub struct NebulaCloud {
    /// Resting world position (when the player is at the origin).
    base_pos: Vec2,
    /// Parallax factor (very small — clouds are the most distant layer).
    parallax: f32,
    /// Starting rotation.
    base_angle: f32,
    /// Slow spin rate (rad/sec).
    rot_rate: f32,
}

/// Which baked texture a cloud uses.
#[derive(Clone, Copy)]
enum Tex {
    Cloud = 0,
    Wispy = 1,
    Core = 2,
}

/// JWST-inspired palettes — `[core, mid, edge]` RGB, brightest→coolest.
/// Ported verbatim from `game-engine.js` `PALETTES`.
const PALETTES: &[[[f32; 3]; 3]] = &[
    // Pillars of Creation — gold/amber dust + teal H II + deep red
    [[1.00, 0.95, 0.70], [1.00, 0.55, 0.20], [0.85, 0.20, 0.30]],
    [[0.85, 1.00, 0.95], [0.30, 0.85, 0.95], [0.20, 0.40, 0.85]],
    // Cosmic Cliffs (Carina) — orange ridge + cyan gas + tan dust
    [[1.00, 0.85, 0.55], [1.00, 0.45, 0.15], [0.30, 0.85, 0.95]],
    // Tarantula — magenta + blue + amber
    [[1.00, 0.70, 0.95], [0.85, 0.30, 0.85], [0.30, 0.50, 1.00]],
    // Southern Ring — emerald/teal core with violet halo
    [[0.65, 1.00, 0.85], [0.30, 0.85, 0.75], [0.65, 0.40, 0.95]],
    // NGC 6334 (Cat's Paw) — deep red core, violet wisps, gold edges
    [[1.00, 0.55, 0.40], [0.85, 0.30, 0.55], [1.00, 0.80, 0.35]],
    // Helix-style — gold center, cool teal halo
    [[1.00, 0.90, 0.50], [0.65, 0.85, 0.60], [0.30, 0.70, 0.95]],
    // Eagle Nebula greenish — emerald + warm dust
    [[0.85, 1.00, 0.65], [0.45, 0.85, 0.40], [1.00, 0.55, 0.30]],
];

/// Drift-haze tints (blue/violet lonely clouds).
const DRIFT_TINTS: &[[f32; 3]] = &[
    [0.20, 0.40, 0.85],
    [0.40, 0.30, 0.80],
    [0.30, 0.55, 0.85],
    [0.50, 0.40, 0.75],
    [0.25, 0.55, 0.75],
];

const NUM_REGIONS: usize = 4;
const DRIFT_COUNT: usize = 3;

/// Alpha scale vs. the JS values: the JS clouds are *additively* blended, which
/// accumulates brighter than the alpha-blended sprites we use here, so we bump
/// the per-sprite alpha to land at a similar on-screen density. Kept low enough
/// that the field stays a backdrop and never reaches the bloom threshold.
const ALPHA_GAIN: f32 = 1.7;

pub fn spawn_nebula(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    bounds: Res<PlayBounds>,
) {
    // Perf/diagnostic escape hatch.
    if std::env::var("DPS_NO_NEBULA").is_ok() {
        return;
    }

    let tex = [
        images.add(bake_cloud(TEX_SIZE)),
        images.add(bake_wispy(TEX_SIZE)),
        images.add(bake_core(TEX_SIZE)),
    ];

    // dps field is centered on the origin; sizes are scaled from the JS 1920-wide
    // field down toward the 1280-wide dps arena (×~0.7).
    let field = bounds.half * FIELD_MARGIN;
    let mut rng = Rng::new(0x51ED_2A17);

    let place = |commands: &mut Commands,
                     pos: Vec2,
                     size: Vec2,
                     t: Tex,
                     tint: [f32; 3],
                     alpha: f32,
                     z: f32,
                     parallax: f32,
                     angle: f32,
                     rot_rate: f32| {
        commands.spawn((
            Sprite {
                image: tex[t as usize].clone(),
                custom_size: Some(size),
                color: Color::linear_rgba(tint[0], tint[1], tint[2], (alpha * ALPHA_GAIN).min(0.30)),
                ..default()
            },
            Transform::from_xyz(pos.x, pos.y, z).with_rotation(Quat::from_rotation_z(angle)),
            NebulaCloud { base_pos: pos, parallax, base_angle: angle, rot_rate },
        ));
    };

    // ── 4 nebula regions ────────────────────────────────────────────────────
    for _ in 0..NUM_REGIONS {
        let palette = PALETTES[rng.idx(PALETTES.len())];
        // Region center within ±0.6 of the field on each axis.
        let region = Vec2::new(
            rng.range(-0.6, 0.6) * field.x,
            rng.range(-0.6, 0.6) * field.y,
        );
        let region_angle = rng.range(0.0, TAU);
        let region_scale = 0.70 + rng.next() * 0.60;

        // Outer halo: 1–2 large soft clouds (edge color).
        let halo_count = 1 + rng.idx(2);
        for _ in 0..halo_count {
            let off = Vec2::new(rng.range(-0.5, 0.5), rng.range(-0.5, 0.5)) * 150.0 * region_scale;
            let size = (360.0 + rng.next() * 160.0) * region_scale;
            place(
                &mut commands, region + off, Vec2::splat(size), Tex::Cloud,
                palette[2], 0.05 + rng.next() * 0.04, -62.0,
                0.02 + rng.next() * 0.03, rng.range(0.0, TAU), rng.range(-0.02, 0.02),
            );
        }

        // Mid-body: 2 wispy filaments aligned to the region angle.
        for _ in 0..2 {
            let along = rng.range(-0.5, 0.5) * 250.0 * region_scale;
            let across = rng.range(-0.5, 0.5) * 80.0 * region_scale;
            let off = Vec2::new(
                region_angle.cos() * along - region_angle.sin() * across,
                region_angle.sin() * along + region_angle.cos() * across,
            );
            let size = (250.0 + rng.next() * 130.0) * region_scale;
            place(
                &mut commands, region + off, Vec2::new(size, size * 0.7), Tex::Wispy,
                palette[1], 0.07 + rng.next() * 0.06, -59.0,
                0.03 + rng.next() * 0.04, region_angle + rng.range(-0.3, 0.3), rng.range(-0.03, 0.03),
            );
        }

        // Core: 1 dense bright core / ionization front.
        let off = Vec2::new(rng.range(-0.5, 0.5), rng.range(-0.5, 0.5)) * 100.0 * region_scale;
        let size = (160.0 + rng.next() * 110.0) * region_scale;
        place(
            &mut commands, region + off, Vec2::splat(size), Tex::Core,
            palette[0], 0.10 + rng.next() * 0.06, -56.0,
            0.04 + rng.next() * 0.04, rng.range(0.0, TAU), rng.range(-0.04, 0.04),
        );
    }

    // ── Drift haze: a few lonely clouds scattered across the field ───────────
    for _ in 0..DRIFT_COUNT {
        let pos = Vec2::new(rng.range(-1.0, 1.0) * field.x, rng.range(-1.0, 1.0) * field.y);
        let size = 210.0 + rng.next() * 130.0;
        let tint = DRIFT_TINTS[rng.idx(DRIFT_TINTS.len())];
        place(
            &mut commands, pos, Vec2::splat(size), Tex::Cloud,
            tint, 0.04 + rng.next() * 0.03, -61.0,
            0.02 + rng.next() * 0.03, rng.range(0.0, TAU), rng.range(-0.02, 0.02),
        );
    }
}

/// Parallax-drift the clouds with the player and spin them very slowly.
pub fn parallax_nebula(
    time: Res<Time>,
    player_q: Query<&Transform, With<Ship>>,
    mut cloud_q: Query<(&NebulaCloud, &mut Transform), Without<Ship>>,
) {
    let elapsed = time.elapsed_secs();
    let player_pos: Vec2 = match player_q.single() {
        Ok(tf) => tf.translation.truncate(),
        Err(_) => Vec2::ZERO,
    };
    for (cloud, mut tf) in &mut cloud_q {
        let offset = cloud.base_pos - player_pos * cloud.parallax;
        tf.translation.x = offset.x;
        tf.translation.y = offset.y;
        tf.rotation = Quat::from_rotation_z(cloud.base_angle + elapsed * cloud.rot_rate);
    }
}

// ── seeded RNG (deterministic backdrop) ─────────────────────────────────────

struct Rng(u32);
impl Rng {
    fn new(seed: u32) -> Self {
        Self(seed | 1)
    }
    /// xorshift32 → [0, 1).
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.next() * (hi - lo)
    }
    fn idx(&mut self, n: usize) -> usize {
        ((self.next() * n as f32) as usize).min(n - 1)
    }
}

// ── noise helpers (value-noise fbm) ─────────────────────────────────────────

#[inline]
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

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

/// 3-octave fbm (matches the JS nebula-atlas octave count).
fn fbm(p0: Vec2) -> f32 {
    let mut p = p0;
    let mut v = 0.0;
    let mut amp = 0.6;
    for _ in 0..3 {
        v += amp * vnoise(p);
        p *= 2.0;
        amp *= 0.5;
    }
    v
}

// ── baked textures (white RGB, alpha = density; tinted per-sprite) ──────────

/// Slot 8 — soft wide gaussian cloud with puffy noise edges.
fn bake_cloud(size: u32) -> Image {
    let n = size as usize;
    let mut data = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let px = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let py = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let r2 = px * px + py * py;
            let uv = Vec2::new(px, py) * 3.0;
            let puff = 0.55 + 0.45 * fbm(uv + Vec2::new(7.0, 3.0));
            // Radial vignette → alpha is exactly 0 by the quad edge (no hard
            // rectangular border where the sprite quad cuts the texture off).
            let vign = smoothstep(1.0, 0.55, r2.sqrt());
            let a = (-r2 * 2.2).exp() * puff * vign;
            write_px(&mut data, n, x, y, a.clamp(0.0, 1.0));
        }
    }
    finish(size, data)
}

/// Slot 13 — anisotropic fbm filaments (horizontal streamers), oval mask.
fn bake_wispy(size: u32) -> Image {
    let n = size as usize;
    let mut data = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let px = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let py = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            // Anisotropic sample: stretch X 2.5×, squash Y 0.8× → filaments.
            let uv = Vec2::new(px * 2.5, py * 0.8) * 2.2 + Vec2::new(13.0, 4.0);
            let wisp = fbm(uv).powf(1.7);
            let mask = (-(px * px * 0.7 + py * py * 1.5) * 2.0).exp();
            let vign = smoothstep(1.0, 0.55, (px * px + py * py).sqrt());
            let a = (wisp * mask * 1.6 * vign).clamp(0.0, 1.0);
            write_px(&mut data, n, x, y, a);
        }
    }
    finish(size, data)
}

/// Slot 14 — dense bright core: sharp peak + fbm-modulated halo.
fn bake_core(size: u32) -> Image {
    let n = size as usize;
    let mut data = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let px = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let py = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let r2 = px * px + py * py;
            let r = r2.sqrt();
            let uv = Vec2::new(px, py) * 3.5 + Vec2::new(21.0, 9.0);
            let peak = (-r2 * 4.0).exp();
            let halo = (1.0 - r).max(0.0);
            let vign = smoothstep(1.0, 0.6, r);
            let a = ((peak + halo * 0.55 * fbm(uv)) * vign).clamp(0.0, 1.0);
            write_px(&mut data, n, x, y, a);
        }
    }
    finish(size, data)
}

#[inline]
fn write_px(data: &mut [u8], n: usize, x: usize, y: usize, a: f32) {
    let idx = (y * n + x) * 4;
    data[idx] = 255;
    data[idx + 1] = 255;
    data[idx + 2] = 255;
    data[idx + 3] = (a * 255.0) as u8;
}

fn finish(size: u32, data: Vec<u8>) -> Image {
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
