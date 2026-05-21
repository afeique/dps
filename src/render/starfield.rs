//! Arena parallax starfield (`docs/port-plan.md` §3.4).
//!
//! Stars are scattered in four depth layers (z ≈ -50 .. -45.5) and **do not
//! scroll**. Instead each star has a parallax factor: every frame the star is
//! repositioned at `base_pos − player_pos × parallax` so distant layers barely
//! shift while near layers shift visibly as the player moves — giving an
//! organic sense of depth without any camera movement.
//!
//! Shapes, colors, and twinkle math are ported from the original JS source:
//!   - `STAR_SHAPES` / `BIG_STAR_SHAPES` from constants.js
//!   - color roll probabilities from background-star.js (40% blue-white,
//!     20% pure-white, 12% cyan-white, 13% gold, 15% saturated nebula tint)
//!   - `NORMAL_STAR_COLORS` palette (18-entry nebula gamut) from constants.js
//!   - twinkle math from webgl-starfield-renderer.js:
//!       wave   = 0.5 + 0.5 * sin(time * speed + phase)
//!       pulse  = 0.94 + 0.18 * wave          (size pulse, ±12% breathing)
//!       blink  = pow(0.5 + 0.5 * sin(time*5 + phase*7), 8)  (5 Hz dip to 0.45)
//!       scale  = base * twinkle_amp_factor * pulse * mix(1.0, 0.55, blink_dip)
//!   - twinkle speed range 0.004–0.018 rad/tick → converted to rad/sec
//!
//! Shapes built:
//!   point/circle (Circle::new), diamond (Rhombus), triangle/hexagon
//!   (RegularPolygon), star4/star5/star6 (hand-built TriangleList fan mesh).
//!   ~85% of stars are small point/circle; ~15% are geometric / pointed.
//!   Far layers (layer 0, z=-50) are forced to point/circle.
//!   Near/bright tier uses HDR linear-RGB > 1.0 for bloom contribution.
//!
//! One shared Mesh2d handle per shape + one shared MeshMaterial2d per palette
//! entry so sprites batch efficiently.

use crate::components::Ship;
use crate::resources::PlayBounds;
use bevy::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use std::f32::consts::TAU;

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
    /// Slow twinkle oscillation frequency in radians/second.
    /// Derived from JS range 0.004–0.018 rad/tick × 60 Hz = 0.24–1.08 rad/s.
    pub twinkle_freq: f32,
    /// Per-star phase offset so stars don't all pulse together.
    pub twinkle_phase: f32,
    /// Whether this star participates in the 5 Hz "blink dip" (JS: sparse,
    /// more visible on near-layer stars).
    pub blink: bool,
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

// ── Shape enum ────────────────────────────────────────────────────────────────

/// The small set of distinct shape slots; index into the shared mesh array.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StarShape {
    /// Tiny filled circle — the dominant "point" shape (JS: 'point'/'circle').
    Point = 0,
    /// Diamond / rhombus (JS: 'diamond').
    Diamond = 1,
    /// Equilateral triangle (JS: 'triangle').
    Triangle = 2,
    /// Regular hexagon (JS: 'hexagon').
    Hexagon = 3,
    /// 4-pointed star (JS: 'star4').
    Star4 = 4,
    /// 5-pointed star (JS: 'star5').
    Star5 = 5,
    /// 6-pointed star (JS: 'star6').
    Star6 = 6,
}

const NUM_SHAPES: usize = 7;

/// JS `STAR_SHAPES` pool (18 entries) reproduced as a weighted draw table.
/// point×5, circle×1, diamond×2, triangle×1, hexagon×1, star4×1, star5×1,
/// star6×1, star8→star6 (closest 6-pt), cube/oct/tetra/prism→Point (3D solids
/// rendered as simple points in 2-D context).
/// 18 slots → ~55% point, 11% diamond, rest geometric/pointed.
const SMALL_STAR_POOL: [StarShape; 18] = [
    StarShape::Point,   // 'point'
    StarShape::Point,   // 'point'
    StarShape::Point,   // 'point'
    StarShape::Point,   // 'point'
    StarShape::Point,   // 'point'
    StarShape::Point,   // 'circle'  (same mesh as point at small scale)
    StarShape::Diamond, // 'diamond'
    StarShape::Diamond, // 'diamond'
    StarShape::Triangle,// 'triangle'
    StarShape::Hexagon, // 'hexagon'
    StarShape::Star4,   // 'star4'
    StarShape::Star5,   // 'star5'
    StarShape::Star6,   // 'star6'
    StarShape::Star6,   // 'star8' → mapped to star6 (2-D closest equivalent)
    StarShape::Point,   // 'cube'   → point (3-D solid; renders as dot in 2-D)
    StarShape::Point,   // 'octahedron' → point
    StarShape::Point,   // 'tetrahedron' → point
    StarShape::Point,   // 'prism' → point
];

