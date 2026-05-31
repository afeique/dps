//! Composed, **translucent** death fireball — a layered burst that reads as a
//! billowing plasma cloud rather than a flat solid disc.
//!
//! On each enemy `Death` it spawns, in one cohesive per-element **palette**:
//!   • a soft **core glow** — three concentric *translucent* filled discs (a
//!     white-hot center → element mid → dim edge) that bloom outward then
//!     dissipate, a cheap fake radial gradient; and
//!   • a handful of **puffs** — small translucent discs scattered at jittered
//!     offsets so the silhouette is lumpy/irregular, never a clean circle.
//! These layer with the expanding shock **rings** (`reaction_fx`) and the
//! **particle** shards/embers (`asteroid_debris`) to give "combinations of
//! concentric circles, flat circles, and particles" (user request).
//!
//! Each layer is alpha-baked translucent AND HDR-bright, so it glows under Bloom
//! while letting the starfield show through. There is no per-frame alpha
//! animation (lyon shapes are pre-tessellated); instead each layer *blooms then
//! shrinks to nothing* (`tick_fireballs`), which reads as a puff dissipating.
//! Despawn is the shared `Lifetime`/`tick_lifetimes` path. Deterministic
//! (Wang-hashed off a `Local` counter), so no `rand` dependency. Modest size.

use crate::components::{Lifetime, Velocity};
use crate::messages::Death;
use crate::systems::enemy::element_for;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;
use std::f32::consts::TAU;

/// One translucent fireball layer. Its Transform scale blooms from ~0 up to
/// `peak` then shrinks back to 0 across its `Lifetime` (`tick_fireballs`).
#[derive(Component)]
pub struct Fireball {
    /// Peak radius multiplier (the disc is a unit-ish disc scaled to this).
    peak: f32,
    max_life: f32,
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

// ── palette + shape ──────────────────────────────────────────────────────────

/// A unit-radius filled 20-gon disc in `color` (translucent + HDR → a soft glow).
fn soft_disc(color: Color) -> Shape {
    let mut path = ShapePath::new();
    for i in 0..20 {
        let a = i as f32 / 20.0 * TAU;
        let p = Vec2::new(a.cos(), a.sin());
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    ShapeBuilder::with(&path.close()).fill(color).build()
}

/// Per-explosion palette from the kill's element: `(hot, mid, edge)` — one
/// cohesive colour family, each **translucent** HDR (alpha < 1 so the burst is
/// see-through; rgb > 1 so Bloom still flares it). The hot center is the element
/// pushed toward white-hot; mid is the element bright; edge is a dim halo.
fn palette(base: Color) -> (Color, Color, Color) {
    let l = base.to_linear();
    let hot = Color::linear_rgba(l.red * 3.0 + 2.5, l.green * 3.0 + 2.0, l.blue * 3.0 + 1.6, 0.50);
    let mid = Color::linear_rgba(l.red * 3.2, l.green * 3.2, l.blue * 3.2, 0.34);
    let edge = Color::linear_rgba(l.red * 2.0, l.green * 2.0, l.blue * 2.0, 0.20);
    (hot, mid, edge)
}

// ── spawn (reads Death) ──────────────────────────────────────────────────────

/// Build the layered translucent fireball at each enemy death. Player death
/// (`kind == None`) is skipped (its FX is the explosion/flash/shake).
pub fn spawn_fireball(mut commands: Commands, mut deaths: MessageReader<Death>, mut seed: Local<u32>) {
    for d in deaths.read() {
        let Some(kind) = d.kind else { continue };
        let (hot, mid, edge) = palette(element_for(kind).color());
        // Modest size; grows a little with boss tier / mini-boss (NOT huge).
        let scale = (1.0 + 0.35 * d.boss_tier as f32) * if d.mini_boss { 1.2 } else { 1.0 };
        let c = d.position;
        let z = 1.6_f32;

        // Core glow: 3 concentric translucent discs (dim wide edge → hot tight
        // center), each blooming + dissipating. The stacked alphas fake a soft
        // radial gradient so the core reads as glowing gas, not a hard disc.
        let mut layer = |peak: f32, color: Color, life: f32, dz: f32| {
            commands.spawn((
                Fireball { peak, max_life: life },
                soft_disc(color),
                Transform::from_translation(c.extend(z + dz)).with_scale(Vec3::splat(0.01)),
                Lifetime { seconds: life },
            ));
        };
        layer(26.0 * scale, edge, 0.50, 0.00); // faint outer halo
        layer(16.0 * scale, mid, 0.42, 0.01); // element body
        layer(9.0 * scale, hot, 0.34, 0.02); // white-hot core

        // Puffs: a few small translucent discs at jittered offsets so the cloud
        // is lumpy/irregular instead of a clean circle. Cycle the palette.
        let puffs = 5 + d.boss_tier as u32 * 2 + if d.mini_boss { 2 } else { 0 };
        for i in 0..puffs {
            *seed = seed.wrapping_add(1);
            let s = *seed;
            let ang = frand(s ^ 0x1, 0.0, TAU);
            let off = frand(s ^ 0x2, 6.0, 24.0) * scale;
            let pos = c + Vec2::new(ang.cos(), ang.sin()) * off;
            let peak = frand(s ^ 0x3, 5.0, 11.0) * scale;
            let life = frand(s ^ 0x4, 0.30, 0.55);
            let color = match i % 3 {
                0 => hot,
                1 => mid,
                _ => edge,
            };
            // Drift outward slowly so the cloud expands as it dissipates.
            let drift = Vec2::new(ang.cos(), ang.sin()) * frand(s ^ 0x5, 12.0, 40.0);
            commands.spawn((
                Fireball { peak, max_life: life },
                soft_disc(color),
                Transform::from_translation(pos.extend(z + 0.03)).with_scale(Vec3::splat(0.01)),
                Velocity(drift),
                Lifetime { seconds: life },
            ));
        }
    }
}

/// Bloom each fireball layer out then shrink it to nothing over its life: a fast
/// ease-out grow in the first ~30%, then a linear shrink — so it billows then
/// dissipates (the translucent disc fading to zero size reads as the puff
/// cooling/clearing). Pure presentation; despawn is `tick_lifetimes`.
pub fn tick_fireballs(mut q: Query<(&Fireball, &Lifetime, &mut Transform)>) {
    for (fb, life, mut tf) in &mut q {
        // progress 0 (just spawned) → 1 (about to despawn)
        let t = 1.0 - (life.seconds / fb.max_life).clamp(0.0, 1.0);
        let r = if t < 0.3 {
            // ease-out grow to the peak
            let k = t / 0.3;
            fb.peak * (1.0 - (1.0 - k) * (1.0 - k))
        } else {
            // linear shrink back to ~0
            let k = (t - 0.3) / 0.7;
            fb.peak * (1.0 - k)
        };
        tf.scale = Vec3::splat(r.max(0.01));
    }
}
