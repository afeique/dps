//! Splitting asteroids: spawn, collision with player bullets, and out-of-bounds
//! culling. Tier 2 = large (~38 r), tier 1 = medium (~24 r), tier 0 = small
//! (~14 r). Bullets split a tier-2 into two tier-1s, a tier-1 into two tier-0s,
//! and destroy tier-0s outright. Bloom from HDR-emissive stroke gives a faint
//! warm-grey rim glow. Deterministic hashes drive spawn position, velocity
//! jitter, and vertex offsets — no `rand` dependency.

use crate::components::*;
use crate::resources::PlayBounds;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

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
    /// Base hue (degrees) + cycle rate (deg/sec).
    hue: f32,
    hue_speed: f32,
    sat: f32,
    light: f32,
}

/// Build the tumble state for a tier/seed: jittered + scaled vertices, random
/// 3-axis spin, and a per-asteroid hue (20 % gold, else a teal→violet band).
fn make_tumbler(tier: u8, seed: u32) -> Tumbler {
    let scale = shape_base_radius(tier) / CIRCUMRADIUS;
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
        hue_speed: hash_range(wang(seed ^ 0xD), 12.0, 36.0),
        sat: 0.9,
        light: 0.6,
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

/// An over-bright (HDR) emissive color from HSL so the wireframe blooms (neon).
fn hdr_hue(hue: f32, sat: f32, light: f32) -> Color {
    let l = Color::hsl(hue, sat, light).to_linear();
    const GAIN: f32 = 2.2;
    Color::linear_rgba(l.red * GAIN, l.green * GAIN, l.blue * GAIN, 1.0)
}

/// Build the wireframe `Shape` (30 edges as disconnected stroked segments).
fn wireframe_shape(screen: &[Vec2; 12], color: Color) -> Shape {
    let mut path = ShapePath::new();
    for &(a, b) in ICO_EDGES.iter() {
        path = path.move_to(screen[a]).line_to(screen[b]);
    }
    ShapeBuilder::with(&path).stroke((color, 2.0)).build()
}

/// Spawn one asteroid of `tier` at `pos` with `vel` and a deterministic tumble
/// seeded by `seed`. Shared by the wave spawner and the split path.
fn spawn_asteroid(commands: &mut Commands, tier: u8, pos: Vec2, vel: Vec2, seed: u32) {
    let tum = make_tumbler(tier, seed);
    let screen = project(&tum.verts, tum.rot);
    let color = hdr_hue(tum.hue, tum.sat, tum.light);
    commands.spawn((
        Asteroid { tier },
        wireframe_shape(&screen, color),
        tum,
        Collider { radius: collider_radius(tier) },
        Velocity(vel),
        Transform::from_xyz(pos.x, pos.y, 0.0),
    ));
}

/// Advance each asteroid's 3-axis tumble + hue cycle and rebuild its wireframe
/// `Shape` (lyon re-tessellates on `Changed<Shape>`). Presentation only — the
/// gameplay collider/velocity are untouched.
pub fn tumble_asteroids(time: Res<Time>, mut q: Query<(&mut Tumbler, &mut Shape)>) {
    let dt = time.delta_secs();
    for (mut tum, mut shape) in &mut q {
        let delta = Quat::from_scaled_axis(tum.spin * dt);
        tum.rot = (delta * tum.rot).normalize();
        tum.hue = (tum.hue + tum.hue_speed * dt).rem_euclid(360.0);
        let screen = project(&tum.verts, tum.rot);
        let color = hdr_hue(tum.hue, tum.sat, tum.light);
        *shape = wireframe_shape(&screen, color);
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
    bullets: Query<(Entity, &Transform, &Collider, &Bullet)>,
    asteroids: Query<(Entity, &Transform, &Collider, &Asteroid, &Velocity)>,
) {
    for (bullet_e, btf, bc, bullet) in &bullets {
        if bullet.kind != BulletKind::Player {
            continue;
        }
        for (ast_e, atf, ac, asteroid, parent_vel) in &asteroids {
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
