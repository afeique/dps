//! Asteroid death burst — the port of rainboids' `createDebris`
//! (`combat-manager.js` §7b) + `asteroid-shard.js`. When a player bullet
//! shatters an asteroid (`systems::asteroids::asteroid_hits` → `AsteroidShatter`),
//! we spawn a radial fan of **3D-tumbling wireframe triangles** plus one
//! expanding colored ring — all tinted from the rock's own live HSL so the burst
//! reads as that specific rock breaking apart, matching the wireframe-asteroid
//! visual language (stroke-only triangles, no fill) rather than a generic puff.
//!
//! Each shard is an equilateral triangle in 3D unit space, spun on all three
//! axes (`spin`) and perspective-projected each frame; `tumble_shards` rebuilds
//! its 3-edge wireframe mesh, fades its HDR brightness with remaining life (so
//! Bloom flares it bright then lets it die to black against the starfield), and
//! applies the source's mild drag so the fan drifts to rest as it fades. Motion
//! is the shared `Velocity`/`integrate` path; despawn is the shared `Lifetime`/
//! `tick_lifetimes` path — no bespoke lifecycle. Deterministic (Wang-hashed off a
//! `Local` counter), so it needs no `rand` dependency.

use crate::components::{Lifetime, Velocity};
use crate::messages::AsteroidShatter;
use crate::render::reaction_fx::{Shockwave, unit_ring};
use crate::systems::asteroids::AsteroidMaterial;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use std::f32::consts::TAU;

/// Equilateral triangle in 3D unit space (XY plane), centered at the origin —
/// the shared shard silhouette (matches `asteroid-shard.js` `BASE_VERTS`).
const BASE_VERTS: [Vec3; 3] = [
    Vec3::new(-1.0, -0.577, 0.0),
    Vec3::new(1.0, -0.577, 0.0),
    Vec3::new(0.0, 1.155, 0.0),
];

/// The triangle's 3 edges (vertex-index pairs) for the wireframe build.
const TRI_EDGES: [(usize, usize); 3] = [(0, 1), (1, 2), (2, 0)];

/// Perspective focal length for the shard's 3D→2D projection (smaller = more
/// "flip in/out of the page" wobble; matches `asteroid-shard.js` `FOCAL`).
const FOCAL: f32 = 100.0;
/// Half-width (px) of each wireframe edge quad — thinner than the asteroid
/// struts (1.6) since shards are small.
const EDGE_HALF_W: f32 = 1.0;
/// HDR gain on the shard color so Bloom turns it neon; fades to ~0 with life.
const HDR_GAIN: f32 = 2.4;
/// Extra HDR gain on the wavefront ring so it pops brighter than the shards.
const RING_GAIN: f32 = 3.2;

// ─── deterministic hash helpers (dependency-free, like systems::asteroids) ───

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
    let t = (wang(seed) as f32) / (u32::MAX as f32);
    lo + t * (hi - lo)
}

/// HSL → linear RGB triple (the base color the per-frame HDR brightness scales).
fn hue_lin(h: f32, s: f32, l: f32) -> [f32; 3] {
    let c = Color::hsl(h.rem_euclid(360.0), s, l).to_linear();
    [c.red, c.green, c.blue]
}

// ─── Component ───────────────────────────────────────────────────────────────

/// One wireframe-triangle shard. `rot` tumbles by `spin` each frame; `size`
/// scales the triangle; `color` is the pre-HDR-gain base, and `max_life` lets the
/// fade scale against the shared `Lifetime`.
#[derive(Component)]
pub struct AsteroidShard {
    rot: Quat,
    /// Angular velocity per axis (rad/sec).
    spin: Vec3,
    size: f32,
    color: [f32; 3],
    max_life: f32,
}

// ─── Spawn (reads AsteroidShatter) ───────────────────────────────────────────

