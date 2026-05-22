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
use std::f32::consts::TAU;

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

// ─── Shape builder ──────────────────────────────────────────────────────────

/// Build an irregular rocky polygon for the given tier.
///
/// 9 vertices with per-vertex radial jitter driven by a deterministic hash
/// keyed on `(tier, vertex_index)`. Fill is near-black rock; stroke is a dim
/// warm-grey HDR emissive so Bloom produces a faint rim glow.
pub fn shape(tier: u8) -> Shape {
    let base = shape_base_radius(tier);
    const VERTS: usize = 9;

    let mut path = ShapePath::new();
    for i in 0usize..VERTS {
        // Jitter: ±22 % of base radius, seeded by tier + vertex index.
        let seed = wang((tier as u32).wrapping_mul(97).wrapping_add(i as u32));
        let jitter = hash_range(seed, -0.22, 0.22);
        let r = base * (1.0 + jitter);

        let angle = (i as f32 / VERTS as f32) * TAU;
        let p = Vec2::new(angle.cos() * r, angle.sin() * r);

        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    let path = path.close();

    ShapeBuilder::with(&path)
        // Dark rocky fill — near-black warm brown.
        .fill(Color::linear_rgb(0.05, 0.04, 0.03))
        // Dim warm-grey emissive: values > 1.0 drive Bloom but stay subtle.
        .stroke((Color::linear_rgb(1.2, 1.0, 0.8), 1.5))
        .build()
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

    let tier: u8 = 2;
    commands.spawn((
        Asteroid { tier },
        shape(tier),
        Collider { radius: collider_radius(tier) },
        Velocity(vel),
        Transform::from_xyz(sx, sy, 0.0),
    ));
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

                    commands.spawn((
                        Asteroid { tier: child_tier },
                        shape(child_tier),
                        Collider { radius: collider_radius(child_tier) },
                        Velocity(vel),
                        Transform::from_xyz(pos.x, pos.y, 0.0),
                    ));
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