/// JS `BIG_STAR_SHAPES` = ['point', 'circle'] — big stars are always simple.
const BIG_STAR_POOL: [StarShape; 2] = [StarShape::Point, StarShape::Point];

// ── Colour palette ────────────────────────────────────────────────────────────
//
// Two sources merged:
//
// A. COLOR ROLL (background-star.js lines 45-76):
//    40% blue-white  → bright cool-white with blue bias
//    20% pure white  → neutral bright white
//    12% cyan-white  → cooler with green+blue push
//    13% warm gold   → warm yellow-orange
//    15% saturated nebula tints: violet/magenta/amber/emerald/neon-blue/gold
//
// B. NORMAL_STAR_COLORS (constants.js lines 296-307, 18 entries):
//    '#7da9ff','#52d6ff','#3effff','#5cf2c8',  ← cool blues/cyans
//    '#b87dff','#d65cff','#ff66e0','#ff5cad',  ← purples/pinks
//    '#ffd75c','#ffb347','#ff7e5c','#ff5c5c',  ← warms
//    '#5cff9d','#9eff5c','#bdff52',            ← greens
//    '#a6b3ff','#a6f3e8','#c3a6ff'             ← pastels
//
// Palette slots 0-9 mirror the layer structure; slots 10-27 are the full
// NORMAL_STAR_COLORS set for use by near/mid-near layers' saturated tint
// roll. Slots 28-29 are HDR (>1.0) for the nearest bloom layer.
//
// All values converted from sRGB hex to *linear* RGB:
//   linear = (sRGB/255)^2.2  (approximate gamma decode)
// HDR near-layer values intentionally exceed 1.0 for Bevy bloom.

// Linear-RGB palette (hand-converted from the JS hex values above).
// Format: (r, g, b) linear f32.
struct PalEntry(f32, f32, f32);

/// Full palette.  Indices match `PAL_*` constants below.
const PALETTE: &[PalEntry] = &[
    // ── 0..9: background-star.js color roll ──────────────────────────────────
    // 0  blue-white (40%): brightness≈240, b+15, g+5 → (240,245,255)/255^2.2
    PalEntry(0.851, 0.902, 1.000),
    // 1  pure white (20%): brightness≈240 → (240,240,240)
    PalEntry(0.878, 0.878, 0.878),
    // 2  cyan-white (12%): (225,250,255) → slightly greener/bluer
    PalEntry(0.769, 0.955, 1.000),
    // 3  warm gold (13%): brightness r+5, g, b-15 → (245,240,225)
    PalEntry(0.921, 0.878, 0.769),
    // 4  nebula violet: (180,100,255)
    PalEntry(0.471, 0.140, 1.000),
    // 5  nebula magenta: (255,90,220)
    PalEntry(1.000, 0.106, 0.718),
    // 6  nebula amber: (255,130,90)
    PalEntry(1.000, 0.248, 0.106),
    // 7  nebula emerald: (120,255,180)
    PalEntry(0.212, 1.000, 0.463),
    // 8  nebula neon-blue: (90,200,255)
    PalEntry(0.106, 0.584, 1.000),
    // 9  nebula gold: (255,200,60)
    PalEntry(1.000, 0.584, 0.049),
    // ── 10..27: NORMAL_STAR_COLORS from constants.js ──────────────────────────
    // 10  #7da9ff → (125,169,255)
    PalEntry(0.224, 0.432, 1.000),
    // 11  #52d6ff → (82,214,255)
    PalEntry(0.090, 0.695, 1.000),
    // 12  #3effff → (62,255,255)
    PalEntry(0.055, 1.000, 1.000),
    // 13  #5cf2c8 → (92,242,200)
    PalEntry(0.118, 0.897, 0.584),
    // 14  #b87dff → (184,125,255)
    PalEntry(0.506, 0.224, 1.000),
    // 15  #d65cff → (214,92,255)
    PalEntry(0.695, 0.114, 1.000),
    // 16  #ff66e0 → (255,102,224)
    PalEntry(1.000, 0.149, 0.753),
    // 17  #ff5cad → (255,92,173)
    PalEntry(1.000, 0.114, 0.432),
    // 18  #ffd75c → (255,215,92)
    PalEntry(1.000, 0.706, 0.118),
    // 19  #ffb347 → (255,179,71)
    PalEntry(1.000, 0.479, 0.071),
    // 20  #ff7e5c → (255,126,92)
    PalEntry(1.000, 0.224, 0.118),
    // 21  #ff5c5c → (255,92,92)
    PalEntry(1.000, 0.118, 0.118),
    // 22  #5cff9d → (92,255,157)
    PalEntry(0.118, 1.000, 0.369),
    // 23  #9eff5c → (158,255,92)
    PalEntry(0.369, 1.000, 0.118),
    // 24  #bdff52 → (189,255,82)
    PalEntry(0.533, 1.000, 0.090),
    // 25  #a6b3ff → (166,179,255)
    PalEntry(0.424, 0.490, 1.000),
    // 26  #a6f3e8 → (166,243,232)
    PalEntry(0.424, 0.906, 0.827),
    // 27  #c3a6ff → (195,166,255)
    PalEntry(0.580, 0.424, 1.000),
    // ── 28..29: HDR near-layer (>1.0 linear) for Bevy bloom ──────────────────
    // 28  HDR cyan — adapted from webgl-starfield near tier + bloom intent
    PalEntry(0.70, 1.80, 2.40),
    // 29  HDR warm-white — hottest near-layer accent
    PalEntry(2.20, 2.00, 1.80),
];

