//! Composed, **fading** death explosion — a billowing smoke cloud, not a flat
//! disc. Port of rainboids' layered `createExplosion` recipe
//! (`combat-manager.js` / `particle.js`), adapted to Bevy meshes.
//!
//! The key difference from a lyon `Shape` (whose colour is baked at spawn, so it
//! can only shrink): every layer here is a **mesh with its OWN `ColorMaterial`**,
//! and `tick_fireballs` **fades that material's alpha every frame** over the
//! layer's life — so the cloud *dissipates* like smoke (alpha → 0) rather than
//! growing then popping out of existence. Layers keep growing/drifting as they
//! fade, never shrinking to nothing.
//!
//! On each enemy `Death`, in one cohesive per-element **palette**:
//!   • **core spheres** — 3 concentric filled lumpy blobs (white-hot → warm →
//!     element), the "collection of spheres"; bright, sustain-then-fade, grow
//!     slightly, static (the anchored fireball).
//!   • **smoke puffs** — ~7 dim translucent lumpy blobs at offsets that drift
//!     out + fade slow (pow-0.45), so the silhouette billows irregularly.
//!   • **rings** — 2 expanding **annulus** rings (real circular rings, not flat
//!     discs) that grow outward and fade linearly — the pressure wavefront.
//! Modest size. Blobs are irregular (per-vertex jittered radius) so nothing
//! reads as a clean circle. Deterministic (Wang-hashed), so no `rand` dep.

use crate::components::{Lifetime, Velocity};
use crate::messages::Death;
use crate::systems::enemy::element_for;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use std::f32::consts::TAU;

/// Alpha-over-life curve for a layer.
#[derive(Clone, Copy, PartialEq)]
enum Fade {
    /// Hold full for the first half of life, then `pow(0.7)` falloff (cores).
    Sustain,
    /// `pow(0.45)` — punchy then slow (smoke, lingers).
    Smoke,
    /// Linear with life (rings).
    Linear,
}

/// One explosion layer: a mesh whose `ColorMaterial` alpha is faded each frame.
#[derive(Component)]
pub struct Fireball {
    /// This layer's own material (so its alpha can be faded independently).
    mat: Handle<ColorMaterial>,
    /// Pre-fade alpha at full strength (the curve scales this toward 0).
    base_alpha: f32,
    /// Peak radius in px (the unit-radius mesh is scaled to this).
    peak: f32,
    /// Scale fraction at spawn (grows from here toward 1.0 over life).
    grow_from: f32,
    /// Cubic-out grow (cores/smoke) vs linear grow (rings expand steadily).
    cubic: bool,
    max_life: f32,
    /// In-plane spin (rad/sec) so the lumpy outline turns as it dissipates.
    spin: f32,
    fade: Fade,
}

// ── deterministic hash ───────────────────────────────────────────────────────

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

// ── meshes ───────────────────────────────────────────────────────────────────

