//! Parallax starfield (`docs/port-plan.md` §3.4). Four depth layers of stars
//! drift downward at different speeds — nearer layers are bigger, brighter, and
//! faster — giving a parallax sense of forward flight. Stars live at z ≈ -50..-45
//! behind all gameplay and wrap vertically. Color variety, size jitter, and
//! per-star twinkle (scale pulsing) enrich the field without per-frame material
//! mutations. Scatter uses a dependency-free deterministic hash.

use crate::resources::PlayBounds;
use bevy::prelude::*;

/// A background star.
#[derive(Component)]
pub struct Star {
    /// Downward drift speed in world-units/second.
    pub drift: f32,
    /// Twinkle oscillation frequency (radians/second).
    pub twinkle_freq: f32,
    /// Twinkle phase offset so stars pulse at different times.
    pub twinkle_phase: f32,
    /// Base scale (no-twinkle size) — twinkle modulates around this.
    pub base_scale: f32,
    /// Twinkle amplitude as fraction of base_scale (0 → no pulse).
    pub twinkle_amp: f32,
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

/// Map rand01 output to `[lo, hi)`.
#[inline]
fn randf(seed: u32, lo: f32, hi: f32) -> f32 {
    lo + rand01(seed) * (hi - lo)
}

// ── Color palette ────────────────────────────────────────────────────────────
//
// Six shared material handles (one per color/brightness tier).
// We intentionally keep no per-star material so there is zero per-frame
// material churn — twinkle is expressed through Transform.scale only.
//
// Tier naming: D = distant/dim, M = mid, N = near; suffix W/B/A/C =
// white / blue-white / amber / cyan.

const NUM_PALETTES: usize = 6;

/// Build the six shared palette materials and return their handles.
fn build_palette(materials: &mut Assets<ColorMaterial>) -> [Handle<ColorMaterial>; NUM_PALETTES] {
    // Distant, cool-white (very dim)
    let d_white = materials.add(ColorMaterial::from_color(Color::linear_rgb(0.18, 0.18, 0.22)));
    // Distant, blue-tinted (very dim)
    let d_blue = materials.add(ColorMaterial::from_color(Color::linear_rgb(0.12, 0.15, 0.30)));
    // Mid-layer, neutral white
    let m_white = materials.add(ColorMaterial::from_color(Color::linear_rgb(0.55, 0.55, 0.60)));
    // Mid-layer, blue-white
    let m_blue = materials.add(ColorMaterial::from_color(Color::linear_rgb(0.45, 0.50, 0.85)));
    // Near, warm amber accent (sparse)
    let n_amber = materials.add(ColorMaterial::from_color(Color::linear_rgb(1.20, 0.80, 0.30)));
    // Near, cool cyan / HDR bloom
    let n_cyan = materials.add(ColorMaterial::from_color(Color::linear_rgb(0.80, 1.60, 2.20)));

    [d_white, d_blue, m_white, m_blue, n_amber, n_cyan]
}

/// Per-layer definition.
struct LayerDef {
    count:      u32,
    z:          f32,
    scale_lo:   f32,
    scale_hi:   f32,
    drift_lo:   f32,
    drift_hi:   f32,
    twink_amp:  f32, // fraction of base scale
    twink_freq_lo: f32,
    twink_freq_hi: f32,
    /// Palette indices this layer may draw from (picked via hash).
    palette:    &'static [usize],
}

/// Four depth layers, far → near (index 0 = furthest back).
const LAYERS: [LayerDef; 4] = [
    // Layer 0 — deep background, very small/dim/slow, cool tones
    LayerDef {
        count: 110,
        z: -50.0,
        scale_lo: 0.45, scale_hi: 0.80,
        drift_lo: 5.0,  drift_hi: 12.0,
        twink_amp: 0.08,
        twink_freq_lo: 0.3, twink_freq_hi: 1.0,
        palette: &[0, 1, 0, 0, 1, 0], // mostly d_white, sprinkle d_blue
    },
    // Layer 1 — mid-distant, slightly bigger, mostly white with blue accents
    LayerDef {
        count: 80,
        z: -48.5,
        scale_lo: 0.70, scale_hi: 1.20,
        drift_lo: 14.0, drift_hi: 24.0,
        twink_amp: 0.12,
        twink_freq_lo: 0.5, twink_freq_hi: 1.5,
        palette: &[2, 2, 3, 2, 3, 2], // mostly m_white, blue accent
    },
    // Layer 2 — mid-near, brighter, mixed white/blue, rare amber
    LayerDef {
        count: 50,
        z: -47.0,
        scale_lo: 1.00, scale_hi: 1.80,
        drift_lo: 28.0, drift_hi: 44.0,
        twink_amp: 0.18,
        twink_freq_lo: 0.8, twink_freq_hi: 2.2,
        palette: &[2, 3, 3, 4, 2, 3], // white/blue, 1-in-6 amber
    },
    // Layer 3 — nearest, HDR bloom, fast, cyan+amber accents
    LayerDef {
        count: 24,
        z: -45.5,
        scale_lo: 1.40, scale_hi: 2.60,
        drift_lo: 50.0, drift_hi: 78.0,
        twink_amp: 0.25,
        twink_freq_lo: 1.0, twink_freq_hi: 3.0,
        palette: &[5, 5, 4, 5, 3, 5], // mostly cyan bloom, amber + blue-white
    },
];

pub fn spawn_starfield(
    mut commands: Commands,
    mut meshes:   ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    bounds: Res<PlayBounds>,
) {
    let circle = meshes.add(Circle::new(1.0));
    let palette = build_palette(&mut materials);
    let field = bounds.half * FIELD_MARGIN;

    let mut seed: u32 = 0xDEAD_BEEF;

    for (li, layer) in LAYERS.iter().enumerate() {
        for i in 0..layer.count {
            // Unique seeds per attribute — stride by a large prime to avoid
            // correlation between fields for the same star index.
            let s0 = seed.wrapping_add(li as u32 * 100_003).wrapping_add(i * 17);

            let x       = randf(s0,              -field.x, field.x);
            let y       = randf(s0.wrapping_add(1), -field.y, field.y);
            let scale   = randf(s0.wrapping_add(2), layer.scale_lo, layer.scale_hi);
            let drift   = randf(s0.wrapping_add(3), layer.drift_lo, layer.drift_hi);
            let phase   = randf(s0.wrapping_add(4), 0.0, std::f32::consts::TAU);
            let freq    = randf(s0.wrapping_add(5), layer.twink_freq_lo, layer.twink_freq_hi);

            // Pick palette entry from the layer's weighted list.
            let palette_slot = ((rand01(s0.wrapping_add(6)) * layer.palette.len() as f32) as usize)
                .min(layer.palette.len() - 1);
            let mat = palette[layer.palette[palette_slot]].clone();

            seed = seed.wrapping_add(7);

            commands.spawn((
                Star {
                    drift,
                    twinkle_freq:  freq,
                    twinkle_phase: phase,
                    base_scale:    scale,
                    twinkle_amp:   layer.twink_amp,
                },
                Mesh2d(circle.clone()),
                MeshMaterial2d(mat),
                Transform::from_xyz(x, y, layer.z).with_scale(Vec3::splat(scale)),
            ));
        }
    }
}

/// Drift stars down (wrap at field edge) and pulse each star's scale to twinkle.
pub fn drift_stars(
    time:   Res<Time>,
    bounds: Res<PlayBounds>,
    mut q:  Query<(&Star, &mut Transform)>,
) {
    let dt      = time.delta_secs();
    let elapsed = time.elapsed_secs();
    let field_h = bounds.half.y * FIELD_MARGIN;
    let span    = field_h * 2.0;

    for (star, mut tf) in &mut q {
        // Vertical drift + vertical wrap.
        tf.translation.y -= star.drift * dt;
        if tf.translation.y < -field_h {
            tf.translation.y += span;
        }

        // Twinkle: subtle sinusoidal scale pulse, no material mutation.
        let pulse = 1.0 + star.twinkle_amp
            * (elapsed * star.twinkle_freq + star.twinkle_phase).sin();
        let s = star.base_scale * pulse;
        tf.scale = Vec3::splat(s);
    }
}
