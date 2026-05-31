//! Composed, **translucent** death fireball — a layered burst that reads as a
//! billowing plasma cloud rather than a flat solid disc.
//!
//! On each enemy `Death`, in one cohesive per-element **palette**, it spawns a
//! mix of:
//!   • a small bright **core** blob — brighter so it blooms into a glow; plus
//!   • a few larger **haze** blobs + scattered **puff** blobs — all dim &
//!     *translucent* so the starfield shows through, and all built as
//!     **irregular lumpy polygons** (per-vertex jittered radius) so the
//!     silhouette is an organic cloud, never a clean circle.
//! Every blob blooms out, slowly spins, and shrinks to nothing (`tick_fireballs`)
//! — and the puffs drift outward — so the burst is *dynamic*, not a static disc.
//! These layer with the expanding shock **rings** (`reaction_fx`) and the
//! **particle** shards/embers (`asteroid_debris`) to give "combinations of
//! concentric circles, flat circles, and particles" (user request).
//!
//! The trick for translucency to actually read: the haze blobs are kept DIM
//! (rgb only modestly over 1) with low alpha, so after alpha-blending over the
//! dark field their effective brightness stays *below* the bloom threshold —
//! they render as see-through smoke, while only the small bright core blooms.
//! lyon `Mesh2d` renders in Bevy's alpha-blended Transparent2d phase, so the
//! baked-alpha fills blend correctly. Deterministic (Wang-hashed), modest size.

use crate::components::{Lifetime, Velocity};
use crate::messages::Death;
use crate::systems::enemy::element_for;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;
use std::f32::consts::TAU;

/// One translucent fireball layer. Its Transform scale blooms from ~0 up to
/// `peak` then shrinks back to 0 across its `Lifetime`; it also slowly spins
/// (`tick_fireballs`) so the lumpy outline turns as it dissipates.
#[derive(Component)]
pub struct Fireball {
    /// Peak radius (px) the blob grows to.
    peak: f32,
    max_life: f32,
    /// In-plane spin (rad/sec).
    spin: f32,
}

// ── deterministic hash (dependency-free) ─────────────────────────────────────

#[inline]
fn wang(mut x: u32) -> u32 {
    x = (x ^ 61) ^ (x >> 16);
    x = x.wrapping_add(x << 3);
    x ^= x >> 4;
    x = x.wrapping_mul(0x27d4_eb2d);
    x ^= x >> 15;
    x
}

#[inline]
fn frand(seed: u32, lo: f32, hi: f32) -> f32 {
    lo + (wang(seed) as f32 / u32::MAX as f32) * (hi - lo)
}

// ── shape: an irregular lumpy blob (NOT a clean circle) ──────────────────────

