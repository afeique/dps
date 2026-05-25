//! Tangerine enemy (in-game name "Bomber") — ported from
//! `js/modules/enemy/enemy-data.js` (TANGERINE entry),
//! `js/modules/enemy/movement.js` (`chasePlayer` TANGERINE branch), and
//! `js/modules/render/shapes.js` (`drawEnemyTangerineShape`).
//!
//! The Bomber is a slow, tanky mine-layer that patrol-chases the player with a
//! wandering steer: it picks a roam direction biased toward the player, holds
//! it for 0.7–1.6 s, then re-rolls.  When close to a wall it blends a repulsion
//! vector back toward the player to stay in the arena.
//!
//! JS speed 2.0 px/frame @60 fps ≈ 120 u/s.
//! JS size 45 → radius 22 (half-width).
//! fire_cooldown Some(2.8) ≈ mid-point of JS `cooldown { min: 1875, max: 7000 }`
//!   converted to seconds: (1875+7000)/2 / 1000 ≈ 4.4 s; clamped down to 2.8 to
//!   keep the Bomber feeling active in the arcade difficulty band.
//!
//! Simplifications vs. JS 5.80.x:
//!   • `mineJustLaid` post-fire slow-down is omitted — the firing system does
//!     not pass that signal through `AiState`; the Bomber coasts at normal
//!     friction instead of the 0.82 brake.
//!   • Roam-direction seeding uses `tf.translation.x.abs() % roam_interval` as
//!     a deterministic stagger rather than `Math.random()` at first sight.
//!   • Wall repulsion is a simple linear ramp (no trigonometric blend), which
//!     matches the JS intent faithfully enough for a 2-D arena.

use crate::components::{AiState, Enemy, EnemyKind, Ship, SpeedMul, Velocity};
use crate::resources::PlayBounds;
use crate::systems::enemy::EnemyStats;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;
use std::f32::consts::TAU;

// ── Stats ─────────────────────────────────────────────────────────────────────

/// JS TANGERINE: health 10, size 45 (radius 22), speed 2.0 px/frame @60 fps.
/// fire_cooldown mirrors the Bomber's mine-laying cadence (~2.8 s).
pub fn stats() -> EnemyStats {
    EnemyStats {
        health: 10.0,
        radius: 22.0,
        // JS 2.0 px/frame × 60 fps = 120 u/s.
        speed: 120.0,
        fire_cooldown: Some(2.8),
    }
}

// ── Shape ─────────────────────────────────────────────────────────────────────

/// Spiked-circle mine-laying Bomber: a filled inner circle with 8 radial spikes
/// extending to the outer radius.  Radially symmetric so the Canvas2D→Bevy Y
/// flip is a no-op.
///
/// JS `drawEnemyTangerineShape`:
///   • innerSize = radius × 0.5  (filled circle)
///   • outerSize = radius × 0.8  (spike tips, ×1.5 for forward spike at angle 0)
///   • 8 evenly-spaced spike lines: innerSize → outerSize
///
/// We replicate this faithfully as a single closed polygon per spike (thin
/// triangular spikes look better with lyon than bare line segments) plus the
/// circle body, all combined into one `Shape` via a single `ShapePath`.
pub fn shape() -> Shape {
    let r = stats().radius;
    let inner = r * 0.5;
    let outer = r * 0.8;
    let spike_count = 8usize;
    // Half-width of each spike base at the inner circle (gives a thin point).
    let spike_half = inner * 0.18;

    let mut path = ShapePath::new();

    // ── Inner circle (approximated as a 24-gon for lyon) ──────────────────
    {
        let steps = 24usize;
        let start = Vec2::new(inner, 0.0);
        path = path.move_to(start);
        for i in 1..steps {
            let a = (i as f32 / steps as f32) * TAU;
            path = path.line_to(Vec2::new(a.cos() * inner, a.sin() * inner));
        }
        path = path.close();
    }

    // ── 8 spike triangles ─────────────────────────────────────────────────
    for i in 0..spike_count {
        let angle = (i as f32 / spike_count as f32) * TAU;
        // JS spike 0 (angle 0 = right) gets ×1.5; in the Bomber's radially
        // symmetric usage that's just a visual accent — we keep it.
        let tip_r = if i == 0 { outer * 1.5 } else { outer };

        // Tip of the spike.
        let tip = Vec2::new(angle.cos() * tip_r, angle.sin() * tip_r);
        // Base corners sit on the inner circle, offset ±spike_half perpendicular.
        let perp = Vec2::new(-angle.sin(), angle.cos());
        let base_l = Vec2::new(angle.cos() * inner, angle.sin() * inner) + perp * spike_half;
        let base_r = Vec2::new(angle.cos() * inner, angle.sin() * inner) - perp * spike_half;

        path = path.move_to(base_l).line_to(tip).line_to(base_r).close();
    }

    ShapeBuilder::with(&path)
        // Near-black orange body; emissive stroke supplies the glow via bloom.
        .fill(Color::linear_rgb(0.05, 0.015, 0.0))
        // Orange-amber emissive edge — JS glowColor `#ffaa66`, boosted for HDR bloom.
        .stroke((Color::linear_rgb(8.0, 3.5, 1.0), 2.0))
        .build()
}

