//! Arena parallax starfield (`docs/port-plan.md` §3.4).
//!
//! Stars are scattered in four depth layers (z ≈ -50 .. -45.5) and **do not
//! scroll**. Instead each star has a parallax factor: every frame the star is
//! repositioned at `base_pos − player_pos × parallax` so distant layers barely
//! shift while near layers shift visibly as the player moves — giving an
//! organic sense of depth without any camera movement.
//!
//! Twinkle is kept: each star pulsates its scale sinusoidally with an
//! independent phase and frequency so the field shimmers.
//!
//! Ten shared color-material tiers (no per-star material allocation).
//! Scatter uses a dependency-free deterministic hash — no `rand` crate.

use crate::components::Ship;
use crate::resources::PlayBounds;
use bevy::prelude::*;

// ── Star component ────────────────────────────────────────────────────────────

/// A background star. Stores everything needed by `parallax_stars`.
#[derive(Component)]
pub struct Star {
    /// Resting world-space position (where the star sits when the player is at
    /// the arena centre).
    pub base_pos: Vec2,
    /// How much the star shifts opposite the player. Far layers: ~0.02–0.05;
    /// near layers: ~0.12–0.20.
    pub parallax: f32,
    /// Base (no-twinkle) render scale.
    pub base_scale: f32,
    /// Twinkle amplitude as a fraction of `base_scale` (0 → no pulse).
    pub twinkle_amp: f32,
    /// Twinkle oscillation frequency (radians / second).
    pub twinkle_freq: f32,
    /// Per-star phase offset so stars don't all pulse together.
    pub twinkle_phase: f32,
}

// ── Deterministic hash ────────────────────────────────────────────────────────

/// Cheap deterministic [0, 1) hash (murmur-style finalizer) — no `rand` dep.
fn rand01(seed: u32) -> f32 {
    let mut x = seed.wrapping_add(0x9E37_79B9);
    x = (x ^ (x >> 16)).wrapping_mul(0x85EB_CA6B);
    x = (x ^ (x >> 13)).wrapping_mul(0xC2B2_AE35);
    x ^= x >> 16;
    x as f32 / u32::MAX as f32
}

/// Map `rand01` output to `[lo, hi)`.
#[inline]
fn randf(seed: u32, lo: f32, hi: f32) -> f32 {
    lo + rand01(seed) * (hi - lo)
}

// ── Colour palette (10 tiers) ─────────────────────────────────────────────────
//
// All values are linear RGB.  Values > 1.0 intentionally exceed SDR so they
// contribute to bloom in a HDR-enabled pipeline.
//
// Tier naming:
//   d_ = distant/dim   m_ = mid   n_ = near
//   suffix: W = cool-white  Ww = warm-white  B = blue  Bw = blue-white
//           C = cyan  A = amber  R = red (faint)  P = purple (faint)
//   _hdr   = HDR accent (> 1.0 peak component, bloom-ready)

const NUM_PALETTES: usize = 10;

fn build_palette(materials: &mut Assets<ColorMaterial>) -> [Handle<ColorMaterial>; NUM_PALETTES] {
    // 0 — distant cool-white (very dim)
    let d_w   = materials.add(ColorMaterial::from_color(Color::linear_rgb(0.16, 0.16, 0.20)));
    // 1 — distant warm-white (very dim, slightly yellowish)
    let d_ww  = materials.add(ColorMaterial::from_color(Color::linear_rgb(0.20, 0.18, 0.14)));
    // 2 — distant blue (faint)
    let d_b   = materials.add(ColorMaterial::from_color(Color::linear_rgb(0.10, 0.14, 0.32)));
    // 3 — mid neutral white
    let m_w   = materials.add(ColorMaterial::from_color(Color::linear_rgb(0.55, 0.56, 0.60)));
    // 4 — mid blue-white
    let m_bw  = materials.add(ColorMaterial::from_color(Color::linear_rgb(0.42, 0.50, 0.90)));
    // 5 — mid faint red
    let m_r   = materials.add(ColorMaterial::from_color(Color::linear_rgb(0.50, 0.18, 0.18)));
    // 6 — mid faint purple
    let m_p   = materials.add(ColorMaterial::from_color(Color::linear_rgb(0.36, 0.20, 0.52)));
    // 7 — near warm amber (moderate HDR)
    let n_a   = materials.add(ColorMaterial::from_color(Color::linear_rgb(1.30, 0.75, 0.20)));
    // 8 — near cyan HDR (bloom-ready)
    let n_c   = materials.add(ColorMaterial::from_color(Color::linear_rgb(0.70, 1.80, 2.40)));
    // 9 — near warm-white HDR accent (hottest, very sparse)
    let n_hdr = materials.add(ColorMaterial::from_color(Color::linear_rgb(2.20, 2.00, 1.80)));

    [d_w, d_ww, d_b, m_w, m_bw, m_r, m_p, n_a, n_c, n_hdr]
}

// ── Layer definitions ─────────────────────────────────────────────────────────

/// Field extends 15 % beyond play-bounds so parallax offsets stay filled.
const FIELD_MARGIN: f32 = 1.20;

struct LayerDef {
    count:          u32,
    z:              f32,
    parallax_lo:    f32,
    parallax_hi:    f32,
    scale_lo:       f32,
    scale_hi:       f32,
    twink_amp:      f32,
    twink_freq_lo:  f32,
    twink_freq_hi:  f32,
    /// Weighted palette index list; entry picked via hash.
    palette:        &'static [usize],
}

