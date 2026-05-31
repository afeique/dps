//! Reynolds steering primitives (pure helpers) for enemy AI.
//!
//! Craig Reynolds' classic "steering behaviors": each returns a *desired
//! velocity* (or a steering nudge) that the per-kind AI blends and then feeds
//! into `Velocity`. Keeping them pure (no ECS) makes the movement math
//! testable and reusable across enemy kinds.

use bevy::prelude::*;

/// **Seek**: desired velocity straight at `target`, at full `max_speed`.
pub fn seek(pos: Vec2, target: Vec2, max_speed: f32) -> Vec2 {
    (target - pos).normalize_or_zero() * max_speed
}

/// **Arrive**: like [`seek`], but ramps speed down inside `slow_radius` so the
/// agent eases onto the target instead of overshooting/orbiting it.
pub fn arrive(pos: Vec2, target: Vec2, max_speed: f32, slow_radius: f32) -> Vec2 {
    let to = target - pos;
    let dist = to.length();
    if dist < 1.0 {
        return Vec2::ZERO;
    }
    let speed = if dist < slow_radius {
        max_speed * (dist / slow_radius)
    } else {
        max_speed
    };
    to / dist * speed
}

/// **Flee**: desired velocity straight *away* from `threat`, at full `max_speed`
/// — the mirror of [`seek`]. Used by kiting enemies that back off when something
/// gets too close.
pub fn flee(pos: Vec2, threat: Vec2, max_speed: f32) -> Vec2 {
    (pos - threat).normalize_or_zero() * max_speed
}

/// **Separation**: a repulsion vector away from every neighbour within `radius`,
/// weighted stronger the closer they are. Keeps a swarm from collapsing onto one
/// point. Returns a raw push (not normalised) — scale it by a weight at the call site.
pub fn separation(pos: Vec2, neighbours: impl Iterator<Item = Vec2>, radius: f32) -> Vec2 {
    let mut push = Vec2::ZERO;
    for n in neighbours {
        let off = pos - n;
        let d = off.length();
        if d > 1e-3 && d < radius {
            push += off / d * (1.0 - d / radius);
        }
    }
    push
}

/// **Cohesion**: steer toward the average position (centre of mass) of neighbours
/// within `radius` — the flock-pull half of boid flocking. Returns a unit-ish
/// vector toward the centroid (zero if no neighbours), scale it at the call site.
pub fn cohesion(pos: Vec2, neighbours: impl Iterator<Item = Vec2>, radius: f32) -> Vec2 {
    let mut sum = Vec2::ZERO;
    let mut n = 0u32;
    for p in neighbours {
        if pos.distance_squared(p) < radius * radius {
            sum += p;
            n += 1;
        }
    }
    if n == 0 {
        return Vec2::ZERO;
    }
    (sum / n as f32 - pos).normalize_or_zero()
}

/// **Alignment**: steer toward the average *heading* of neighbour velocities
/// within `radius` — the match-velocity half of boid flocking. Returns a unit-ish
/// vector along the mean heading (zero if no moving neighbours).
pub fn alignment(pos: Vec2, neighbours: impl Iterator<Item = (Vec2, Vec2)>, radius: f32) -> Vec2 {
    let mut sum = Vec2::ZERO;
    let mut n = 0u32;
    for (p, v) in neighbours {
        if pos.distance_squared(p) < radius * radius {
            sum += v;
            n += 1;
        }
    }
    if n == 0 {
        return Vec2::ZERO;
    }
    (sum / n as f32).normalize_or_zero()
}

/// **Wander**: a small, smoothly-varying jitter vector for idle drift. Deterministic
/// from `seed` + time `t` (two out-of-phase sines), so it needs no RNG and is
/// reproducible. Add it to a seek/orbit desired-velocity to break up dead-straight
/// paths. Magnitude ≈ `strength`.
pub fn wander(seed: u32, t: f32, strength: f32) -> Vec2 {
    let a = seed as f32 * 0.618_034;
    Vec2::new((t * 1.7 + a).sin(), (t * 2.3 + a * 1.7).cos()) * strength
}

/// Force-limited approach: nudge `vel` toward the `desired` velocity by at most
/// `accel` this step (the classic "steering = desired − velocity, truncated"),
/// so the agent eases into turns/stops instead of snapping. `desired` is already
/// speed-bounded by [`seek`]/[`arrive`], so no extra cap is needed.
pub fn approach(vel: Vec2, desired: Vec2, accel: f32) -> Vec2 {
    vel + (desired - vel).clamp_length_max(accel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seek_points_at_target() {
        let v = seek(Vec2::ZERO, Vec2::new(10.0, 0.0), 100.0);
        assert!((v.x - 100.0).abs() < 1e-3 && v.y.abs() < 1e-3);
    }

    #[test]
    fn arrive_slows_near_target() {
        let far = arrive(Vec2::ZERO, Vec2::new(500.0, 0.0), 100.0, 80.0);
        let near = arrive(Vec2::ZERO, Vec2::new(40.0, 0.0), 100.0, 80.0);
        assert!((far.length() - 100.0).abs() < 1e-3, "full speed far out");
        assert!(near.length() < far.length(), "eases off inside slow radius");
    }

    #[test]
    fn separation_pushes_away_from_crowd() {
        let push = separation(Vec2::ZERO, [Vec2::new(5.0, 0.0)].into_iter(), 50.0);
        assert!(push.x < 0.0, "pushed away from the neighbour on +X");
    }

    #[test]
    fn flee_points_away_from_threat() {
        let v = flee(Vec2::ZERO, Vec2::new(10.0, 0.0), 100.0);
        assert!((v.x + 100.0).abs() < 1e-3 && v.y.abs() < 1e-3, "flees on −X, got {v:?}");
    }

    #[test]
    fn cohesion_pulls_toward_centroid() {
        // Two neighbours off on +X → cohesion steers +X.
        let v = cohesion(
            Vec2::ZERO,
            [Vec2::new(10.0, 0.0), Vec2::new(20.0, 0.0)].into_iter(),
            100.0,
        );
        assert!(v.x > 0.0 && v.y.abs() < 1e-3, "toward the centroid on +X, got {v:?}");
    }

    #[test]
    fn cohesion_ignores_distant_neighbours() {
        let v = cohesion(Vec2::ZERO, [Vec2::new(500.0, 0.0)].into_iter(), 100.0);
        assert_eq!(v, Vec2::ZERO, "out-of-radius neighbours don't pull");
    }

    #[test]
    fn alignment_matches_mean_heading() {
        let v = alignment(
            Vec2::ZERO,
            [(Vec2::new(5.0, 0.0), Vec2::new(0.0, 30.0))].into_iter(),
            100.0,
        );
        assert!(v.y > 0.0 && v.x.abs() < 1e-3, "aligns to +Y heading, got {v:?}");
    }

    #[test]
    fn wander_is_bounded_and_deterministic() {
        let a = wander(7, 1.0, 20.0);
        let b = wander(7, 1.0, 20.0);
        assert_eq!(a, b, "deterministic for the same seed+time");
        assert!(a.length() <= 20.0 * 1.5 + 1e-3, "bounded ~strength, got {a:?}");
        assert_ne!(wander(7, 1.0, 20.0), wander(7, 2.0, 20.0), "varies over time");
    }
}