// ── AI ────────────────────────────────────────────────────────────────────────

/// Bomber wandering-chase AI — faithful to JS `chasePlayer` (TANGERINE branch).
///
/// State packing into `AiState`:
///
/// | Rust field    | JS equivalent                                          |
/// |---------------|--------------------------------------------------------|
/// | `wander.x`    | `bomberRoamDir` — current heading angle (rad)          |
/// | `wander.y`    | time until next roam re-roll (seconds remaining)       |
/// | `phase`       | stagger seed / accumulated time (used only for init)   |
///
/// The JS uses absolute `frameClock.now` timestamps; here we store a
/// countdown (`wander.y`) that ticks down by `dt` each frame and is reset to
/// a new interval (0.7–1.6 s) when it expires.  This is semantically identical
/// with no external clock dependency.
pub fn ai(
    time: Res<Time>,
    bounds: Res<PlayBounds>,
    player: Query<&Transform, With<Ship>>,
    mut q: Query<(&Enemy, &mut AiState, &mut Velocity, &Transform, Option<&SpeedMul>)>,
) {
    let dt = time.delta_secs();
    let spd = stats().speed;

    // Wall-repulsion margin: JS 180 px; we map 1:1 to world units.
    const WALL_MARGIN: f32 = 180.0;

    // Roam-change base interval: JS 700 ms + rand(0..900 ms) → 0.7–1.6 s.
    // Without rand we use a pseudo-random step derived from entity position.
    // The interval is recomputed each reset as:
    //   0.7 + (|roamDir| * 137.5) % 0.9  (maps any angle to [0, 0.9) s)
    const ROAM_BASE: f32 = 0.7;
    const ROAM_SPREAD: f32 = 0.9;

    // Distance threshold: when farther than this, spread the wander angle less
    // so the Bomber homes in more directly (JS 250 px, spread 0.35 vs 0.85).
    const FAR_THRESHOLD: f32 = 250.0;
    const SPREAD_FAR: f32 = 0.35;
    const SPREAD_NEAR: f32 = 0.85;

    // Acceleration per frame-equivalent (JS `acc = 0.042` px/frame²; we scale
    // to u/s² so the feel at 60 fps is identical):  0.042 × 60 = 2.52 u/s².
    const ACC: f32 = 2.52;

    for (enemy, mut ai, mut vel, tf, sm) in &mut q {
        let spd = spd * sm.map_or(1.0, |s| s.0);
        // Plaguebearer + Hydra (EN) reuse the tangerine chase.
        if !matches!(enemy.kind, EnemyKind::Tangerine | EnemyKind::Plaguebearer | EnemyKind::Hydra) {
            continue;
        }

        let pos = tf.translation.truncate();

        // ── Resolve player position (fallback: drift toward origin) ───────
        let player_pos = match player.single() {
            Ok(pt) => pt.translation.truncate(),
            Err(_) => Vec2::ZERO,
        };

        let to_player = player_pos - pos;
        let dist_to_player = to_player.length().max(1.0);
        let to_player_angle = to_player.y.atan2(to_player.x);

        // ── Lazy-init: first frame after spawn (phase == 0, wander == ZERO) ─
        if ai.phase == 0.0 && ai.wander == Vec2::ZERO {
            // Seed roam direction biased toward player with a small spread.
            let seed_offset = (tf.translation.x.abs() * 0.37) % (std::f32::consts::PI * 0.5)
                - std::f32::consts::PI * 0.25;
            ai.wander.x = to_player_angle + seed_offset;
            // Stagger the first re-roll time using entity X so Bombers spawned
            // simultaneously don't all re-roll on the same frame.
            ai.wander.y = ROAM_BASE + (tf.translation.x.abs() * 137.5_f32.recip()) % ROAM_SPREAD;
            ai.phase = 1.0; // mark as initialised
        }

        // ── Tick roam countdown ───────────────────────────────────────────
        ai.wander.y -= dt;

        if ai.wander.y <= 0.0 {
            // Time to pick a new roam direction.
            let spread = if dist_to_player > FAR_THRESHOLD {
                SPREAD_FAR
            } else {
                SPREAD_NEAR
            };
            // Deterministic pseudo-random offset: use the current roam angle
            // modulated by a large prime so successive re-rolls diverge.
            let pseudo = (ai.wander.x * 1618.034).sin() * 0.5 + 0.5; // in [0, 1)
            let offset = (pseudo - 0.5) * std::f32::consts::PI * spread;
            ai.wander.x = to_player_angle + offset;

            // Reset timer: ROAM_BASE + pseudo-random fraction of ROAM_SPREAD.
            let t = ((ai.wander.x * 137.5).abs()) % ROAM_SPREAD;
            ai.wander.y = ROAM_BASE + t;
        }

        // ── Wall repulsion (JS: linear ramp, max factor 0.2) ─────────────
        let half = bounds.half;
        // Remap entity position from [−half, +half] to [0, fieldSize] for the
        // JS margin arithmetic, then back.
        let rep_x = {
            let left_gap  = (pos.x + half.x).max(0.0);   // dist from left wall
            let right_gap = (half.x - pos.x).max(0.0);   // dist from right wall
            let left_rep  = if left_gap  < WALL_MARGIN { ((WALL_MARGIN - left_gap)  / WALL_MARGIN) *  0.2 } else { 0.0 };
            let right_rep = if right_gap < WALL_MARGIN { ((WALL_MARGIN - right_gap) / WALL_MARGIN) * -0.2 } else { 0.0 };
            left_rep + right_rep
        };
        let rep_y = {
            let bot_gap = (pos.y + half.y).max(0.0);
            let top_gap = (half.y - pos.y).max(0.0);
            let bot_rep = if bot_gap < WALL_MARGIN { ((WALL_MARGIN - bot_gap) / WALL_MARGIN) *  0.2 } else { 0.0 };
            let top_rep = if top_gap < WALL_MARGIN { ((WALL_MARGIN - top_gap) / WALL_MARGIN) * -0.2 } else { 0.0 };
            bot_rep + top_rep
        };

        let rep = Vec2::new(rep_x, rep_y);
        let rep_len = rep.length();

        // When wall repulsion is strong, blend roam direction toward player
        // to pull back toward the arena centre (JS blend logic).
        let roam_dir = if rep_len > 0.07 {
            let blend_y = rep_y + to_player_angle.sin() * 0.6;
            let blend_x = rep_x + to_player_angle.cos() * 0.6;
            let blended = blend_y.atan2(blend_x);
            // Also reset timer so the Bomber commits to the new heading.
            ai.wander.y = 0.7;
            blended
        } else {
            ai.wander.x
        };

        // ── Accelerate in roam direction + wall repulsion ─────────────────
        vel.0.x += roam_dir.cos() * ACC * dt + rep_x * dt * 60.0;
        vel.0.y += roam_dir.sin() * ACC * dt + rep_y * dt * 60.0;

        // ── Speed cap ─────────────────────────────────────────────────────
        let speed = vel.0.length();
        if speed > spd {
            vel.0 = vel.0 / speed * spd;
        }

        // ── Soft-bounce at hard play boundary (mirrors hunter_ai convention)
        if tf.translation.x.abs() > bounds.half.x {
            vel.0.x = -vel.0.x.abs() * tf.translation.x.signum();
        }
        if tf.translation.y.abs() > bounds.half.y {
            vel.0.y = -vel.0.y.abs() * tf.translation.y.signum();
        }
    }
}
