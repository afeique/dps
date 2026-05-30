//! Splitting asteroids: spawn, collision with player bullets, and out-of-bounds
//! culling. Tier 2 = large (~38 r), tier 1 = medium (~24 r), tier 0 = small
//! (~14 r). Bullets split a tier-2 into two tier-1s, a tier-1 into two tier-0s,
//! and destroy tier-0s outright. Bloom from HDR-emissive stroke gives a faint
//! warm-grey rim glow. Deterministic hashes drive spawn position, velocity
//! jitter, and vertex offsets — no `rand` dependency.

use crate::components::*;
use crate::messages::AsteroidShatter;
use crate::resources::PlayBounds;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

// ─── collider radii by tier ─────────────────────────────────────────────────

fn collider_radius(tier: u8) -> f32 {
    match tier {
        2 => 36.0,
        1 => 22.0,
        _ => 13.0,
    }
}

fn shape_base_radius(tier: u8) -> f32 {
    match tier {
        2 => 38.0,
        1 => 24.0,
        _ => 14.0,
    }
}

// ─── deterministic hash helpers ─────────────────────────────────────────────

/// Tiny integer hash (Wang hash). Returns a value in 0..2^32.
#[inline]
fn wang(mut x: u32) -> u32 {
    x = (x ^ 61) ^ (x >> 16);
    x = x.wrapping_add(x << 3);
    x ^= x >> 4;
    x = x.wrapping_mul(0x27d4_eb2d);
    x ^= x >> 15;
    x
}

/// Map a hash output to `[lo, hi)`.
#[inline]
fn hash_range(seed: u32, lo: f32, hi: f32) -> f32 {
    let t = (wang(seed) as f32) / (u32::MAX as f32);
    lo + t * (hi - lo)
}

// ─── Component ──────────────────────────────────────────────────────────────

/// Marks an asteroid entity and records its split tier (2 = large, 1 = medium,
/// 0 = small / final).
#[derive(Component, Debug, Clone, Copy)]
pub struct Asteroid {
    pub tier: u8,
}

// ─── 3D tumbling icosahedron wireframe (spec VI.1) ───────────────────────────

/// Golden ratio — the icosahedron's defining constant.
const PHI: f32 = 1.618_034;
/// Circumradius of the canonical vertices below, `sqrt(1 + PHI²)`.
pub const CIRCUMRADIUS: f32 = 1.902_113;

/// The 12 icosahedron vertices (cyclic permutations of `(0, ±1, ±φ)`) in the
/// order that matches `ICO_EDGES` (spec VI.1).
pub const ICO_VERTS: [Vec3; 12] = [
    Vec3::new(-1.0, PHI, 0.0),  // 0
    Vec3::new(1.0, PHI, 0.0),   // 1
    Vec3::new(-1.0, -PHI, 0.0), // 2
    Vec3::new(1.0, -PHI, 0.0),  // 3
    Vec3::new(0.0, -1.0, PHI),  // 4
    Vec3::new(0.0, 1.0, PHI),   // 5
    Vec3::new(0.0, -1.0, -PHI), // 6
    Vec3::new(0.0, 1.0, -PHI),  // 7
    Vec3::new(PHI, 0.0, -1.0),  // 8
    Vec3::new(PHI, 0.0, 1.0),   // 9
    Vec3::new(-PHI, 0.0, -1.0), // 10
    Vec3::new(-PHI, 0.0, 1.0),  // 11
];

/// The 30 icosahedron edges (vertex-index pairs, spec VI.1).
pub const ICO_EDGES: [(usize, usize); 30] = [
    (0, 1), (0, 5), (0, 7), (0, 10), (0, 11),
    (1, 5), (1, 7), (1, 8), (1, 9),
    (2, 3), (2, 4), (2, 6), (2, 10), (2, 11),
    (3, 4), (3, 6), (3, 8), (3, 9),
    (4, 5), (4, 9), (4, 11),
    (5, 9), (5, 11),
    (6, 7), (6, 8), (6, 10),
    (7, 8), (7, 10),
    (8, 9),
    (10, 11),
];

