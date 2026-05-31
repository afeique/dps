//! Wasp enemy — a true **boid swarm**: each wasp flocks (separation + cohesion +
//! alignment) with the other wasps while weakly seeking the Core, producing the
//! living, erratic shoal that a lone-darting wasp never quite sold. Fast, twitchy,
//! and hard to predict because the swarm shifts as a body. Ported from rainboids'
//! swarmer archetype (`separationWeight` flocking), retargeted onto the Core.
//!
//! Two-pass: snapshot every swarmer's `(pos, vel)` (immutable) so the boid math
//! sees a consistent frame, then steer each one (mutable). A per-wasp wander seed
//! (the iteration index) keeps the shoal shimmering instead of moving as one blob.

use crate::components::{Core, Enemy, EnemyKind, SpeedMul, Velocity};
use crate::systems::enemy::EnemyStats;
use crate::systems::steering::{alignment, approach, cohesion, separation, wander};
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

// ── Stats ─────────────────────────────────────────────────────────────────────

/// JS WASP: health 5, size 36 (radius 18), speed 3.5 px/frame @60 fps.
pub fn stats() -> EnemyStats {
    EnemyStats {
        health: 5.0,
        radius: 18.0,
        speed: 210.0,
        fire_cooldown: Some(0.7),
    }
}

// ── Shape ─────────────────────────────────────────────────────────────────────

/// Wasp silhouette: swept razor wings + thorax + head + stinger. Authored in
/// Canvas2D (+Y down, facing +X), flipped to Bevy (+Y up) by negating Y.
pub fn shape() -> Shape {
    let s = stats().radius;

    let wu = [
        Vec2::new(s * 0.15, 0.0),
        Vec2::new(s * 0.80, s * 0.22),
        Vec2::new(-s * 0.05, s * 1.05),
        Vec2::new(-s * 0.65, s * 0.90),
        Vec2::new(-s * 0.75, s * 0.28),
    ];
    let wl = [
        Vec2::new(s * 0.15, 0.0),
        Vec2::new(s * 0.80, -s * 0.22),
        Vec2::new(-s * 0.05, -s * 1.05),
        Vec2::new(-s * 0.65, -s * 0.90),
        Vec2::new(-s * 0.75, -s * 0.28),
    ];
    let thorax = [
        Vec2::new(s * 0.37, 0.0),
        Vec2::new(-s * 0.05, s * 0.30),
        Vec2::new(-s * 0.47, 0.0),
        Vec2::new(-s * 0.05, -s * 0.30),
    ];
    let head = [
        Vec2::new(s * 0.68, 0.0),
        Vec2::new(s * 0.38, s * 0.22),
        Vec2::new(s * 0.08, 0.0),
        Vec2::new(s * 0.38, -s * 0.22),
    ];
    let stinger = [
        Vec2::new(s * 0.75, 0.0),
        Vec2::new(s * 0.58, s * 0.10),
        Vec2::new(s * 0.58, -s * 0.10),
    ];

    let path = ShapePath::new()
        .move_to(wu[0]).line_to(wu[1]).line_to(wu[2]).line_to(wu[3]).line_to(wu[4]).close()
        .move_to(wl[0]).line_to(wl[1]).line_to(wl[2]).line_to(wl[3]).line_to(wl[4]).close()
        .move_to(thorax[0]).line_to(thorax[1]).line_to(thorax[2]).line_to(thorax[3]).close()
        .move_to(head[0]).line_to(head[1]).line_to(head[2]).line_to(head[3]).close()
        .move_to(stinger[0]).line_to(stinger[1]).line_to(stinger[2]).close();

    ShapeBuilder::with(&path)
        .fill(Color::linear_rgb(0.04, 0.04, 0.0))
        .stroke((Color::linear_rgb(8.0, 7.0, 1.0), 1.5))
        .build()
}

// ── AI / boid flock ─────────────────────────────────────────────────────────

const ACCEL_FRAC: f32 = 0.05;
const NEIGHBOUR_RADIUS: f32 = 120.0;
const SEPARATION_RADIUS: f32 = 52.0;
// Boid weights: spread strongest, then a weak pull together + heading-match, and
// a gentle Core-seek so the whole shoal still drifts in to attack.
const W_SEPARATION: f32 = 1.6;
const W_COHESION: f32 = 0.5;
const W_ALIGNMENT: f32 = 0.35;
const W_SEEK: f32 = 0.55;
const W_WANDER: f32 = 0.4;

fn is_swarmer(kind: EnemyKind) -> bool {
    matches!(kind, EnemyKind::Wasp | EnemyKind::Cinder | EnemyKind::LumenDrone)
}

pub fn ai(
    time: Res<Time>,
    core: Query<&Transform, (With<Core>, Without<Enemy>)>,
    mut enemies: Query<(&Transform, &mut Velocity, &Enemy, Option<&SpeedMul>), With<Enemy>>,
) {
    let Ok(core_tf) = core.single() else {
        return;
    };
    let core_pos = core_tf.translation.truncate();
    let t = time.elapsed_secs();
    let base = stats().speed;

    // Pass 1 — snapshot the swarm so flocking sees a consistent frame.
    let flock: Vec<(Vec2, Vec2)> = enemies
        .iter()
        .filter(|(_, _, e, _)| is_swarmer(e.kind))
        .map(|(tf, vel, _, _)| (tf.translation.truncate(), vel.0))
        .collect();

    // Pass 2 — steer each swarmer by the boid combination.
    let mut idx = 0u32;
    for (tf, mut vel, enemy, sm) in &mut enemies {
        if !is_swarmer(enemy.kind) {
            continue;
        }
        let pos = tf.translation.truncate();
        idx += 1;
        let spd = base * sm.map_or(1.0, |s| s.0);

        let sep = separation(pos, flock.iter().map(|(p, _)| *p), SEPARATION_RADIUS);
        let coh = cohesion(pos, flock.iter().map(|(p, _)| *p), NEIGHBOUR_RADIUS);
        let ali = alignment(pos, flock.iter().copied(), NEIGHBOUR_RADIUS);
        let seek_dir = (core_pos - pos).normalize_or_zero();
        let wnd = wander(idx, t, 1.0);

        let desired = (sep * W_SEPARATION
            + coh * W_COHESION
            + ali * W_ALIGNMENT
            + seek_dir * W_SEEK
            + wnd * W_WANDER)
            .normalize_or_zero()
            * spd;
        vel.0 = approach(vel.0, desired, spd * ACCEL_FRAC);
    }
}