/// Four layers, far → near.  Star counts: 110 + 85 + 55 + 30 = 280 total.
const LAYERS: [LayerDef; 4] = [
    // Layer 0 — deep background: barely moves (parallax ≈ 0.02), tiny, dim
    LayerDef {
        count: 110, z: -50.0,
        parallax_lo: 0.015, parallax_hi: 0.035,
        scale_lo: 0.40,     scale_hi: 0.85,
        twink_amp: 0.07,
        twink_freq_lo: 0.25, twink_freq_hi: 0.90,
        // cool-white dominant, warm-white & blue sprinkled
        palette: &[0, 0, 1, 0, 2, 0, 1, 0, 0, 2],
    },
    // Layer 1 — mid-distant: slight parallax, neutral/blue whites, rare purple
    LayerDef {
        count: 85, z: -48.5,
        parallax_lo: 0.04, parallax_hi: 0.07,
        scale_lo: 0.65,    scale_hi: 1.30,
        twink_amp: 0.12,
        twink_freq_lo: 0.45, twink_freq_hi: 1.50,
        // mid-white dominant, blue-white + faint purple/red
        palette: &[3, 3, 4, 3, 4, 6, 3, 3, 5, 4],
    },
    // Layer 2 — mid-near: noticeable parallax, brighter, mixed palette
    LayerDef {
        count: 55, z: -47.0,
        parallax_lo: 0.08, parallax_hi: 0.12,
        scale_lo: 0.90,    scale_hi: 1.90,
        twink_amp: 0.18,
        twink_freq_lo: 0.70, twink_freq_hi: 2.20,
        // white/blue-white core, amber + purple accents
        palette: &[3, 4, 4, 7, 3, 6, 4, 3, 5, 4],
    },
    // Layer 3 — nearest: strong parallax (0.12–0.20), HDR bloom, shimmery
    LayerDef {
        count: 30, z: -45.5,
        parallax_lo: 0.12, parallax_hi: 0.20,
        scale_lo: 1.30,    scale_hi: 2.80,
        twink_amp: 0.28,
        twink_freq_lo: 1.00, twink_freq_hi: 3.20,
        // mostly cyan HDR + amber; a few warm-white HDR accents
        palette: &[8, 8, 7, 9, 8, 7, 8, 9, 4, 8],
    },
];

// ── Startup system ────────────────────────────────────────────────────────────

pub fn spawn_starfield(
    mut commands:  Commands,
    mut meshes:    ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    bounds:        Res<PlayBounds>,
) {
    let circle  = meshes.add(Circle::new(1.0));
    let palette = build_palette(&mut materials);
    // Scatter field is wider than the play-area so parallax offsets don't
    // expose a gap at the edges even when the player is at a corner.
    let field   = bounds.half * FIELD_MARGIN;

    let mut seed: u32 = 0xDEAD_BEEF;

    for (li, layer) in LAYERS.iter().enumerate() {
        for i in 0..layer.count {
            // Stride seeds by a large prime to de-correlate fields.
            let s0 = seed
                .wrapping_add(li as u32 * 100_003)
                .wrapping_add(i.wrapping_mul(17));

            let x         = randf(s0,                  -field.x, field.x);
            let y         = randf(s0.wrapping_add(1),  -field.y, field.y);
            let scale     = randf(s0.wrapping_add(2),  layer.scale_lo,    layer.scale_hi);
            let parallax  = randf(s0.wrapping_add(3),  layer.parallax_lo, layer.parallax_hi);
            let phase     = randf(s0.wrapping_add(4),  0.0, std::f32::consts::TAU);
            let freq      = randf(s0.wrapping_add(5),  layer.twink_freq_lo, layer.twink_freq_hi);

            let palette_slot = ((rand01(s0.wrapping_add(6)) * layer.palette.len() as f32) as usize)
                .min(layer.palette.len() - 1);
            let mat = palette[layer.palette[palette_slot]].clone();

            seed = seed.wrapping_add(7);

            commands.spawn((
                Star {
                    base_pos:      Vec2::new(x, y),
                    parallax,
                    base_scale:    scale,
                    twinkle_amp:   layer.twink_amp,
                    twinkle_freq:  freq,
                    twinkle_phase: phase,
                },
                Mesh2d(circle.clone()),
                MeshMaterial2d(mat),
                Transform::from_xyz(x, y, layer.z).with_scale(Vec3::splat(scale)),
            ));
        }
    }
}

// ── Per-frame system ──────────────────────────────────────────────────────────

/// Reposition stars relative to the player (parallax) and pulse their scale
/// (twinkle).  No vertical drift; no wrapping.
pub fn parallax_stars(
    time:       Res<Time>,
    player_q:   Query<&Transform, With<Ship>>,
    mut star_q: Query<(&Star, &mut Transform), Without<Ship>>,
) {
    let elapsed = time.elapsed_secs();

    // Bevy 0.18: single() returns Result — fall back to zero if no player.
    let player_pos: Vec2 = match player_q.single() {
        Ok(tf) => tf.translation.truncate(),
        Err(_) => Vec2::ZERO,
    };

    for (star, mut tf) in &mut star_q {
        // Parallax offset: stars shift opposite to player motion.
        let offset = star.base_pos - player_pos * star.parallax;
        tf.translation.x = offset.x;
        tf.translation.y = offset.y;
        // z is set at spawn and never changes.

        // Twinkle: sinusoidal scale pulse.
        let pulse = 1.0
            + star.twinkle_amp
                * (elapsed * star.twinkle_freq + star.twinkle_phase).sin();
        tf.scale = Vec3::splat(star.base_scale * pulse);
    }
}