/// Perspective focal length for the wireframe projection (spec VI.1 `fov 300`).
const FOV: f32 = 300.0;

/// Per-asteroid 3D-tumble state. The 12 vertices (jittered ±25 %, scaled to the
/// tier's pixel radius) tumble via `spin`; `tumble_asteroids` rotates + projects
/// them each frame and rebuilds the lyon wireframe `Shape` with a cycling hue.
#[derive(Component)]
pub struct Tumbler {
    verts: [Vec3; 12],
    rot: Quat,
    /// Angular velocity per axis (rad/sec).
    spin: Vec3,
    /// Moving base hue (degrees); advances at `hue_speed` (deg/sec).
    hue: f32,
    hue_speed: f32,
    /// Hue spread across the 12 vertices → the rainbow gradient (degrees).
    hue_spread: f32,
    sat: f32,
    light: f32,
    /// Circumradius (px) for the depth-fade normalisation.
    radius: f32,
    /// Phase offset (rad) for the per-asteroid brightness pulse.
    pulse_phase: f32,
}

/// Edge half-width (px) of the wireframe struts.
const EDGE_HALF_W: f32 = 1.6;
/// Brightness-pulse rate (rad/sec).
const PULSE_RATE: f32 = 2.5;
/// HDR gain on the wireframe color so Bloom turns it neon.
const HDR_GAIN: f32 = 2.6;
/// Extra brightness on the vertex glint nodes (over the struts) so the joints of
/// the wireframe read as bright facets — the rock glints like a cut gem.
const NODE_GAIN: f32 = 1.7;

/// Build the tumble state for a tier/seed: jittered + scaled vertices, random
/// 3-axis spin, a moving base hue (20 % gold, else a teal→violet band), a hue
/// spread for the rainbow gradient, and a pulse phase.
fn make_tumbler(tier: u8, seed: u32) -> Tumbler {
    let radius = shape_base_radius(tier);
    let scale = radius / CIRCUMRADIUS;
    let mut verts = [Vec3::ZERO; 12];
    for (i, v) in ICO_VERTS.iter().enumerate() {
        let j = hash_range(wang(seed ^ (i as u32).wrapping_mul(0x9E37)), -0.25, 0.25);
        verts[i] = *v * (scale * (1.0 + j));
    }
    let gold = hash_range(wang(seed ^ 0x60D), 0.0, 1.0) < 0.20;
    let hue = if gold {
        hash_range(wang(seed ^ 0x11), 40.0, 60.0)
    } else {
        hash_range(wang(seed ^ 0x22), 150.0, 280.0)
    };
    Tumbler {
        verts,
        rot: Quat::IDENTITY,
        spin: Vec3::new(
            hash_range(wang(seed ^ 0xA), -2.4, 2.4),
            hash_range(wang(seed ^ 0xB), -2.4, 2.4),
            hash_range(wang(seed ^ 0xC), -2.4, 2.4),
        ),
        hue,
        hue_speed: hash_range(wang(seed ^ 0xD), 14.0, 40.0),
        hue_spread: hash_range(wang(seed ^ 0x5E), 60.0, 180.0),
        sat: 0.95,
        light: 0.58,
        radius,
        pulse_phase: hash_range(wang(seed ^ 0x7F), 0.0, std::f32::consts::TAU),
    }
}

/// Rotate + perspective-project the 12 vertices to 2D (spec VI.1: `scale =
/// fov/(fov+z)`). Returns model-local screen offsets (the Transform positions it).
pub fn project(verts: &[Vec3; 12], rot: Quat) -> [Vec2; 12] {
    let mut out = [Vec2::ZERO; 12];
    for (i, v) in verts.iter().enumerate() {
        let p = rot * *v;
        let s = FOV / (FOV + p.z);
        out[i] = Vec2::new(p.x * s, p.y * s);
    }
    out
}

