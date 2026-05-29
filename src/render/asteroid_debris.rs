//! Wireframe-shard death bursts — the port of rainboids' `createDebris`
//! (`combat-manager.js` §7b) + `asteroid-shard.js`. The signature primitive is a
//! radial fan of **3D-tumbling wireframe triangles** ([`burst_shards`]), reused
//! by two events:
//!   * **Asteroid shatter** (`spawn_asteroid_debris` ← `AsteroidShatter`): shards
//!     in the rock's own live HSL + one expanding colored ring, so the burst
//!     reads as that specific rock breaking apart.
//!   * **Enemy death** (`spawn_enemy_shrapnel` ← `Death`): a modest fan tinted to
//!     the kill's damage element, completing the kill burst alongside the fire
//!     core (`render::explosion`) + wavefront rings (`reaction_fx`).
//! Stroke-only triangles (no fill) match the wireframe-asteroid visual language
//! rather than a generic puff.
//!
//! Each shard is an equilateral triangle in 3D unit space, spun on all three
//! axes (`spin`) and perspective-projected each frame; `tumble_shards` rebuilds
//! its 3-edge wireframe mesh, fades its HDR brightness with remaining life (so
//! Bloom flares it bright then lets it die to black against the starfield), and
//! applies the source's mild drag so the fan drifts to rest as it fades. Motion
//! is the shared `Velocity`/`integrate` path; despawn is the shared `Lifetime`/
//! `tick_lifetimes` path — no bespoke lifecycle. Deterministic (Wang-hashed off a
//! `Local` counter), so it needs no `rand` dependency.

use crate::components::{Bleed, Burning, Collider, Corrode, Frozen, Lifetime, Mark, Velocity};
use crate::messages::{AsteroidShatter, Death};
use crate::render::reaction_fx::{Shockwave, unit_ring};
use crate::systems::asteroids::{AsteroidMaterial, ICO_EDGES};
use crate::systems::enemy::element_for;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;
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
/// Half-width (px) of a line-debris strut quad.
const LINE_HALF_W: f32 = 1.1;
/// HDR gain on the line-debris color — a touch under the shards since the struts
/// are numerous; keeps them from washing out the bloom.
const LINE_HDR_GAIN: f32 = 2.2;
/// HDR gain on lingering embers (soft glow motes, Bloom flares them).
const EMBER_GAIN: f32 = 2.0;

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
        burst_shards(
            &mut commands,
            &mut seed,
            ev.center,
            0.16,
            count,
            size_scale.max(0.6),
            base,
            bright,
            dim,
        );

        // Line debris (§7): the rock's broken wireframe struts detach and drift
        // apart — one segment per icosahedron edge, in the rock's base color,
        // flying outward from its midpoint, spinning, and fading fast (~0.8s).
        // Built from the verts captured at the break, so the cage that flies
        // apart is the exact one the player was looking at.
        for &(a, b) in ICO_EDGES.iter() {
            *seed = seed.wrapping_add(1);
            let s = *seed;
            let (p1, p2) = (ev.verts[a], ev.verts[b]);
            let mid = (p1 + p2) * 0.5;
            let dir = if mid.length_squared() < 0.01 {
                let ang = frand(s ^ 0xB1, 0.0, TAU);
                Vec2::new(ang.cos(), ang.sin())
            } else {
                mid.normalize()
            };
            let life = frand(s ^ 0xB4, 0.6, 0.95);
            commands.spawn((
                LineDebris {
                    p1,
                    p2,
                    color: base,
                    max_life: life,
                    spin: frand(s ^ 0xB3, -3.0, 3.0),
                },
                Transform::from_translation(ev.center.extend(0.17)),
                Velocity(dir * frand(s ^ 0xB2, 80.0, 200.0)),
                Lifetime { seconds: life },
            ));
        }

        // Lingering embers (§4/§5): soft glow motes that drift slowly out and
        // linger ~1.3–2.0 s as the rock's afterglow (asteroids get no fiery
        // hanabi burst, so this is their "cooling" trail). The first is a white-
        // hot core; the rest take the rock's hue ±30°, brightened.
        let ember_count = (6.0 + 5.0 * size_scale) as u32;
        for i in 0..ember_count {
            *seed = seed.wrapping_add(1);
            let s = *seed;
            let ang = frand(s ^ 0xE1, 0.0, TAU);
            let drift = frand(s ^ 0xE2, 12.0, 55.0); // slow, unlike the shards
            let life = frand(s ^ 0xE3, 1.3, 2.0);
            let r = frand(s ^ 0xE4, 2.0, 4.5);
            let color = if i == 0 {
                Color::linear_rgb(EMBER_GAIN, EMBER_GAIN, EMBER_GAIN) // white-hot core
            } else {
                let eh = ev.hue + frand(s ^ 0xE5, -30.0, 30.0);
                let el = frand(s ^ 0xE6, 0.55, 0.80);
                let l = Color::hsl(eh.rem_euclid(360.0), ev.sat, el).to_linear();
                Color::linear_rgb(l.red * EMBER_GAIN, l.green * EMBER_GAIN, l.blue * EMBER_GAIN)
            };
            let dir = Vec2::new(ang.cos(), ang.sin());
            commands.spawn((
                Ember { max_life: life },
                ember_disc(color, r),
                Transform::from_translation(ev.center.extend(0.14)),
                Velocity(dir * drift),
                Lifetime { seconds: life },
            ));
        }
    }
}