fn build_palette(materials: &mut Assets<ColorMaterial>) -> Vec<Handle<ColorMaterial>> {
    PALETTE
        .iter()
        .map(|p| {
            materials.add(ColorMaterial::from_color(Color::linear_rgb(p.0, p.1, p.2)))
        })
        .collect()
}

// ── Pointed-star mesh builder ─────────────────────────────────────────────────

/// Build a 2-D N-pointed star as a `TriangleList` mesh (triangle fan from
/// centre to alternating outer/inner radius vertices).
///
/// Mirrors JS `_strokeNStar` inner-ratio values:
///   star4 → inner_ratio 0.45,  star5 → 0.42,  star6 → 0.50
fn build_star_mesh(
    points: u32,
    outer_r: f32,
    inner_ratio: f32,
    meshes: &mut Assets<Mesh>,
) -> Handle<Mesh> {
    let verts_count = (points * 2) as usize;
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(verts_count + 1);
    let mut normals:   Vec<[f32; 3]> = Vec::with_capacity(verts_count + 1);
    let mut uvs:       Vec<[f32; 2]> = Vec::with_capacity(verts_count + 1);
    let mut indices:   Vec<u32>      = Vec::with_capacity(points as usize * 3);

    // Vertex 0 = centre
    positions.push([0.0, 0.0, 0.0]);
    normals.push([0.0, 0.0, 1.0]);
    uvs.push([0.5, 0.5]);

    // Outer / inner rim vertices, alternating
    for i in 0..verts_count {
        // Start from -PI/2 so the first tip points up (JS: `-Math.PI / 2 + i*PI/points`)
        let angle = -std::f32::consts::FRAC_PI_2 + (i as f32 * std::f32::consts::PI) / points as f32;
        let r = if i % 2 == 0 { outer_r } else { outer_r * inner_ratio };
        let x = angle.cos() * r;
        let y = angle.sin() * r;
        positions.push([x, y, 0.0]);
        normals.push([0.0, 0.0, 1.0]);
        // UV: map from [-r, r] to [0, 1]
        uvs.push([(x / outer_r) * 0.5 + 0.5, (y / outer_r) * 0.5 + 0.5]);
    }

    // Triangle fan: centre(0), rim[i], rim[i+1 (wrapped)]
    for i in 0..verts_count as u32 {
        let next = (i % verts_count as u32) + 1;
        let wrap = if next as usize > verts_count { 1 } else { next };
        indices.push(0);
        indices.push(i + 1);
        indices.push(wrap);
    }

    let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices));

    meshes.add(mesh)
}

