//! Procedural death **blast** — a volumetric-feeling explosion built from two
//! parts, replacing the old flat filled-circle fireball:
//!
//!   1. **Sphere-impostor puffs** — a cloud of lumpy blobs, each a triangle-fan
//!      mesh with a **per-vertex radial gradient** (hot opaque center → fully
//!      transparent element-tinted rim). That gradient is what the old version
//!      lacked: a uniform-colour fill reads as a flat paper disc, whereas a
//!      centre-bright / rim-transparent blob reads as a lit translucent sphere
//!      (the HDR-bright centre blooms into a glow). Several overlapping puffs at
//!      jittered offsets make a billowing cloud that **expands as it fades fast**.
//!
//!   2. **3D perspective rings** — the shock wavefronts are NOT flat overhead
//!      circles. Each ring is a circle living on a **tilted plane in 3D**, and
//!      `tick_blast_rings` rebuilds its mesh every frame by rotating the circle
//!      by the ring's orientation and **perspective-projecting** it (the same
//!      `x·f/(f+z)` divide `asteroid_debris::tumble_shards` uses for its shards).
//!      So a ring reads as an ellipse seen in perspective — foreshortened, with
//!      its near arc larger than its far arc — and that perspective *evolves* as
//!      the ring expands and its plane slowly tumbles. Rings spawn at varied
//!      tilts (one near-flat, one medium, one steep) for interesting angles.
//!
//! One per-element **palette** drives a whole blast (Pyro orange, Cryo blue-white,
//! Volt violet, …) so it's cohesive but enemies explode in different colours.
//! `AlphaMode2d` here has no `Add`, so glow comes from HDR colour + bloom under
//! plain `Blend`. Each layer owns a `ColorMaterial` whose alpha is faded per
//! frame (so the cloud dissipates like smoke, not pops). Deterministic
//! (Wang-hashed), modest size. Despawn via the shared `Lifetime` path; smoke
//! drift via the shared `Velocity`/`integrate` path.

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
    /// Hold full for the first half of life, then `pow(0.7)` falloff (hot core).
    Sustain,
    /// `pow(0.45)` — punchy then slow (smoke lingers).
    Smoke,
    /// Linear with remaining life (rings).
    Linear,
}

fn fade_alpha(fade: Fade, frac: f32) -> f32 {
    match fade {
        Fade::Sustain => {
            if frac > 0.5 { 1.0 } else { (frac / 0.5).powf(0.7) }
        }
        Fade::Smoke => frac.powf(0.45),
        Fade::Linear => frac,
    }
}

/// A sphere-impostor puff: a gradient blob that grows, spins, drifts, and fades.
#[derive(Component)]
pub struct BlastPuff {
    mat: Handle<ColorMaterial>,
    base_alpha: f32,
    peak: f32,
    grow_from: f32,
    max_life: f32,
    spin: f32,
    fade: Fade,
}