/// Per-vertex HDR colors (spec VI.1): a rainbow hue gradient across the vertices
/// (edges interpolate between endpoints), a brightness pulse, and a depth fade
/// (near vertices brighter). `t` is the elapsed time for the pulse.
fn vertex_colors(tum: &Tumbler, depth: &[f32; 12], t: f32) -> [[f32; 4]; 12] {
    let pulse = 0.78 + 0.22 * (t * PULSE_RATE + tum.pulse_phase).sin();
    let zmax = tum.radius.max(1.0);
    let mut out = [[0.0; 4]; 12];
    for i in 0..12 {
        let hue = (tum.hue + (i as f32 / 12.0) * tum.hue_spread).rem_euclid(360.0);
        // Near (z<0 under `fov/(fov+z)`) is brighter; far (z>0) dimmer.
        let fade = (0.4 + 0.6 * ((zmax - depth[i]) / (2.0 * zmax))).clamp(0.4, 1.0);
        let b = pulse * fade * HDR_GAIN;
        let l = Color::hsl(hue, tum.sat, tum.light).to_linear();
        out[i] = [l.red * b, l.green * b, l.blue * b, 1.0];
    }
    out
}

/// Build the 30-edge wireframe geometry (each edge = a thin quad, endpoints
/// colored by their vertex so the GPU interpolates the rainbow along each strut).
pub fn wireframe_geometry(
    screen: &[Vec2; 12],
    colors: &[[f32; 4]; 12],
) -> (Vec<[f32; 3]>, Vec<[f32; 4]>, Vec<u32>) {
    let mut pos = Vec::with_capacity(ICO_EDGES.len() * 4);
    let mut col = Vec::with_capacity(ICO_EDGES.len() * 4);
    let mut idx = Vec::with_capacity(ICO_EDGES.len() * 6);
    for (k, &(a, b)) in ICO_EDGES.iter().enumerate() {
        let (pa, pb) = (screen[a], screen[b]);
        let dir = (pb - pa).normalize_or_zero();
        let perp = Vec2::new(-dir.y, dir.x) * EDGE_HALF_W;
        let base = (k * 4) as u32;
        for p in [pa + perp, pa - perp, pb - perp, pb + perp] {
            pos.push([p.x, p.y, 0.0]);
        }
        col.extend_from_slice(&[colors[a], colors[a], colors[b], colors[b]]);
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (pos, col, idx)
}

/// Append a bright glint node (a small `half`-sized quad) at each of the 12
/// vertices, brighter than the struts (`NODE_GAIN`) and inheriting the vertex's
/// pulsing depth-faded hue — so the icosahedron's joints sparkle like a cut gem's
/// facets. Appended *after* the edge quads, so the static index buffer built once
/// in `tumble_asteroids` stays valid (vertex count is constant: 30 edges + 12
/// nodes every frame).
pub fn push_vertex_nodes(
    pos: &mut Vec<[f32; 3]>,
    col: &mut Vec<[f32; 4]>,
    idx: &mut Vec<u32>,
    screen: &[Vec2; 12],
    colors: &[[f32; 4]; 12],
    half: f32,
) {
    for i in 0..12 {
        let c = screen[i];
        let base = pos.len() as u32;
        for off in [
            Vec2::new(-half, -half),
            Vec2::new(half, -half),
            Vec2::new(half, half),
            Vec2::new(-half, half),
        ] {
            let p = c + off;
            pos.push([p.x, p.y, 0.0]);
        }
        let vc = colors[i];
        let node = [vc[0] * NODE_GAIN, vc[1] * NODE_GAIN, vc[2] * NODE_GAIN, 1.0];
        col.extend_from_slice(&[node, node, node, node]);
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// Spawn one asteroid of `tier` at `pos` with `vel` and a deterministic tumble
/// seeded by `seed`. Shared by the wave spawner and the split path. The
/// renderable `Mesh2d` is attached lazily by `tumble_asteroids` (needs `Assets`),
/// so this stays free of asset plumbing in the wave/collision systems.
fn spawn_asteroid(commands: &mut Commands, tier: u8, pos: Vec2, vel: Vec2, seed: u32) {
    commands.spawn((
        Asteroid { tier },
        make_tumbler(tier, seed),
        Collider { radius: collider_radius(tier) },
        Velocity(vel),
        Transform::from_xyz(pos.x, pos.y, 0.0),
    ));
}

/// Shared white material for the vertex-colored asteroid wireframes (the per-
/// vertex HDR colors carry the rainbow; the material just passes them through).
#[derive(Resource)]
pub struct AsteroidMaterial(pub Handle<ColorMaterial>);

/// Create the shared wireframe material (Startup).
pub fn setup_asteroid_material(
    mut commands: Commands,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let h = materials.add(ColorMaterial::from(Color::WHITE));
    commands.insert_resource(AsteroidMaterial(h));
}

/// Advance each asteroid's 3-axis tumble + moving hue and rebuild its vertex-
/// colored wireframe mesh (pulsing, depth-faded rainbow, spec VI.1). Attaches the
/// `Mesh2d` on first sight (lazy, so spawning needs no `Assets`). Presentation
/// only — the gameplay collider/velocity are untouched.
pub fn tumble_asteroids(
    time: Res<Time>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mat: Res<AsteroidMaterial>,
    mut q: Query<(Entity, &mut Tumbler, Option<&Mesh2d>)>,
) {
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    for (e, mut tum, mesh2d) in &mut q {
        let delta = Quat::from_scaled_axis(tum.spin * dt);
        tum.rot = (delta * tum.rot).normalize();
        tum.hue = (tum.hue + tum.hue_speed * dt).rem_euclid(360.0);

        let mut screen = [Vec2::ZERO; 12];
        let mut depth = [0.0f32; 12];
        for i in 0..12 {
            let p = tum.rot * tum.verts[i];
            let s = FOV / (FOV + p.z);
            screen[i] = Vec2::new(p.x * s, p.y * s);
            depth[i] = p.z;
        }
        let colors = vertex_colors(&tum, &depth, t);
        let (mut pos, mut col, mut idx) = wireframe_geometry(&screen, &colors);
        // Bright glint nodes at the 12 vertices (sized to the rock) so the joints
        // sparkle. Constant vertex count keeps the lazily-built index buffer valid.
        let node_half = (tum.radius * 0.06).clamp(1.6, 3.2);
        push_vertex_nodes(&mut pos, &mut col, &mut idx, &screen, &colors, node_half);

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

// ─── Spawn ───────────────────────────────────────────────────────────────────

/// Spawn one tier-2 asteroid from a `seed`-chosen screen edge with an
/// inward-drifting velocity. Deterministic in `seed` so the sequence is
/// reproducible. Called per-wave by `wave::spawn_pulse` (the wave's
/// `WaveDef.asteroids` count, spec V) — there is no longer a standalone
/// periodic spawner.
pub fn spawn_one_asteroid(commands: &mut Commands, bounds: &PlayBounds, seed: u32) {
    // Pick which edge: 0=top, 1=bottom, 2=left, 3=right.
    let edge = wang(seed) % 4;
    let hx = bounds.half.x;
    let hy = bounds.half.y;
    let margin = 60.0; // spawn slightly outside the play area

    let (sx, sy) = match edge {
        0 => (hash_range(wang(seed ^ 0xA1), -hx, hx), hy + margin),  // top
        1 => (hash_range(wang(seed ^ 0xB2), -hx, hx), -hy - margin), // bottom
        2 => (-hx - margin, hash_range(wang(seed ^ 0xC3), -hy, hy)), // left
        _ => (hx + margin, hash_range(wang(seed ^ 0xD4), -hy, hy)),  // right
    };

    // Aim roughly toward the centre with a lateral spread.
    let to_center = Vec2::new(-sx, -sy).normalize_or_zero();
    let speed = hash_range(wang(seed ^ 0xE5), 40.0, 90.0);
    let lateral = hash_range(wang(seed ^ 0xF6), -25.0, 25.0);
    let perp = Vec2::new(-to_center.y, to_center.x);
    let vel = to_center * speed + perp * lateral;

    spawn_asteroid(commands, 2, Vec2::new(sx, sy), vel, seed);
}

// ─── Collision / split system ────────────────────────────────────────────────

/// Check every player bullet against every asteroid. On overlap:
/// - Despawn the bullet.
/// - If `tier > 0`, replace the asteroid with two children of `tier - 1`
///   whose velocities are deflected ±35° from the parent's and slightly faster.
/// - If `tier == 0`, destroy the asteroid outright.
pub fn asteroid_hits(
    mut commands: Commands,
    mut shatter: MessageWriter<AsteroidShatter>,
    bullets: Query<(Entity, &Transform, &Collider, &Bullet)>,
    asteroids: Query<(
        Entity,
        &Transform,
        &Collider,
        &Asteroid,
        &Velocity,
        Option<&Tumbler>,
    )>,
) {
    for (bullet_e, btf, bc, bullet) in &bullets {
        if bullet.kind != BulletKind::Player {
            continue;
        }
        for (ast_e, atf, ac, asteroid, parent_vel, tumbler) in &asteroids {
            let reach = bc.radius + ac.radius;
            let d2 = btf
                .translation
                .truncate()
                .distance_squared(atf.translation.truncate());
            if d2 > reach * reach {
                continue;
            }

            // Hit confirmed — consume the bullet.
            commands.entity(bullet_e).despawn();

            // Split or destroy.
            let pos = atf.translation.truncate();

            // Seed the death-burst VFX (render::asteroid_debris) — every hit
            // shatters the parent into a fan of hue-matched wireframe shards +
            // a ring, fired whether it splits into children or is destroyed
            // outright. Use the rock's live HSL so the debris is its own color;
            // fall back to a warm grey for tumbler-less (test) asteroids.
            let (hue, sat, light, radius, verts) = match tumbler {
                Some(t) => (t.hue, t.sat, t.light, t.radius, project(&t.verts, t.rot)),
                None => (40.0, 0.2, 0.6, shape_base_radius(asteroid.tier), [Vec2::ZERO; 12]),
            };
            shatter.write(AsteroidShatter { center: pos, hue, sat, light, radius, verts });

            commands.entity(ast_e).despawn();

            if asteroid.tier > 0 {
                let child_tier = asteroid.tier - 1;
                let parent_dir = if parent_vel.0.length_squared() > 0.0 {
                    parent_vel.0.normalize()
                } else {
                    Vec2::Y
                };
                let child_speed = parent_vel.0.length() * 1.3 + 20.0;

                for (i, angle_offset) in [35.0_f32, -35.0_f32].iter().enumerate() {
                    let rot = Mat2::from_angle(angle_offset.to_radians());
                    let dir = rot * parent_dir;
                    // Tiny additional jitter per child so they don't mirror exactly.
                    let jitter_seed = wang((ast_e.to_bits() as u32).wrapping_add(i as u32 * 13));
                    let jitter = hash_range(jitter_seed, -8.0, 8.0);
                    let vel = dir * (child_speed + jitter);

                    spawn_asteroid(&mut commands, child_tier, pos, vel, jitter_seed);
                }
            }
            // tier 0: just despawned above, no children.

            break; // bullet is gone; stop checking more asteroids
        }
    }
}

// ─── Cull system ─────────────────────────────────────────────────────────────

/// Despawn asteroids that drift well outside the play bounds so entity count
/// stays bounded. Uses a generous margin (250 u) matching the enemy cull.
pub fn cull_asteroids(
    mut commands: Commands,
    bounds: Res<PlayBounds>,
    q: Query<(Entity, &Transform), With<Asteroid>>,
) {
    let margin = 250.0;
    for (e, tf) in &q {
        if tf.translation.x.abs() > bounds.half.x + margin
            || tf.translation.y.abs() > bounds.half.y + margin
        {
            commands.entity(e).despawn();
        }
    }
}