// ── Layer definitions ─────────────────────────────────────────────────────────

/// Field extends 20% beyond play-bounds so parallax offsets stay filled.
const FIELD_MARGIN: f32 = 1.20;

struct LayerDef {
    count:          u32,
    z:              f32,
    parallax_lo:    f32,
    parallax_hi:    f32,
    scale_lo:       f32,
    scale_hi:       f32,
    /// Twinkle amplitude range — mirrors JS twinkleAmplitude: random(0.10, 0.20)
    twink_amp_lo:   f32,
    twink_amp_hi:   f32,
    /// Twinkle freq in rad/s — derived from JS 0.004–0.018 rad/tick × 60 Hz:
    /// lo ≈ 0.004*60 = 0.24 rad/s,  hi ≈ 0.018*60 = 1.08 rad/s.
    /// Near layers get a higher multiplier (JS: `* (1 + z * 0.25)`).
    twink_freq_lo:  f32,
    twink_freq_hi:  f32,
    /// Probability 0..1 that a star in this layer gets the 5 Hz blink dip.
    blink_prob:     f32,
    /// Force all shapes to Point/circle (true for deep background).
    force_point:    bool,
    /// Allow big-star pool (JS: 15% big star chance for non-foreground).
    allow_big:      bool,
    /// Weighted palette index list; entry picked via hash.
    /// Indices reference `PALETTE` table above.
    palette:        &'static [usize],
}

/// Four layers, far → near.  Star counts: 110 + 85 + 55 + 30 = 280 total.
const LAYERS: [LayerDef; 4] = [
    // Layer 0 — deep background: barely moves, tiny, dim cool/warm whites.
    // force_point = true: JS color-star.js line 54-56: z>=2.0 → 'point'.
    LayerDef {
        count: 110, z: -50.0,
        parallax_lo: 0.015, parallax_hi: 0.035,
        scale_lo: 0.40,     scale_hi: 0.85,
        twink_amp_lo: 0.10, twink_amp_hi: 0.20,
        twink_freq_lo: 0.24, twink_freq_hi: 0.65,
        blink_prob: 0.00,
        force_point: true,
        allow_big: false,
        // JS color roll: 40% blue-white(0), 20% white(1), 12% cyan(2),
        // 13% gold(3), 15% nebula — dims; no near-layer HDR.
        // Weighted: 40% slot-0, 20% slot-1, 12% slot-2, 13% slot-3,
        //           15% saturated nebula (4..9) distributed evenly.
        palette: &[0, 0, 0, 0, 1, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 0, 1, 3, 2, 0],
    },
    // Layer 1 — mid-distant: slight parallax, neutral/blue whites + rare nebula.
    LayerDef {
        count: 85, z: -48.5,
        parallax_lo: 0.04, parallax_hi: 0.07,
        scale_lo: 0.65,    scale_hi: 1.30,
        twink_amp_lo: 0.10, twink_amp_hi: 0.22,
        twink_freq_lo: 0.30, twink_freq_hi: 0.80,
        blink_prob: 0.05,
        force_point: false,
        allow_big: false,
        // Mid layer: mostly blue-white (0) and white (1), sprinkled nebula.
        palette: &[0, 0, 1, 0, 2, 10, 11, 25, 1, 0, 3, 0, 26, 27, 1, 0, 2, 12, 0, 1],
    },
    // Layer 2 — mid-near: noticeable parallax, NORMAL_STAR_COLORS palette.
    LayerDef {
        count: 55, z: -47.0,
        parallax_lo: 0.08, parallax_hi: 0.12,
        scale_lo: 0.90,    scale_hi: 1.90,
        twink_amp_lo: 0.12, twink_amp_hi: 0.25,
        twink_freq_lo: 0.45, twink_freq_hi: 1.00,
        blink_prob: 0.12,
        force_point: false,
        allow_big: true,
        // Full NORMAL_STAR_COLORS range (10..27), plus background-roll whites.
        palette: &[0, 1, 10, 11, 12, 13, 14, 15, 16, 17,
                   18, 19, 20, 21, 22, 23, 24, 25, 26, 27],
    },
    // Layer 3 — nearest: strong parallax, HDR bloom, rich shapes.
    LayerDef {
        count: 30, z: -45.5,
        parallax_lo: 0.12, parallax_hi: 0.20,
        scale_lo: 1.30,    scale_hi: 2.80,
        twink_amp_lo: 0.15, twink_amp_hi: 0.28,
        twink_freq_lo: 0.60, twink_freq_hi: 1.08,
        blink_prob: 0.25,
        force_point: false,
        allow_big: true,
        // Near layer: NORMAL_STAR_COLORS + HDR slots 28/29 for bloom.
        palette: &[10, 11, 12, 13, 14, 15, 16, 17,
                   18, 19, 28, 28, 29, 27, 26, 22,
                   23, 24, 25, 28],
    },
];

