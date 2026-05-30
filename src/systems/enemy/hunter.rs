//! Hunter enemy — ported from `js/modules/enemy/enemy-data.js` (HUNTER entry),
//! `js/modules/enemy/movement.js` (`hunterArcMovement`), and
//! `js/modules/enemy/firing.js` (`hunter_single` / `shootBurst3`).
//!
//! The Hunter orbits the player in a sticky one-way arc (CW or CCW chosen at
//! spawn), enriched with a vortex angular speed, periodic slingshot
//! contractions, and frequent lunges straight at the player.
//! JS speed 2.6 px/frame @60 fps ≈ 156 u/s → capped at 160 here.
//! JS size 32 → radius 16 (half-width).
//!
//! Simplifications vs. JS 5.80.x:
//!   • No perpendicular weave (needs a separate `_arcWeavePhase` field;
//!     omitted to stay within the two `AiState` scratch fields).
//!   • Lunge / slingshot dice use `AiState.phase` (accumulated time) in
//!     place of `frameClock.now`; the pseudo-period is identical.
//!   • Firing dispatch lives in the parent `enemy_fire` system; this file
//!     provides only movement + stats + shape.

use crate::components::{AiState, Core, Enemy, EnemyKind, SpeedMul, Velocity};
use crate::resources::PlayBounds;
use crate::systems::enemy::EnemyStats;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

// ── Stats ─────────────────────────────────────────────────────────────────────

/// JS HUNTER: health 5, size 32 (radius 16), speed 2.6 px/frame @60 fps.
/// fire_cooldown mirrors the JS `shootRate: 1.5` (seconds between bursts).
pub fn stats() -> EnemyStats {
    EnemyStats {
        health: 5.0,
        radius: 16.0,
        // JS 2.6 px/frame × 60 fps = 156 u/s; rounded up slightly so the
        // Hunter feels as threatening as documented.
        speed: 160.0,
        fire_cooldown: Some(1.5),
    }
}

// ── Shape ─────────────────────────────────────────────────────────────────────

/// Aggressive red-orange triangle, nose pointing +Y (Bevy forward), radius ≈ 16.
///
/// Canvas2D is +Y-down; in `render/shapes.rs` every Y is negated so the shape
/// lands correctly in Bevy's +Y-up space.  The Hunter's triangle is authored in
/// Canvas space (nose at top = negative Y), then flipped here.
pub fn shape() -> Shape {
    let r = stats().radius;

    // Canvas-space vertices (nose up = negative Y in Canvas):
    //   nose:       (  0,  -r * 1.0 )
    //   right wing: ( +r * 0.85,  +r * 0.72 )
    //   left wing:  ( -r * 0.85,  +r * 0.72 )
    // Notch at rear centre gives an aggressive swept look.
    //   rear notch: (  0,  +r * 0.32 )
    //
    // In Bevy (+Y up) every Y is negated:
    let nose  = Vec2::new(0.0,          r * 1.0);
    let right = Vec2::new( r * 0.85,   -r * 0.72);
    let notch = Vec2::new(0.0,          -r * 0.32);
    let left  = Vec2::new(-r * 0.85,   -r * 0.72);

    let path = ShapePath::new()
        .move_to(nose)
        .line_to(right)
        .line_to(notch)
        .line_to(left)
        .close();

    ShapeBuilder::with(&path)
        // Near-black interior; the emissive stroke provides the red glow via bloom.
        .fill(Color::linear_rgb(0.04, 0.0, 0.0))
        // Red-orange emissive edge — JS glowColor '#ff6666', boosted for HDR bloom.
        .stroke((Color::linear_rgb(8.0, 1.5, 0.5), 2.0))
        .build()
}

// ── AI ────────────────────────────────────────────────────────────────────────

