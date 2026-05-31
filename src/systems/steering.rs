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
}