// ── Startup system ────────────────────────────────────────────────────────────

pub fn spawn_starfield(
    mut commands:  Commands,
    mut meshes:    ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    bounds:        Res<PlayBounds>,
) {
    // ── Build shared mesh handles (one per shape) ────────────────────────────
    //
    // All shapes are built at unit scale (radius ≈ 1.0); the Transform scale
    // on each star entity controls actual rendered size.

    let mut shape_handles: [Option<Handle<Mesh>>; NUM_SHAPES] = Default::default();

    // Point / small circle (Circle radius 1.0, 8 segments — stays crisp at tiny scale)
    shape_handles[StarShape::Point   as usize] = Some(meshes.add(Circle::new(1.0).mesh().resolution(8).build()));
    // Diamond: Rhombus(horizontal_diagonal=2.0, vertical_diagonal=2.0) → square-on-edge
    shape_handles[StarShape::Diamond as usize] = Some(meshes.add(Rhombus::new(2.0, 2.0)));
    // Triangle: RegularPolygon 3 sides, circumradius 1.0
    shape_handles[StarShape::Triangle as usize] = Some(meshes.add(RegularPolygon::new(1.0, 3)));
    // Hexagon: RegularPolygon 6 sides, circumradius 1.0
    shape_handles[StarShape::Hexagon as usize] = Some(meshes.add(RegularPolygon::new(1.0, 6)));
    // Star4: 4-pointed star, inner_ratio 0.45 (JS: star4 → _strokeNStar(r,4,0.45))
    shape_handles[StarShape::Star4   as usize] = Some(build_star_mesh(4, 1.0, 0.45, &mut meshes));
    // Star5: 5-pointed star, inner_ratio 0.42 (JS: star5 → _strokeNStar(r,5,0.42))
    shape_handles[StarShape::Star5   as usize] = Some(build_star_mesh(5, 1.0, 0.42, &mut meshes));
    // Star6: 6-pointed star, inner_ratio 0.50 (JS: star6 → _strokeNStar(r,6,0.5))
    shape_handles[StarShape::Star6   as usize] = Some(build_star_mesh(6, 1.0, 0.50, &mut meshes));

    // ── Build shared material handles (one per palette entry) ────────────────
    let palette = build_palette(&mut materials);

    // ── Scatter the star field ───────────────────────────────────────────────
    // Field extends beyond play-area so parallax offsets stay filled.
    let field = bounds.half * FIELD_MARGIN;

    let mut seed: u32 = 0xDEAD_BEEF;

    for (li, layer) in LAYERS.iter().enumerate() {
        for i in 0..layer.count {
            // Stride seeds by a large prime per layer to de-correlate fields.
            let s0 = seed
                .wrapping_add(li as u32 * 100_003)
                .wrapping_add(i.wrapping_mul(17));

            let x        = randf(s0,                 -field.x, field.x);
            let y        = randf(s0.wrapping_add(1), -field.y, field.y);
            let scale    = randf(s0.wrapping_add(2),  layer.scale_lo,      layer.scale_hi);
            let parallax = randf(s0.wrapping_add(3),  layer.parallax_lo,   layer.parallax_hi);
            let phase    = randf(s0.wrapping_add(4),  0.0, TAU);
            let freq     = randf(s0.wrapping_add(5),  layer.twink_freq_lo, layer.twink_freq_hi);
            let amp      = randf(s0.wrapping_add(6),  layer.twink_amp_lo,  layer.twink_amp_hi);

            // Palette slot
            let palette_slot = ((rand01(s0.wrapping_add(7)) * layer.palette.len() as f32) as usize)
                .min(layer.palette.len() - 1);
            let mat = palette[layer.palette[palette_slot]].clone();

            // Shape selection
            // JS: 15% big star (big→BIG_STAR_POOL; small→SMALL_STAR_POOL; foreground→point).
            // layer.force_point overrides (deep background, matches JS z>=2 → 'point').
            let shape = if layer.force_point {
                StarShape::Point
            } else {
                let shape_roll = rand01(s0.wrapping_add(8));
                let big_roll   = rand01(s0.wrapping_add(9));
                if layer.allow_big && big_roll < 0.15 {
                    // Big star → simple pool only (JS BIG_STAR_SHAPES)
                    let idx = ((rand01(s0.wrapping_add(10)) * BIG_STAR_POOL.len() as f32) as usize)
                        .min(BIG_STAR_POOL.len() - 1);
                    BIG_STAR_POOL[idx]
                } else {
                    // Small star → full shape pool (JS STAR_SHAPES, 18 entries)
                    let idx = ((shape_roll * SMALL_STAR_POOL.len() as f32) as usize)
                        .min(SMALL_STAR_POOL.len() - 1);
                    SMALL_STAR_POOL[idx]
                }
            };

            // Blink participation
            let blink = rand01(s0.wrapping_add(11)) < layer.blink_prob;

            seed = seed.wrapping_add(13);

            let mesh_handle = shape_handles[shape as usize]
                .as_ref()
                .expect("all shape handles built")
                .clone();

            commands.spawn((
                Star {
                    base_pos:      Vec2::new(x, y),
                    parallax,
                    base_scale:    scale,
                    twinkle_amp:   amp,
                    twinkle_freq:  freq,
                    twinkle_phase: phase,
                    blink,
                },
                Mesh2d(mesh_handle),
                MeshMaterial2d(mat),
                Transform::from_xyz(x, y, layer.z).with_scale(Vec3::splat(scale)),
            ));
        }
    }
}