/// Hunter arc-strafe AI — faithful to JS `hunterArcMovement` within the two
/// available `AiState` scratch fields:
///
/// | Rust field        | JS equivalent                     |
/// |-------------------|-----------------------------------|
/// | `phase`           | accumulated time (replaces `frameClock.now`);
///                        also encodes the current arc angle via `sin/cos`. |
/// | `wander.x`        | `_arcAngle`  — current radial angle (rad)        |
/// | `wander.y`        | `_arcDirection` sign (1.0 CW, −1.0 CCW), then    |
///                        repurposed as a packed slingshot/lunge timer       |
///
/// Because we only have two Vec2 components, the per-entity sticky CW/CCW
/// direction is encoded as the sign of `wander.y` and the lunge / slingshot
/// state is derived from `phase` thresholds rather than wall-clock timestamps.
pub fn ai(
    time: Res<Time>,
    bounds: Res<PlayBounds>,
    player: Query<&Transform, With<Core>>,
    mut q: Query<(&Enemy, &mut AiState, &mut Velocity, &Transform, Option<&SpeedMul>)>,
) {
    let dt = time.delta_secs();
    let spd = stats().speed;

    // Preferred orbit radius (JS `_arcRadius` 230–310; we use a fixed 270).
    const ORBIT_RADIUS: f32 = 270.0;
    // Close-range target during a lunge.
    const LUNGE_RADIUS: f32 = 90.0;
    // Contracted radius during a slingshot.
    const SLING_RADIUS: f32 = 130.0;

    // Vortex: angular speed oscillates ±50% around the baseline omega.
    // JS baseline `_arcOmega` ≈ 0.026 rad/tick @60 fps → 1.56 rad/s.
    const OMEGA_BASE: f32 = 1.56;

    // Lunge: every ~4 s, 35% chance → we approximate by using a sawtooth on
    // `phase` with period 4 s, triggering when (phase % 4) < 0.7 (≈35% duty).
    const LUNGE_PERIOD: f32 = 4.0;
    const LUNGE_DUTY: f32 = 0.7;   // seconds of lunge per period

    // Slingshot: every ~5.5 s, 60% chance → sawtooth period 5.5 s,
    // trigger when (phase % 5.5) < 3.3 (≈60% duty).
    const SLING_PERIOD: f32 = 5.5;
    const SLING_DUTY: f32 = 3.3;

    for (enemy, mut ai, mut vel, tf, sm) in &mut q {
        let spd = spd * sm.map_or(1.0, |s| s.0);
        // Ashen Detonator + Tesla Wraith (EN) reuse the hunter orbital arc.
        if !matches!(
            enemy.kind,
            EnemyKind::Hunter | EnemyKind::AshenDetonator | EnemyKind::TeslaWraith
        ) {
            continue;
        }

        // ── Init: choose sticky orbit direction the first time we see this
        //    entity (phase == 0.0 and wander == Vec2::ZERO).
        if ai.phase == 0.0 && ai.wander == Vec2::ZERO {
            // Seed direction from the entity's world X so different spawns
            // diverge without needing rand (no dependency here).
            let dir = if tf.translation.x >= 0.0 { 1.0_f32 } else { -1.0_f32 };
            // Derive initial arc angle from entity position relative to world
            // origin (a reasonable stand-in for targeting the player at spawn).
            let angle = tf.translation.y.atan2(tf.translation.x);
            ai.wander = Vec2::new(angle, dir);
            // Stagger phase so all hunters don't pulse simultaneously.
            ai.phase = tf.translation.x.abs() % LUNGE_PERIOD;
        }

        // Advance accumulated time.
        ai.phase += dt;

        // Unpack scratch.
        let arc_angle  = ai.wander.x;
        let arc_dir    = ai.wander.y.signum(); // +1 or −1

        // ── Derived booleans from phase sawtooth ─────────────────────────────
        let lunge    = (ai.phase % LUNGE_PERIOD)  < LUNGE_DUTY;
        let slingshot = (ai.phase % SLING_PERIOD) < SLING_DUTY;

        // ── Vortex angular speed (JS: `1 + 0.5 * sin(now * 0.0012 + arcAngle * 0.9)`)
        //    Equivalent at 1 s grain: use phase as the time proxy.
        let vortex = 1.0 + 0.5 * (ai.phase * 0.0012 * 1000.0 + arc_angle * 0.9).sin();
        let omega  = OMEGA_BASE * vortex * (if slingshot { 1.4 } else { 1.0 });

        // ── Advance arc angle (freeze during lunge so bullet vector stays true)
        let new_arc_angle = if lunge {
            arc_angle
        } else {
            arc_angle + arc_dir * omega * dt
        };
        ai.wander.x = new_arc_angle;

        // ── Target radius
        let radius_breathe = (ai.phase * 0.6 + new_arc_angle * 1.7).sin() * 30.0;
        let target_radius = if lunge {
            LUNGE_RADIUS
        } else if slingshot {
            SLING_RADIUS + (ai.phase * 4.0).sin() * 20.0
        } else {
            ORBIT_RADIUS + radius_breathe
        };

        // ── Compute target world position
        let target_pos = match player.single() {
            Ok(pt) => {
                let px = pt.translation.x + new_arc_angle.cos() * target_radius;
                let py = pt.translation.y + new_arc_angle.sin() * target_radius;
                Vec2::new(px, py)
            }
            Err(_) => {
                // No player — drift gently inward toward origin.
                Vec2::new(
                    -tf.translation.x * 0.5,
                    -tf.translation.y * 0.5,
                )
            }
        };

        // ── Steer toward target (JS: velocity += (dir/dist)*speed*steer; vel *= 0.92)
        let pos = tf.translation.truncate();
        let delta = target_pos - pos;
        let dist  = delta.length().max(1.0);
        let steer = if lunge { 0.20 } else if slingshot { 0.13 } else { 0.10 };

        vel.0 += (delta / dist) * spd * steer;
        vel.0 *= 0.92; // JS friction coefficient

        // ── Speed cap (JS lunge/slingshot get higher ceilings)
        let cap_mul = if lunge { 2.6 } else if slingshot { 2.0 } else { 1.7 };
        let max_v   = spd * cap_mul;
        let v = vel.0.length();
        if v > max_v {
            vel.0 = vel.0 / v * max_v;
        }

        // ── Soft-bounce at play bounds (mirrors drifter_ai convention)
        if tf.translation.x.abs() > bounds.half.x {
            vel.0.x = -vel.0.x.abs() * tf.translation.x.signum();
        }
        if tf.translation.y.abs() > bounds.half.y {
            vel.0.y = -vel.0.y.abs() * tf.translation.y.signum();
        }
    }
}