/// An irregular filled blob (a "sphere"): a unit-radius triangle-fan whose rim
/// radii are jittered in `[lumpiness, 1.0]`, so the outline is an organic lump,
/// not a circle. White vertex colour (the `ColorMaterial` supplies the real hue).
fn blob_mesh(seed: u32, lumpiness: f32) -> Mesh {
    const N: usize = 18;
    let mut pos: Vec<[f32; 3]> = Vec::with_capacity(N + 1);
    pos.push([0.0, 0.0, 0.0]); // center
    for i in 0..N {
        let a = i as f32 / N as f32 * TAU;
        let r = frand(seed ^ (i as u32).wrapping_mul(0x9E37_79B9), lumpiness, 1.0);
        pos.push([a.cos() * r, a.sin() * r, 0.0]);
    }
    let col = vec![[1.0_f32, 1.0, 1.0, 1.0]; N + 1];
    let mut idx: Vec<u32> = Vec::with_capacity(N * 3);
    for i in 0..N as u32 {
        idx.push(0);
        idx.push(1 + i);
        idx.push(1 + (i + 1) % N as u32);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
        .with_inserted_indices(Indices::U32(idx))
}

/// A unit-radius **annulus** ring of relative half-width `hw` (a real circular
/// ring band, not a filled disc), as a closed triangle strip.
fn ring_mesh(hw: f32) -> Mesh {
    const N: usize = 40;
    let (inner, outer) = (1.0 - hw, 1.0 + hw);
    let mut pos: Vec<[f32; 3]> = Vec::with_capacity(N * 2);
    for i in 0..N {
        let a = i as f32 / N as f32 * TAU;
        let (c, s) = (a.cos(), a.sin());
        pos.push([c * inner, s * inner, 0.0]);
        pos.push([c * outer, s * outer, 0.0]);
    }
    let col = vec![[1.0_f32, 1.0, 1.0, 1.0]; N * 2];
    let mut idx: Vec<u32> = Vec::with_capacity(N * 6);
    for i in 0..N as u32 {
        let a = 2 * i;
        let b = 2 * i + 1;
        let c = 2 * ((i + 1) % N as u32);
        let d = 2 * ((i + 1) % N as u32) + 1;
        idx.extend_from_slice(&[a, b, c, b, d, c]);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
        .with_inserted_indices(Indices::U32(idx))
}

// ── palette ──────────────────────────────────────────────────────────────────

/// Element base colour as a linear-rgb triple.
fn lin(c: Color) -> [f32; 3] {
    let l = c.to_linear();
    [l.red, l.green, l.blue]
}

// ── spawn (reads Death) ──────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn spawn_layer(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    mesh: Mesh,
    rgb: [f32; 3],
    alpha: f32,
    pos: Vec2,
    z: f32,
    peak: f32,
    grow_from: f32,
    cubic: bool,
    life: f32,
    spin: f32,
    drift: Vec2,
    fade: Fade,
) {
    // Translucent colour → `ColorMaterial::from` selects alpha-blend mode.
    let mat = materials.add(ColorMaterial::from(Color::linear_rgba(
        rgb[0], rgb[1], rgb[2], alpha,
    )));
    commands.spawn((
        Fireball {
            mat: mat.clone(),
            base_alpha: alpha,
            peak,
            grow_from,
            cubic,
            max_life: life,
            spin,
            fade,
        },
        Mesh2d(meshes.add(mesh)),
        MeshMaterial2d(mat),
        Transform::from_translation(pos.extend(z)).with_scale(Vec3::splat(peak * grow_from)),
        Velocity(drift),
        Lifetime { seconds: life },
    ));
}

/// Build the layered fading explosion at each enemy death. Player death
/// (`kind == None`) is skipped (its FX is the flash/shake).
pub fn spawn_fireball(
    mut commands: Commands,
    mut deaths: MessageReader<Death>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut seed: Local<u32>,
) {
    for d in deaths.read() {
        let Some(kind) = d.kind else { continue };
        let el = lin(element_for(kind).color());
        // Cohesive family: a white-hot center, a warm midtone, the element, and a
        // dim smoke — all derived from the element so the blast reads as one hue.
        let hot = [el[0] * 0.6 + 2.4, el[1] * 0.6 + 1.9, el[2] * 0.6 + 1.5];
        let warm = [el[0] * 1.4 + 0.8, el[1] * 1.2 + 0.4, el[2] * 1.2 + 0.2];
        let elem = [el[0] * 1.8, el[1] * 1.8, el[2] * 1.8];
        let smoke = [el[0] * 0.9 + 0.08, el[1] * 0.9 + 0.10, el[2] * 0.9 + 0.14];

        let scale = (1.0 + 0.35 * d.boss_tier as f32) * if d.mini_boss { 1.2 } else { 1.0 };
        let c = d.position;

        // ── Core "spheres": 3 concentric filled blobs, bright, sustain-fade,
        // grow a little (cubic), static. Outer element → warm → white-hot.
        *seed = seed.wrapping_add(1);
        let s0 = *seed;
        spawn_layer(&mut commands, &mut meshes, &mut materials, blob_mesh(s0 ^ 0x11, 0.6),
            elem, 0.45, c, 1.62, 13.0 * scale, 0.55, true, 0.55, frand(s0 ^ 0xA, -1.0, 1.0), Vec2::ZERO, Fade::Sustain);
        spawn_layer(&mut commands, &mut meshes, &mut materials, blob_mesh(s0 ^ 0x22, 0.65),
            warm, 0.6, c, 1.64, 9.0 * scale, 0.55, true, 0.5, frand(s0 ^ 0xB, -1.2, 1.2), Vec2::ZERO, Fade::Sustain);
        spawn_layer(&mut commands, &mut meshes, &mut materials, blob_mesh(s0 ^ 0x33, 0.72),
            hot, 0.8, c, 1.66, 5.5 * scale, 0.5, true, 0.42, frand(s0 ^ 0xC, -1.4, 1.4), Vec2::ZERO, Fade::Sustain);

        // ── Smoke puffs: dim translucent lumpy blobs that drift out + fade slow,
        // so the cloud billows irregularly (the "cloud of smoke").
        let puffs = 7 + d.boss_tier as u32 * 2 + if d.mini_boss { 2 } else { 0 };
        for i in 0..puffs {
            *seed = seed.wrapping_add(1);
            let s = *seed;
            let ang = frand(s ^ 0x1, 0.0, TAU);
            let dir = Vec2::new(ang.cos(), ang.sin());
            let off = dir * frand(s ^ 0x2, 5.0, 18.0) * scale;
            let peak = frand(s ^ 0x3, 5.0, 10.0) * scale;
            let life = frand(s ^ 0x4, 0.55, 0.95);
            let rgb = if i % 3 == 0 { elem } else { smoke };
            let a = if i % 3 == 0 { 0.30 } else { 0.20 };
            let drift = dir * frand(s ^ 0x5, 16.0, 46.0);
            spawn_layer(&mut commands, &mut meshes, &mut materials, blob_mesh(s, 0.4),
                rgb, a, c + off, 1.58, peak, 0.4, true, life, frand(s ^ 0x6, -1.6, 1.6), drift, Fade::Smoke);
        }

        // ── Rings: 2 expanding annulus rings (real circular rings), fade linear.
        spawn_layer(&mut commands, &mut meshes, &mut materials, ring_mesh(0.10),
            warm, 0.55, c, 1.70, 30.0 * scale, 0.2, false, 0.55, 0.0, Vec2::ZERO, Fade::Linear);
        spawn_layer(&mut commands, &mut meshes, &mut materials, ring_mesh(0.06),
            elem, 0.4, c, 1.71, 46.0 * scale, 0.15, false, 0.7, 0.0, Vec2::ZERO, Fade::Linear);
    }
}

/// Grow + spin each layer and **fade its material's alpha** over life, so the
/// cloud dissipates (alpha → 0) rather than vanishing by shrinking. Despawn is
/// the shared `Lifetime`/`tick_lifetimes` path; drift is `Velocity`/`integrate`.
pub fn tick_fireballs(
    time: Res<Time>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut q: Query<(&Fireball, &Lifetime, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (fb, life, mut tf) in &mut q {
        let frac = (life.seconds / fb.max_life).clamp(0.0, 1.0); // 1 fresh → 0 gone
        let tt = 1.0 - frac; // 0 fresh → 1 gone (grow progress)

        // Grow from `grow_from` toward 1.0 (never shrinks); rings grow steadily.
        let e = if fb.cubic { 1.0 - (1.0 - tt).powi(3) } else { tt };
        let g = fb.grow_from + (1.0 - fb.grow_from) * e;
        tf.scale = Vec3::splat((fb.peak * g).max(0.01));
        tf.rotation *= Quat::from_rotation_z(fb.spin * dt);

        // Fade alpha by the curve → smoke clearing.
        let a = match fb.fade {
            Fade::Sustain => {
                if frac > 0.5 { 1.0 } else { (frac / 0.5).powf(0.7) }
            }
            Fade::Smoke => frac.powf(0.45),
            Fade::Linear => frac,
        };
        if let Some(m) = materials.get_mut(&fb.mat) {
            m.color.set_alpha(fb.base_alpha * a);
        }
    }
}