/// Spawn a radial fan of `count` tumbling wireframe-triangle shards from
/// `center` at depth `z`, cycling the `base`/`bright`/`dim` palette (every 5th a
/// white spark) with jittered angles, fly-out speed (`110..280 × speed_scale`),
/// size, spin, and lifetime. Shared by the asteroid shatter and the enemy-death
/// shrapnel — the `AsteroidShard`/`tumble_shards` primitive is source-agnostic.
/// Advances `seed` per shard so repeated bursts don't stamp identical fans.
#[allow(clippy::too_many_arguments)]
fn burst_shards(
    commands: &mut Commands,
    seed: &mut u32,
    center: Vec2,
    z: f32,
    count: u32,
    speed_scale: f32,
    base: [f32; 3],
    bright: [f32; 3],
    dim: [f32; 3],
) {
    for i in 0..count {
        *seed = seed.wrapping_add(1);
        let s = *seed;
        // Evenly-spaced angles + jitter so the burst reads as an organic
        // shatter, not a fixed pinwheel.
        let angle = (i as f32 / count.max(1) as f32) * TAU + frand(s ^ 0x1, -0.35, 0.35);
        let speed = frand(s ^ 0x2, 110.0, 280.0) * speed_scale;
        let size = frand(s ^ 0x3, 4.0, 9.0);
        let max_life = frand(s ^ 0x4, 1.1, 1.7);
        // Most shards take the source color; every 5th is a white spark pop.
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
            Transform::from_translation(center.extend(z)),
            Velocity(dir * speed),
            Lifetime { seconds: max_life },
        ));
    }
}