// ── Per-frame system ──────────────────────────────────────────────────────────

/// Reposition stars relative to the player (parallax) and pulse their scale
/// (twinkle + blink).  No vertical drift; no wrapping.
///
/// Twinkle math mirrors webgl-starfield-renderer.js vertex shader:
///   wave  = 0.5 + 0.5 * sin(time * speed + phase)       ← slow breathe
///   pulse = 0.94 + 0.18 * wave                           ← ±12% size pulse
///   blink = pow(0.5 + 0.5 * sin(time*5 + phase*7), 8)   ← 5 Hz spike
///   final = base_scale * ((1-amp) + amp*wave) * pulse * mix(1.0, 0.55, blink_active)
///
/// Because shared materials can't carry per-star alpha in a batched pipeline,
/// the combined effect is baked entirely into Transform.scale (scale-pulse is
/// the Bevy-friendly equivalent of the JS alpha×size pulse).
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

        // ── Slow twinkle (JS: wave = 0.5+0.5*sin(t*speed+phase)) ─────────────
        let wave = 0.5 + 0.5 * (elapsed * star.twinkle_freq + star.twinkle_phase).sin();

        // ── Size pulse (JS: pulse = 0.94 + 0.18 * wave) ──────────────────────
        // Quad grows ~12% at bright peak, shrinks ~6% at dim trough.
        let pulse = 0.94 + 0.18 * wave;

        // ── Alpha-equivalent twinkle applied to scale ─────────────────────────
        // JS: twinkle = (1-amp) + amp*wave   (opacity modulator)
        // We convert this to a scale modulator so it reads visually.
        let twinkle = (1.0 - star.twinkle_amp) + star.twinkle_amp * wave;

        // ── 5 Hz blink dip (JS: pow(0.5+0.5*sin(t*5+phase*7), 8)) ───────────
        // Produces narrow bright spikes separated by dim troughs.
        // Multiplier dips to 0.45 at trough (JS: 1-0.55 = 0.45 scale factor).
        let blink_mul = if star.blink {
            let blink_wave = 0.5 + 0.5 * (elapsed * 5.0 + star.twinkle_phase * 7.0).sin();
            let blink_peak = blink_wave.powf(8.0); // very narrow spikes
            // mix(1.0, 0.45, blink_peak): at peak→dip to 0.45, at trough→stay 1.0
            1.0 - blink_peak * 0.55
        } else {
            1.0
        };

        // Combine: base_scale × slow-twinkle × size-pulse × blink
        tf.scale = Vec3::splat(star.base_scale * twinkle * pulse * blink_mul);
    }
}