/// A unit-ish filled blob: a closed 18-gon whose per-vertex radius is jittered
/// in `[lumpiness, 1.0]` (deterministic from `seed`), so the outline is an
/// organic cloud rather than a circle. Lower `lumpiness` = more ragged.
fn blob_shape(seed: u32, color: Color, lumpiness: f32) -> Shape {
    const N: usize = 18;
    let mut path = ShapePath::new();
    for i in 0..N {
        let a = i as f32 / N as f32 * TAU;
        let r = frand(seed ^ (i as u32).wrapping_mul(0x9E37_79B9), lumpiness, 1.0);
        let p = Vec2::new(a.cos() * r, a.sin() * r);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    ShapeBuilder::with(&path.close()).fill(color).build()
}

// ── palette ──────────────────────────────────────────────────────────────────

/// Per-explosion palette from the kill's element: `(core, haze, smoke)` — one
/// cohesive colour family. `core` is a brightish hot center that still blooms;
/// `haze`/`smoke` are DIM + translucent so they read as see-through gas (their
/// effective brightness lands under the bloom threshold). Different enemies →
/// different element colours, but every layer of one blast shares the family.
fn palette(base: Color) -> (Color, Color, Color) {
    let l = base.to_linear();
    // Hot core: element pushed toward white-hot, fairly opaque → blooms to a glow.
    let core = Color::linear_rgba(l.red * 2.2 + 1.6, l.green * 2.2 + 1.1, l.blue * 2.2 + 0.8, 0.62);
    // Haze: dim element body, see-through.
    let haze = Color::linear_rgba(l.red * 1.7 + 0.15, l.green * 1.7 + 0.15, l.blue * 1.7 + 0.15, 0.24);
    // Smoke: dimmer + hue-shifted-cool edge, very see-through.
    let smoke = Color::linear_rgba(l.red * 1.2 + 0.1, l.green * 1.2 + 0.12, l.blue * 1.2 + 0.18, 0.15);
    (core, haze, smoke)
}

// ── spawn (reads Death) ──────────────────────────────────────────────────────

/// Build the layered translucent fireball at each enemy death. Player death
/// (`kind == None`) is skipped (its FX is the explosion/flash/shake).
pub fn spawn_fireball(
    mut commands: Commands,
    mut deaths: MessageReader<Death>,
    mut seed: Local<u32>,
) {
    for d in deaths.read() {
        let Some(kind) = d.kind else { continue };
        let (core, haze, smoke) = palette(element_for(kind).color());
        // Modest size; grows a little with boss tier / mini-boss (NOT huge).
        let scale = (1.0 + 0.35 * d.boss_tier as f32) * if d.mini_boss { 1.2 } else { 1.0 };
        let c = d.position;
        let z = 1.6_f32;

        // One blob layer at an offset, with its own lumpiness/spin/drift.
        let mut blob = |s: u32, off: Vec2, peak: f32, color: Color, lump: f32, life: f32, dz: f32| {
            let spin = frand(s ^ 0xB10B, -1.8, 1.8);
            let drift = if off == Vec2::ZERO {
                Vec2::ZERO
            } else {
                off.normalize_or_zero() * frand(s ^ 0xD12F, 14.0, 44.0)
            };
            commands.spawn((
                Fireball { peak, max_life: life, spin },
                blob_shape(s, color, lump),
                Transform::from_translation((c + off).extend(z + dz))
                    .with_scale(Vec3::splat(0.01)),
                Velocity(drift),
                Lifetime { seconds: life },
            ));
        };

        *seed = seed.wrapping_add(1);
        let base = *seed;
        // Smoke halo (big, ragged, very dim) → haze body (mid) → bright core
        // (small, rounder): stacked translucency fakes a soft radial gradient,
        // the irregular outlines make it a cloud not a disc.
        blob(base ^ 0x01, Vec2::ZERO, 24.0 * scale, smoke, 0.45, 0.62, 0.00);
        blob(base ^ 0x02, Vec2::ZERO, 16.0 * scale, haze, 0.55, 0.50, 0.01);
        blob(base ^ 0x03, Vec2::ZERO, 8.5 * scale, core, 0.72, 0.34, 0.02);

        // Puffs: small translucent lumpy blobs at jittered offsets that drift
        // out — they break the round silhouette and make the cloud billow.
        let puffs = 6 + d.boss_tier as u32 * 2 + if d.mini_boss { 2 } else { 0 };
        for i in 0..puffs {
            *seed = seed.wrapping_add(1);
            let s = *seed;
            let ang = frand(s ^ 0x1, 0.0, TAU);
            let off = Vec2::new(ang.cos(), ang.sin()) * frand(s ^ 0x2, 7.0, 22.0) * scale;
            let peak = frand(s ^ 0x3, 4.0, 9.0) * scale;
            let life = frand(s ^ 0x4, 0.35, 0.70);
            let color = if i % 3 == 0 { haze } else { smoke };
            blob(s, off, peak, color, 0.4, life, 0.03);
        }
    }
}

/// Bloom each fireball layer out, spin it, then shrink it to nothing over its
/// life: a fast ease-out grow in the first ~30%, then a linear shrink — so it
/// billows then dissipates (the translucent lumpy blob fading to zero size reads
/// as gas cooling/clearing). Pure presentation; despawn is `tick_lifetimes`.
pub fn tick_fireballs(time: Res<Time>, mut q: Query<(&Fireball, &Lifetime, &mut Transform)>) {
    let dt = time.delta_secs();
    for (fb, life, mut tf) in &mut q {
        // progress 0 (just spawned) → 1 (about to despawn)
        let t = 1.0 - (life.seconds / fb.max_life).clamp(0.0, 1.0);
        let r = if t < 0.3 {
            let k = t / 0.3;
            fb.peak * (1.0 - (1.0 - k) * (1.0 - k)) // ease-out grow to the peak
        } else {
            let k = (t - 0.3) / 0.7;
            fb.peak * (1.0 - k) // linear shrink back to ~0
        };
        tf.rotation *= Quat::from_rotation_z(fb.spin * dt);
        tf.scale = Vec3::splat(r.max(0.01));
    }
}