/// Element-tinted shrapnel shards on each enemy death — the directional shard
/// layer of `createDebris` (§7b), reusing [`burst_shards`]. Completes the
/// enemy-kill burst alongside the white-hot fire core (`render::explosion`) and
/// the element wavefront rings (`reaction_fx::spawn_death_rings`): a modest fan
/// of wireframe triangles tinted to the kill's damage element, scaling with boss
/// tier / mini-boss. Player death (`kind == None`) is skipped — its FX is the
/// explosion + screen shake + flash, not a hue-coded shatter.
pub fn spawn_enemy_shrapnel(
    mut commands: Commands,
    mut deaths: MessageReader<Death>,
    mut seed: Local<u32>,
) {
    for d in deaths.read() {
        let Some(kind) = d.kind else { continue }; // skip the player's own death
        let c = element_for(kind).color().to_linear();
        let base = [c.red, c.green, c.blue];
        // Bright = pushed toward white for the highlight shards; dim = darkened.
        let bright = [
            (c.red * 1.4 + 0.2).min(1.0),
            (c.green * 1.4 + 0.2).min(1.0),
            (c.blue * 1.4 + 0.2).min(1.0),
        ];
        let dim = [c.red * 0.55, c.green * 0.55, c.blue * 0.55];
        // Modest fan (enemies die far more often than rocks) + a boss bonus.
        let count = 6 + d.boss_tier as u32 * 4 + if d.mini_boss { 3 } else { 0 };
        burst_shards(&mut commands, &mut seed, d.position, 0.16, count, 0.7, base, bright, dim);
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

// ─── Line debris — the rock's detached wireframe struts (createDebris §7) ─────

/// One detached wireframe strut. The segment (`p1`→`p2`, local offsets from the
/// entity origin) is baked into the mesh; the entity spins via its Transform,
/// drifts via `Velocity`, and fades its HDR brightness with remaining life.
#[derive(Component)]
pub struct LineDebris {
    p1: Vec2,
    p2: Vec2,
    color: [f32; 3],
    max_life: f32,
    /// In-plane spin (rad/sec).
    spin: f32,
}

/// Build a single edge as a thin quad in `color` (4 verts, 2 tris).
fn edge_quad(p1: Vec2, p2: Vec2, color: [f32; 4]) -> (Vec<[f32; 3]>, Vec<[f32; 4]>, Vec<u32>) {
    let dir = (p2 - p1).normalize_or_zero();
    let perp = Vec2::new(-dir.y, dir.x) * LINE_HALF_W;
    let pts = [p1 + perp, p1 - perp, p2 - perp, p2 + perp];
    let pos = pts.iter().map(|p| [p.x, p.y, 0.0]).collect();
    let col = vec![color; 4];
    let idx = vec![0, 1, 2, 0, 2, 3];
    (pos, col, idx)
}

/// Spin + drift + fade each strut. The segment geometry is static, so after the
/// lazy first-frame attach this only refreshes the COLOR attribute (brightness
/// rides remaining life). Despawn is the shared `Lifetime`/`tick_lifetimes` path.
pub fn tick_line_debris(
    time: Res<Time>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mat: Res<AsteroidMaterial>,
    mut q: Query<(
        Entity,
        &LineDebris,
        &Lifetime,
        &mut Velocity,
        &mut Transform,
        Option<&Mesh2d>,
    )>,
) {
    let dt = time.delta_secs();
    // Gentle drag so struts ease to a drift as they fade.
    let drag = 0.97_f32.powf(dt * 60.0);
    for (e, ld, life, mut vel, mut tf, mesh2d) in &mut q {
        tf.rotation *= Quat::from_rotation_z(ld.spin * dt);
        vel.0 *= drag;
        let frac = (life.seconds / ld.max_life).clamp(0.0, 1.0);
        let b = LINE_HDR_GAIN * frac;
        let c = [ld.color[0] * b, ld.color[1] * b, ld.color[2] * b, 1.0];
        match mesh2d {
            Some(m) => {
                if let Some(mesh) = meshes.get_mut(&m.0) {
                    // Positions are static — only the fading color changes.
                    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; 4]);
                }
            }
            None => {
                let (pos, col, idx) = edge_quad(ld.p1, ld.p2, c);
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

// ─── Lingering embers — afterglow motes (createDebris §4/§5) ──────────────────

/// One soft glow mote. Drifts via `Velocity`, fades via `fade_embers`, despawns
/// via the shared `Lifetime` path. `max_life` scales the fade.
#[derive(Component)]
pub struct Ember {
    max_life: f32,
}

/// A small filled 12-gon disc in `color` (HDR so Bloom turns it into a soft glow).
fn ember_disc(color: Color, r: f32) -> Shape {
    let mut path = ShapePath::new();
    for i in 0..12 {
        let a = i as f32 / 12.0 * TAU;
        let p = Vec2::new(a.cos() * r, a.sin() * r);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    ShapeBuilder::with(&path.close()).fill(color).build()
}

/// Shrink each ember toward nothing as its life runs out. A `sqrt` curve keeps it
/// near full size for most of its life then drops fast at the end, so embers read
/// as lingering then winking out rather than steadily shrinking.
pub fn fade_embers(mut q: Query<(&Ember, &Lifetime, &mut Transform)>) {
    for (em, life, mut tf) in &mut q {
        let frac = (life.seconds / em.max_life).clamp(0.0, 1.0);
        tf.scale = Vec3::splat(frac.sqrt());
    }
}

/// Emit rising fire embers off burning enemies — the `Burning` status had only a
/// ring aura; now each afflicted enemy trails hot orange motes that drift up and
/// fade, so fire damage reads as actual flame. Throttled (a batch every
/// `BURN_EMBER_INTERVAL`, capped per batch) so a screen of burning foes can't
/// flood the field. Reuses the [`Ember`] primitive ([`fade_embers`] + the shared
/// Velocity/Lifetime paths). Pure presentation.
pub fn emit_burn_embers(
    time: Res<Time>,
    mut commands: Commands,
    mut acc: Local<f32>,
    mut seed: Local<u32>,
    burning: Query<&Transform, With<Burning>>,
) {
    const BURN_EMBER_INTERVAL: f32 = 0.07;
    const MAX_PER_BATCH: usize = 40;
    *acc += time.delta_secs();
    if *acc < BURN_EMBER_INTERVAL {
        return;
    }
    *acc = 0.0;
    for (n, tf) in burning.iter().enumerate() {
        if n >= MAX_PER_BATCH {
            break;
        }
        *seed = seed.wrapping_add(1);
        let s = *seed;
        let pos = tf.translation.truncate()
            + Vec2::new(frand(s ^ 0x1, -8.0, 8.0), frand(s ^ 0x2, -6.0, 6.0));
        let life = frand(s ^ 0x3, 0.4, 0.8);
        let r = frand(s ^ 0x4, 1.5, 3.0);
        // Hot pyro orange (HDR → bloom); green channel jitters for flame variety.
        let color = Color::linear_rgb(9.0, frand(s ^ 0x5, 2.0, 4.0), 0.3);
        commands.spawn((
            Ember { max_life: life },
            ember_disc(color, r),
            Transform::from_translation(pos.extend(0.13)),
            // Rise + a little lateral drift.
            Velocity(Vec2::new(frand(s ^ 0x6, -15.0, 15.0), frand(s ^ 0x7, 30.0, 70.0))),
            Lifetime { seconds: life },
        ));
    }
}

/// Glinting ice sparkles on frozen enemies — `Frozen` had only a ring aura; now
/// each iced foe twinkles with tiny stationary cyan-white motes popping at random
/// points within its body, so a freeze reads as "encased in shimmering ice".
/// Throttled batch (every `GLINT_INTERVAL`, capped); reuses the [`Ember`]
/// primitive (Bloom turns each bright dot into a soft star). Pure presentation.
pub fn emit_frozen_glints(
    time: Res<Time>,
    mut commands: Commands,
    mut acc: Local<f32>,
    mut seed: Local<u32>,
    frozen: Query<(&Transform, &Collider), With<Frozen>>,
) {
    const GLINT_INTERVAL: f32 = 0.11;
    const MAX_PER_BATCH: usize = 30;
    *acc += time.delta_secs();
    if *acc < GLINT_INTERVAL {
        return;
    }
    *acc = 0.0;
    for (n, (tf, col)) in frozen.iter().enumerate() {
        if n >= MAX_PER_BATCH {
            break;
        }
        *seed = seed.wrapping_add(1);
        let s = *seed;
        // A random point inside the enemy's body so the shimmer covers it.
        let ang = frand(s ^ 0x1, 0.0, TAU);
        let rad = frand(s ^ 0x2, 0.0, col.radius * 0.9);
        let pos = tf.translation.truncate() + Vec2::new(ang.cos(), ang.sin()) * rad;
        let life = frand(s ^ 0x3, 0.25, 0.5);
        let r = frand(s ^ 0x4, 1.0, 2.2);
        commands.spawn((
            Ember { max_life: life },
            ember_disc(Color::linear_rgb(6.0, 8.5, 9.0), r), // icy white-cyan
            Transform::from_translation(pos.extend(0.14)),
            Velocity(Vec2::ZERO), // a stationary glint, not a drifting ember
            Lifetime { seconds: life },
        ));
    }
}

/// Trailing motes for the remaining damage-over-time / debuff statuses, tinted +
/// moving per status so each reads at a glance: CORRODE drips acid-green
/// *downward*, BLEED sheds red flecks that ooze down slowly, MARK exhales violet
/// void wisps that *rise*. Throttled batch (shared accumulator, capped across all
/// three) reusing the [`Ember`] primitive. Completes the per-status particle set
/// (conduct arcs / burn embers / frozen glints already cover the rest).
pub fn emit_status_motes(
    time: Res<Time>,
    mut commands: Commands,
    mut acc: Local<f32>,
    mut seed: Local<u32>,
    corrode: Query<&Transform, With<Corrode>>,
    bleed: Query<&Transform, With<Bleed>>,
    mark: Query<&Transform, With<Mark>>,
) {
    const MOTE_INTERVAL: f32 = 0.12;
    const MAX_PER_BATCH: usize = 30;
    *acc += time.delta_secs();
    if *acc < MOTE_INTERVAL {
        return;
    }
    *acc = 0.0;

    // `fall` is the base vertical velocity (negative = drips down, positive =
    // rises); horizontal + jitter are rolled per mote. All randomness lives here
    // so the per-status loops below never touch `seed` (keeps the borrow simple).
    let mut emit = |base: Vec2, color: Color, fall: f32| {
        *seed = seed.wrapping_add(1);
        let s = *seed;
        let pos = base + Vec2::new(frand(s ^ 0x1, -7.0, 7.0), frand(s ^ 0x2, -7.0, 7.0));
        let vel = Vec2::new(frand(s ^ 0x5, -10.0, 10.0), fall + frand(s ^ 0x6, -8.0, 8.0));
        let life = frand(s ^ 0x3, 0.4, 0.9);
        let r = frand(s ^ 0x4, 1.2, 2.6);
        commands.spawn((
            Ember { max_life: life },
            ember_disc(color, r),
            Transform::from_translation(pos.extend(0.12)),
            Velocity(vel),
            Lifetime { seconds: life },
        ));
    };

    let mut n = 0usize;
    for tf in &corrode {
        if n >= MAX_PER_BATCH {
            break;
        }
        emit(tf.translation.truncate(), Color::linear_rgb(2.0, 9.0, 1.0), -40.0); // acid drips
        n += 1;
    }
    for tf in &bleed {
        if n >= MAX_PER_BATCH {
            break;
        }
        emit(tf.translation.truncate(), Color::linear_rgb(9.0, 0.8, 1.2), -12.0); // blood oozes
        n += 1;
    }
    for tf in &mark {
        if n >= MAX_PER_BATCH {
            break;
        }
        emit(tf.translation.truncate(), Color::linear_rgb(6.0, 1.0, 9.0), 12.0); // void wisps rise
        n += 1;
    }
}