/// On each `AsteroidShatter`, spawn the radial shard fan + one colored ring.
/// Shard count, fly-out speed, and ring radius scale with the rock's size; the
/// color palette (base / bright / dim / white-spark cycle) is derived from its
/// live hue — the §7b recipe. No `Assets` needed here: the shard mesh is attached
/// lazily by `tumble_shards` (so this stays cheap and asset-free).
pub fn spawn_asteroid_debris(
    mut commands: Commands,
    mut reader: MessageReader<AsteroidShatter>,
    // Advances per shard so repeated bursts don't stamp identical fans.
    mut seed: Local<u32>,
) {
    for ev in reader.read() {
        let size_scale = (ev.radius / 25.0).clamp(0.4, 1.5);
        let base = hue_lin(ev.hue, ev.sat, ev.light);
        let bright = hue_lin(ev.hue, ev.sat, (ev.light + 0.20).min(0.95));
        let dim = hue_lin(ev.hue + 20.0, ev.sat, (ev.light - 0.15).max(0.40));

        // Wavefront ring — reuse the reaction shockwave (expand + fade + despawn
        // are already handled by `reaction_fx::tick_shockwaves`).
        let ring = Color::linear_rgb(base[0] * RING_GAIN, base[1] * RING_GAIN, base[2] * RING_GAIN);
        commands.spawn((
            Shockwave { age: 0.0, peak: (ev.radius * 2.4).clamp(40.0, 170.0) },
            unit_ring(ring),
            Transform::from_translation(ev.center.extend(1.7)).with_scale(Vec3::splat(1.0)),
        ));

        // 10 shards for small rocks → ~28 for the largest (§7b `10 + 12*scale`).
        let count = (10.0 + 12.0 * size_scale) as u32;
        for i in 0..count {
            *seed = seed.wrapping_add(1);
            let s = *seed;
            // Evenly-spaced angles + jitter so the burst reads as an organic
            // shatter, not a fixed pinwheel.
            let angle = (i as f32 / count as f32) * TAU + frand(s ^ 0x1, -0.35, 0.35);
            let speed = frand(s ^ 0x2, 110.0, 280.0) * size_scale.max(0.6);
            let size = frand(s ^ 0x3, 4.0, 9.0);
            let max_life = frand(s ^ 0x4, 1.1, 1.7);
            // Most shards take the rock's color; every 5th is a white spark pop.
            let color = if i % 5 == 0 {
                [1.0, 1.0, 1.0]
            } else if i % 3 == 0 {
                bright
            } else if i % 3 == 1 {
                base
            } else {
                dim
            };
            let dir = Vec2::new(angle.cos(), angle.sin());
            commands.spawn((
                AsteroidShard {
                    rot: Quat::from_euler(
                        EulerRot::XYZ,
                        frand(s ^ 0x5, 0.0, TAU),
                        frand(s ^ 0x6, 0.0, TAU),
                        frand(s ^ 0x7, 0.0, TAU),
                    ),
                    spin: Vec3::new(
                        frand(s ^ 0x8, -5.0, 5.0),
                        frand(s ^ 0x9, -7.0, 7.0),
                        frand(s ^ 0xA, -9.0, 9.0),
                    ),
                    size,
                    color,
                    max_life,
                },
                Transform::from_translation(ev.center.extend(0.16)),
                Velocity(dir * speed),
                Lifetime { seconds: max_life },
            ));
        }
    }
}

// ─── Tumble + rebuild (per-frame) ────────────────────────────────────────────

/// Build the 3-edge triangle wireframe geometry (each edge = a thin quad,
/// uniformly colored `color` so the whole shard shares its HDR tint).
fn tri_wireframe(screen: &[Vec2; 3], color: [f32; 4]) -> (Vec<[f32; 3]>, Vec<[f32; 4]>, Vec<u32>) {
    let mut pos = Vec::with_capacity(TRI_EDGES.len() * 4);
    let mut col = Vec::with_capacity(TRI_EDGES.len() * 4);
    let mut idx = Vec::with_capacity(TRI_EDGES.len() * 6);
    for (k, &(a, b)) in TRI_EDGES.iter().enumerate() {
        let (pa, pb) = (screen[a], screen[b]);
        let dir = (pb - pa).normalize_or_zero();
        let perp = Vec2::new(-dir.y, dir.x) * EDGE_HALF_W;
        let base = (k * 4) as u32;
        for p in [pa + perp, pa - perp, pb - perp, pb + perp] {
            pos.push([p.x, p.y, 0.0]);
        }
        col.extend_from_slice(&[color, color, color, color]);
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (pos, col, idx)
}

/// Advance each shard's tumble, apply the source's drag, fade its HDR brightness
/// with remaining life, and rebuild its wireframe mesh. Attaches the `Mesh2d` on
/// first sight (lazy, so spawning needs no `Assets`). Presentation only — the
/// `Lifetime` despawn + `Velocity` integration run elsewhere.
pub fn tumble_shards(
    time: Res<Time>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mat: Res<AsteroidMaterial>,
    mut q: Query<(
        Entity,
        &mut AsteroidShard,
        &Lifetime,
        &mut Velocity,
        Option<&Mesh2d>,
    )>,
) {
    let dt = time.delta_secs();
    // Per-frame 0.985 drag at 60 Hz → frame-rate-independent exponential.
    let drag = 0.985_f32.powf(dt * 60.0);
    for (e, mut shard, life, mut vel, mesh2d) in &mut q {
        let delta = Quat::from_scaled_axis(shard.spin * dt);
        shard.rot = (delta * shard.rot).normalize();
        vel.0 *= drag;

        // Brightness rides remaining life so the shard blooms then dies to black.
        let frac = (life.seconds / shard.max_life).clamp(0.0, 1.0);
        let b = HDR_GAIN * frac;
        let c = [shard.color[0] * b, shard.color[1] * b, shard.color[2] * b, 1.0];

        // Rotate + perspective-project the 3 verts (denom clamp keeps an edge-on
        // shard from blowing up to infinity), matching `asteroid-shard.js`.
        let mut screen = [Vec2::ZERO; 3];
        for i in 0..3 {
            let p = shard.rot * BASE_VERTS[i];
            let denom = (1.0 + p.z * shard.size / FOCAL).max(0.25);
            screen[i] = Vec2::new(p.x, p.y) * (shard.size / denom);
        }
        let (pos, col, idx) = tri_wireframe(&screen, c);

        match mesh2d {
            Some(m) => {
                if let Some(mesh) = meshes.get_mut(&m.0) {
                    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
                    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
                }
            }
            None => {
                let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
                    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
                    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
                    .with_inserted_indices(Indices::U32(idx));
                commands
                    .entity(e)
                    .insert((Mesh2d(meshes.add(mesh)), MeshMaterial2d(mat.0.clone())));
            }
        }
    }
}