/// A 3D-oriented shock ring, rebuilt + perspective-projected each frame.
#[derive(Component)]
pub struct BlastRing {
    mesh: Handle<Mesh>,
    mat: Handle<ColorMaterial>,
    base_alpha: f32,
    peak: f32,
    grow_from: f32,
    max_life: f32,
    /// Orientation of the ring's plane in 3D (tilt away from the screen plane).
    rot: Quat,
    /// Slow tumble of that plane (rad/s) so the perspective shifts as it expands.
    spin: Vec3,
    /// Band half-width as a fraction of the ring radius.
    hw: f32,
    /// Perspective focal length (smaller = stronger foreshortening).
    focal: f32,
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

#[inline]
fn lin(c: Color) -> [f32; 3] {
    let l = c.to_linear();
    [l.red, l.green, l.blue]
}

// ── meshes ───────────────────────────────────────────────────────────────────

/// A lumpy unit-radius gradient blob (sphere impostor): a triangle fan with a
/// bright opaque `center_rgb` hub and `N` transparent rim vertices in `rim_rgb`,
/// rim radii jittered in `[lumpiness, 1.0]` so the outline is organic. The
/// per-vertex alpha gradient (1 at centre → 0 at rim) is what makes it read as a
/// soft round volume instead of a flat disc.
fn blob_mesh(seed: u32, lumpiness: f32, center_rgb: [f32; 3], rim_rgb: [f32; 3]) -> Mesh {
    const N: usize = 18;
    let mut pos: Vec<[f32; 3]> = Vec::with_capacity(N + 1);
    let mut col: Vec<[f32; 4]> = Vec::with_capacity(N + 1);
    pos.push([0.0, 0.0, 0.0]);
    col.push([center_rgb[0], center_rgb[1], center_rgb[2], 1.0]);
    for i in 0..N {
        let a = i as f32 / N as f32 * TAU;
        let r = frand(seed ^ (i as u32).wrapping_mul(0x9E37_79B9), lumpiness, 1.0);
        pos.push([a.cos() * r, a.sin() * r, 0.0]);
        col.push([rim_rgb[0], rim_rgb[1], rim_rgb[2], 0.0]);
    }
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

/// Number of angular segments around a ring.
const RING_SEG: usize = 48;

/// Build a soft-edged ring band's vertex COLORS once (inner α0 → mid α1 → outer
/// α0, all in `rgb`). Positions are filled per-frame by `ring_positions`.
fn ring_colors(rgb: [f32; 3]) -> Vec<[f32; 4]> {
    let mut col = Vec::with_capacity(RING_SEG * 3);
    for _ in 0..RING_SEG {
        col.push([rgb[0], rgb[1], rgb[2], 0.0]); // inner edge
        col.push([rgb[0], rgb[1], rgb[2], 1.0]); // mid (bright)
        col.push([rgb[0], rgb[1], rgb[2], 0.0]); // outer edge
    }
    col
}

/// Triangle indices for the ring's inner→mid→outer band, closing the loop.
fn ring_indices() -> Vec<u32> {
    let mut idx = Vec::with_capacity(RING_SEG * 12);
    for i in 0..RING_SEG as u32 {
        let j = (i + 1) % RING_SEG as u32;
        let (i0, i1, i2) = (3 * i, 3 * i + 1, 3 * i + 2); // inner, mid, outer @ i
        let (j0, j1, j2) = (3 * j, 3 * j + 1, 3 * j + 2);
        idx.extend_from_slice(&[i0, i1, j1, i0, j1, j0]); // inner→mid quad
        idx.extend_from_slice(&[i1, i2, j2, i1, j2, j1]); // mid→outer quad
    }
    idx
}

/// Compute the ring's perspective-projected band positions for radius `r`,
/// orientation `rot`, band half-width fraction `hw`, focal `focal`. Each circle
/// point is placed in 3D on the tilted plane, then divided by `(1 + z/focal)` so
/// the part of the ring nearer the camera projects larger (true foreshortening).
fn ring_positions(r: f32, rot: Quat, hw: f32, focal: f32) -> Vec<[f32; 3]> {
    let mut pos = Vec::with_capacity(RING_SEG * 3);
    let project = |v: Vec3| -> [f32; 3] {
        let denom = (1.0 + v.z / focal).max(0.3);
        [v.x / denom, v.y / denom, 0.0]
    };
    for i in 0..RING_SEG {
        let a = i as f32 / RING_SEG as f32 * TAU;
        let dir = rot * Vec3::new(a.cos(), a.sin(), 0.0); // tilted circle direction
        pos.push(project(dir * (r * (1.0 - hw))));
        pos.push(project(dir * r));
        pos.push(project(dir * (r * (1.0 + hw))));
    }
    pos
}

// ── spawn (reads Death) ──────────────────────────────────────────────────────

/// Build the puff cloud + 3D rings at each enemy death. Player death
/// (`kind == None`) is skipped (its FX is the flash/shake).
pub fn spawn_blast(
    mut commands: Commands,
    mut deaths: MessageReader<Death>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut seed: Local<u32>,
) {
    for d in deaths.read() {
        let Some(kind) = d.kind else { continue };
        let el = lin(element_for(kind).color());
        // Palette family (all from the element): a hot near-white centre, the
        // bright element, and a dim smoke. Centre is HDR (blooms) but tinted, not
        // pure white, so the colour survives and it doesn't wash out.
        let hot = [el[0] * 2.0 + 1.4, el[1] * 2.0 + 1.1, el[2] * 2.0 + 0.9];
        let elem = [el[0] * 2.2, el[1] * 2.2, el[2] * 2.2];
        let smoke = [el[0] * 1.0 + 0.05, el[1] * 1.0 + 0.06, el[2] * 1.0 + 0.09];
        let ring_rgb = [el[0] * 2.6 + 0.3, el[1] * 2.6 + 0.3, el[2] * 2.6 + 0.3];

        let scale = (1.0 + 0.35 * d.boss_tier as f32) * if d.mini_boss { 1.2 } else { 1.0 };
        let c = d.position;

        // Spawn one gradient puff (own material so its alpha fades independently).
        #[allow(clippy::too_many_arguments)]
        let puff = |commands: &mut Commands,
                        meshes: &mut Assets<Mesh>,
                        materials: &mut Assets<ColorMaterial>,
                        s: u32,
                        off: Vec2,
                        peak: f32,
                        grow_from: f32,
                        center: [f32; 3],
                        rim: [f32; 3],
                        base_alpha: f32,
                        life: f32,
                        z: f32,
                        drift: Vec2,
                        fade: Fade| {
            let mat = materials.add(ColorMaterial::from(Color::linear_rgba(
                1.0, 1.0, 1.0, base_alpha,
            )));
            commands.spawn((
                BlastPuff {
                    mat: mat.clone(),
                    base_alpha,
                    peak,
                    grow_from,
                    max_life: life,
                    spin: frand(s ^ 0x5A, -1.6, 1.6),
                    fade,
                },
                Mesh2d(meshes.add(blob_mesh(s, frand(s ^ 0x7C, 0.45, 0.7), center, rim))),
                MeshMaterial2d(mat),
                Transform::from_translation((c + off).extend(z))
                    .with_scale(Vec3::splat(peak * grow_from)),
                Velocity(drift),
                Lifetime { seconds: life },
            ));
        };

        // ── Hot core: 3 small bright blobs (centre hot → rim element), sustain-fade.
        *seed = seed.wrapping_add(1);
        let s0 = *seed;
        puff(&mut commands, &mut meshes, &mut materials, s0 ^ 0x11, Vec2::ZERO, 12.0 * scale, 0.4, hot, elem, 0.55, 0.42, 1.66, Vec2::ZERO, Fade::Sustain);
        puff(&mut commands, &mut meshes, &mut materials, s0 ^ 0x22, Vec2::ZERO, 8.5 * scale, 0.4, hot, elem, 0.5, 0.36, 1.67, Vec2::ZERO, Fade::Sustain);
        puff(&mut commands, &mut meshes, &mut materials, s0 ^ 0x33, Vec2::ZERO, 5.0 * scale, 0.4, hot, hot, 0.6, 0.3, 1.68, Vec2::ZERO, Fade::Sustain);

        // ── Smoke: dim element/grey blobs drifting outward, slow fade — the cloud.
        let puffs = 7 + d.boss_tier as u32 * 2 + if d.mini_boss { 2 } else { 0 };
        for i in 0..puffs {
            *seed = seed.wrapping_add(1);
            let s = *seed;
            let ang = frand(s ^ 0x1, 0.0, TAU);
            let dir = Vec2::new(ang.cos(), ang.sin());
            let off = dir * frand(s ^ 0x2, 5.0, 18.0) * scale;
            let peak = frand(s ^ 0x3, 8.0, 16.0) * scale;
            let life = frand(s ^ 0x4, 0.6, 0.95);
            let (center, rim) = if i % 3 == 0 { (elem, smoke) } else { (smoke, smoke) };
            let a = if i % 3 == 0 { 0.24 } else { 0.18 };
            let drift = dir * frand(s ^ 0x5, 18.0, 50.0);
            puff(&mut commands, &mut meshes, &mut materials, s, off, peak, 0.45, center, rim, a, life, 1.58, drift, Fade::Smoke);
        }

        // ── 3D rings: tilted shock wavefronts at varied angles (near-flat, medium,
        // steep) so they expand "from other perspectives", not just overhead.
        let tilts = [
            frand(s0 ^ 0x00C1, 0.15, 0.45),
            frand(s0 ^ 0x00C2, 0.7, 1.1),
            frand(s0 ^ 0x00C3, 1.0, 1.35),
        ];
        for (n, &tilt) in tilts.iter().enumerate() {
            *seed = seed.wrapping_add(1);
            let s = *seed;
            // Tilt the flat XY circle about a random in-plane axis → out of the
            // overhead plane. A small extra Z roll varies the silhouette.
            let axis_ang = frand(s ^ 0xA1, 0.0, TAU);
            let axis = Vec3::new(axis_ang.cos(), axis_ang.sin(), 0.0);
            let rot = Quat::from_axis_angle(axis, tilt)
                * Quat::from_rotation_z(frand(s ^ 0xA2, 0.0, TAU));
            let spin = Vec3::new(
                frand(s ^ 0xB1, -0.8, 0.8),
                frand(s ^ 0xB2, -0.8, 0.8),
                frand(s ^ 0xB3, -0.4, 0.4),
            );
            let peak = (64.0 + 26.0 * n as f32) * scale;
            let life = frand(s ^ 0xC1, 0.45, 0.7);
            let hw = frand(s ^ 0xC2, 0.05, 0.09);
            let focal = 260.0;
            let base_alpha = 0.5;
            let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
                .with_inserted_attribute(
                    Mesh::ATTRIBUTE_POSITION,
                    ring_positions(peak * 0.15, rot, hw, focal),
                )
                .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, ring_colors(ring_rgb))
                .with_inserted_indices(Indices::U32(ring_indices()));
            let mh = meshes.add(mesh);
            let mat =
                materials.add(ColorMaterial::from(Color::linear_rgba(1.0, 1.0, 1.0, base_alpha)));
            commands.spawn((
                BlastRing {
                    mesh: mh.clone(),
                    mat: mat.clone(),
                    base_alpha,
                    peak,
                    grow_from: 0.15,
                    max_life: life,
                    rot,
                    spin,
                    hw,
                    focal,
                },
                Mesh2d(mh),
                MeshMaterial2d(mat),
                Transform::from_translation(c.extend(1.7 + 0.01 * n as f32)),
                Lifetime { seconds: life },
            ));
        }
    }
}

// ── tick ─────────────────────────────────────────────────────────────────────

/// Grow + spin each puff and fade its material alpha (smoke dissipating). Drift
/// is the shared `Velocity`/`integrate` path; despawn the shared `Lifetime` path.
pub fn tick_blast_puffs(
    time: Res<Time>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut q: Query<(&BlastPuff, &Lifetime, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (p, life, mut tf) in &mut q {
        let frac = (life.seconds / p.max_life).clamp(0.0, 1.0);
        let tt = 1.0 - frac;
        let e = 1.0 - (1.0 - tt).powi(3); // cubic-out grow
        let g = p.grow_from + (1.0 - p.grow_from) * e;
        tf.scale = Vec3::splat((p.peak * g).max(0.01));
        tf.rotation *= Quat::from_rotation_z(p.spin * dt);
        if let Some(m) = materials.get_mut(&p.mat) {
            m.color.set_alpha(p.base_alpha * fade_alpha(p.fade, frac));
        }
    }
}

/// Expand each 3D ring, tumble its plane, rebuild its perspective-projected band
/// mesh, and fade it. The per-frame re-projection is what gives the evolving
/// 3D perspective as the ring grows.
pub fn tick_blast_rings(
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut q: Query<(&mut BlastRing, &Lifetime)>,
) {
    let dt = time.delta_secs();
    for (mut ring, life) in &mut q {
        let frac = (life.seconds / ring.max_life).clamp(0.0, 1.0);
        let tt = 1.0 - frac;
        let e = 1.0 - (1.0 - tt).powi(3); // fast cubic-out expand
        let r = ring.peak * (ring.grow_from + (1.0 - ring.grow_from) * e);
        // Tumble the ring's plane so the perspective shifts as it expands.
        let spin = ring.spin;
        ring.rot = (Quat::from_scaled_axis(spin * dt) * ring.rot).normalize();
        let (rot, hw, focal) = (ring.rot, ring.hw, ring.focal);
        if let Some(mesh) = meshes.get_mut(&ring.mesh) {
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, ring_positions(r, rot, hw, focal));
        }
        if let Some(m) = materials.get_mut(&ring.mat) {
            m.color.set_alpha(ring.base_alpha * fade_alpha(Fade::Linear, frac));
        }
    }
}
