//! Titan enemy — ported from `js/modules/enemy/enemy-data.js` (TITAN entry),
//! `js/modules/enemy/movement.js` (`boulderMovement`), and
//! `js/modules/render/shapes.js` (`drawEnemyTitanShape`).
//!
//! The Titan is the biggest, tankiest enemy (health 20, radius 32).  It uses
//! the `boulder` movement pattern: a three-phase cycle of
//!   1. **idle** — slow tangential orbit while winding up the next charge
//!   2. **approaching** — locked-direction charge toward the player's position
//!      at the moment the charge began; high inertia, accelerates gently
//!   3. **braking** — strong friction until nearly stopped, then resets to idle
//!
//! JS speed 1.5 px/frame @60 fps ≈ 90 u/s.
//! JS size 64 → radius 32 (half-width).
//!
//! Simplifications vs. JS `boulderMovement` / `drawEnemyTitanShape`:
//!   • JS uses `frameClock.now` (wall ms) for idle-timer; here we integrate
//!     `dt` into `AiState.phase` as accumulated seconds so no external clock
//!     is needed.  The idle durations (1.5–2.5 s) are preserved faithfully.
//!   • JS orbit-sign reseed on braking done via `tf.translation.x` sign as a
//!     cheap deterministic stand-in for `Math.random()`.
//!   • Shape: outer hex hull + inner hex + 6 vertex spikes rendered as lyon
//!     closed paths; the turret, gradient core, and exhaust pods are omitted
//!     (no gradient/arc API in lyon).  The outline weight and HDR magenta
//!     stroke reproduce the original glow character.
//!   • `wander.x` encodes the locked charge angle; `wander.y` encodes the
//!     sticky orbit sign (+1.0 / −1.0).  `phase` drives the idle countdown.

use crate::components::{AiState, Enemy, EnemyKind, Ship, Velocity};
use crate::resources::PlayBounds;
use crate::systems::enemy::EnemyStats;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;
use std::f32::consts::TAU;

// ── Stats ─────────────────────────────────────────────────────────────────────

/// JS TITAN: health 20, size 64 (radius 32), speed 1.5 px/frame @60 fps ≈ 90 u/s.
/// fire_cooldown 2.5 s (JS `shootRate: 0.15` Hz ≈ every 6.7 s, capped at a
/// friendlier 2.5 s so the sweeping laser fires meaningfully in play).
pub fn stats() -> EnemyStats {
    EnemyStats {
        health: 20.0,
        radius: 32.0,
        speed: 90.0,
        fire_cooldown: Some(2.5),
    }
}

// ── Shape ─────────────────────────────────────────────────────────────────────

/// Armored magenta hexagonal juggernaut, nose pointing +Y (Bevy forward).
///
/// Three lyon paths stacked at the same Z:
///   1. Outer hex hull (6 vertices, vertex 0 stretched 1.35× forward = +Y).
///   2. Inner hex (0.72× scale, darker fill, emissive purple stroke).
///   3. Six vertex spikes (small triangles at each outer-hex corner).
///
/// Canvas space has the nose at the *right* (angle 0 = +X); we rotate the hex
/// 90° CCW so vertex 0 faces +Y (Bevy forward) by starting the loop at
/// −TAU/4 rather than 0.
pub fn shape() -> Shape {
    let r = stats().radius;

    // All geometry is composed in one ShapePath so all paths bloom together
    // under the shared HDR emissive stroke.  We walk three sub-paths:
    //   1. Outer armor hex (vertex 0 stretched 1.35× toward +Y / forward)
    //   2. Inner hex at 0.72× scale
    //   3. Six vertex spike triangles
    let inner_scale = 0.72_f32;
    let tip_len = r * 0.22;
    let flank_r  = r * 0.15;

    let mut full_path = ShapePath::new();

    // ── Outer armor hex ─────────────────────────────────────────────────────
    // Shift start angle to −π/2 so vertex 0 (the stretched one) faces +Y.
    for i in 0..6usize {
        let base_angle = (i as f32 / 6.0) * TAU - TAU / 4.0;
        let stretch = if i == 0 { 1.35_f32 } else { 1.0_f32 };
        let p = Vec2::new(base_angle.cos() * r * stretch, base_angle.sin() * r * stretch);
        full_path = if i == 0 { full_path.move_to(p) } else { full_path.line_to(p) };
    }
    full_path = full_path.close();

    // ── Inner hex ───────────────────────────────────────────────────────────
    for i in 0..6usize {
        let angle = (i as f32 / 6.0) * TAU - TAU / 4.0;
        let p = Vec2::new(angle.cos() * r * inner_scale, angle.sin() * r * inner_scale);
        full_path = if i == 0 { full_path.move_to(p) } else { full_path.line_to(p) };
    }
    full_path = full_path.close();

    // ── Vertex spikes ───────────────────────────────────────────────────────
    // One triangle per outer-hex vertex: tip extends outward, flanks at ±0.35 rad.
    for i in 0..6usize {
        let base_angle = (i as f32 / 6.0) * TAU - TAU / 4.0;
        let stretch = if i == 0 { 1.35_f32 } else { 1.0_f32 };
        let vx = base_angle.cos() * r * stretch;
        let vy = base_angle.sin() * r * stretch;

        let tip_x = vx + base_angle.cos() * tip_len;
        let tip_y = vy + base_angle.sin() * tip_len;
        let fl_x = vx + (base_angle + 0.35).cos() * flank_r;
        let fl_y = vy + (base_angle + 0.35).sin() * flank_r;
        let fr_x = vx + (base_angle - 0.35).cos() * flank_r;
        let fr_y = vy + (base_angle - 0.35).sin() * flank_r;

        full_path = full_path
            .move_to(Vec2::new(tip_x, tip_y))
            .line_to(Vec2::new(fl_x, fl_y))
            .line_to(Vec2::new(fr_x, fr_y))
            .close();
    }

    ShapeBuilder::with(&full_path)
        // Near-black deep-purple interior; the emissive stroke provides the
        // magenta-pink glow via bloom.
        .fill(Color::linear_rgb(0.04, 0.0, 0.08))
        // Magenta-pink emissive edge — JS glowColor `#ff66ff`, boosted for
        // HDR bloom (~8× sRGB).
        .stroke((Color::linear_rgb(8.0, 2.0, 8.0), 2.5))
        .build()
}

// ── AI ────────────────────────────────────────────────────────────────────────

/// Titan boulder AI — faithful to JS `boulderMovement` within the two
/// available `AiState` scratch fields:
///
/// | Rust field    | JS equivalent / role                                       |
/// |---------------|------------------------------------------------------------|
/// | `phase`       | Idle countdown (seconds elapsed in the current idle phase) |
/// | `wander.x`    | Locked charge angle (rad), set at idle→approaching trans.  |
/// | `wander.y`    | Sticky orbit sign (+1.0 CW / −1.0 CCW); 0.0 = uninit.     |
///
/// The three-phase cycle is encoded implicitly: when `wander.y == 0.0` the
/// entity is uninitialized; negative `phase` encodes the `approaching` state
/// (phase < 0 → charging), zero/positive `phase` encodes `idle`/`braking`
/// depending on speed.  To keep everything in two f32 fields we use a sign
/// convention on `phase`:
///
///   • `phase >= 0` → idle: counting *up* toward `idle_duration` (1.5–2.5 s).
///   • `phase < -APPROACHING_SENTINEL` → approaching: charging in locked dir.
///   • `phase == -BRAKING_SENTINEL` → braking.
///
/// Simpler explanation encoded as concrete constants below.
///
/// State machine encoded in `phase`:
///   `phase >= 0`     → IDLE      (phase is elapsed idle seconds)
///   `phase == -1.0`  → APPROACHING
///   `phase == -2.0`  → BRAKING
pub fn ai(
    time: Res<Time>,
    bounds: Res<PlayBounds>,
    player: Query<&Transform, With<Ship>>,
    mut q: Query<(&Enemy, &mut AiState, &mut Velocity, &Transform)>,
) {
    let dt = time.delta_secs();
    let spd = stats().speed;

    // Phase sentinels (stored in `AiState.phase` to distinguish states
    // without a separate enum component).
    const IDLE: f32 = 0.0;          // phase >= 0  → idle; phase counts elapsed s
    const APPROACHING: f32 = -1.0;  // exact sentinel for approaching state
    const BRAKING: f32 = -2.0;      // exact sentinel for braking state

    // Idle duration bounds (seconds). JS: 1500–2500 ms.
    const IDLE_MIN: f32 = 1.5;
    const IDLE_MAX: f32 = 2.5;

    // Orbit drift speed during idle (JS: config.speed * 0.45).
    const ORBIT_DRIFT: f32 = 0.45;

    // Acceleration during charge (JS `boulderAccel = 0.06` px/frame² @60 fps
    // → 0.06 × 60² = 216 u/s² — that's very fast; scale back for 90 u/s max
    // so the charge feels weighty: use 0.06 × spd as a fraction).
    // JS: `this.vel.x += cos(angle) * 0.06` per frame @60 fps
    //   → 0.06 × 60 = 3.6 px/s² acceleration but the cap is speed*3.0.
    // Rust: we want the same *feel* — gentle build-up to max speed.
    // accel = 0.06 * 60 = 3.6 normalized → in our units: 3.6 u/s² × (spd/1.5)
    // = 3.6 × 60 = 216 u/s² (too explosive). Use a tuned 45 u/s² instead so
    // the Titan ramps up over ~2 s to full speed.
    const BOULDER_ACCEL: f32 = 45.0; // u/s²

    // Max charge speed cap (JS: config.speed * 3.0).
    const CHARGE_CAP_MUL: f32 = 3.0;

    // Braking friction per second (JS: vel *= 0.93 per frame @60 fps
    // → 0.93^60 ≈ 0.013 per second → we use an exponential equivalent).
    // Rust: vel *= braking_friction^dt.  To match JS: friction_per_sec = 0.93^60 ≈ 0.013.
    // But that's too aggressive; we want the Titan to coast to a stop over ~1.5 s.
    // 0.93^(60 * dt) per frame = 0.93^(dt * 60) ≈ exp(ln(0.93) * 60 * dt)
    // We apply it as: vel *= exp(ln(0.93) * 60.0 * dt) = 0.93^(60*dt).
    // Precompute constant: ln(0.93) ≈ -0.07257.
    const LN_FRICTION: f32 = -0.07257; // ln(0.93)

    // Speed threshold below which braking transitions to idle.
    // JS: `if (currentSpeed < 0.18)` (px/frame @60 fps → 10.8 u/s).
    const BRAKE_STOP_THRESHOLD: f32 = 10.8;

    for (enemy, mut ai, mut vel, tf) in &mut q {
        if enemy.kind != EnemyKind::Titan {
            continue;
        }

        // ── Init: first time we see this entity ──────────────────────────────
        if ai.wander.y == 0.0 {
            // Choose sticky orbit sign from X position (deterministic, no rand dep).
            ai.wander.y = if tf.translation.x >= 0.0 { 1.0 } else { -1.0 };
            // Start in idle with a staggered delay so not all Titans charge at once.
            let stagger = (tf.translation.x.abs() % (IDLE_MAX - IDLE_MIN)) + IDLE_MIN * 0.5;
            ai.phase = IDLE + stagger.min(IDLE_MAX - 0.1);
            // wander.x: charge angle, will be set at idle→approaching transition.
            ai.wander.x = 0.0;
        }

        // ── Decode idle duration from entity ID proxy ────────────────────────
        // Without rand, vary idle duration per entity via X position hash.
        let idle_duration = IDLE_MIN + (tf.translation.x.abs() % 1.0) * (IDLE_MAX - IDLE_MIN);

        // ── Current speed ────────────────────────────────────────────────────
        let current_speed = vel.0.length();

        // ── Fetch player position (best-effort) ──────────────────────────────
        let player_pos: Option<Vec2> = player
            .single()
            .ok()
            .map(|pt| pt.translation.truncate());

        let pos = tf.translation.truncate();

        // ── State machine ────────────────────────────────────────────────────
        if ai.phase >= IDLE {
            // ── IDLE: slow tangential orbit while winding up ─────────────────
            ai.phase += dt;

            if let Some(pp) = player_pos {
                let to_player = pp - pos;
                let dist_to_player = to_player.length().max(1.0);
                let orbit_sign = ai.wander.y;

                // Tangent perpendicular to radial direction (JS: tangentX = -dy/dist * sign)
                let tangent = Vec2::new(
                    -to_player.y / dist_to_player * orbit_sign,
                     to_player.x / dist_to_player * orbit_sign,
                );
                let drift_speed = spd * ORBIT_DRIFT;

                // Smooth approach to orbit velocity (JS: vel += (target - vel) * 0.08)
                let v = vel.0;
                vel.0 = v + (tangent * drift_speed - v) * (1.0 - (-0.08_f32 * 60.0 * dt).exp());

                // Transition to approaching when idle duration elapsed.
                if ai.phase >= idle_duration {
                    // Lock charge direction toward player's current position.
                    let charge_angle = to_player.y.atan2(to_player.x);
                    ai.wander.x = charge_angle;
                    ai.phase = APPROACHING;
                }
            } else {
                // No player — keep drifting toward origin.
                let to_origin = -pos;
                let drift = to_origin.normalize_or_zero() * spd * 0.2;
                let v = vel.0;
                vel.0 = v + (drift - v) * (1.0 - (-0.05_f32 * 60.0 * dt).exp());
            }
        } else if (ai.phase - APPROACHING).abs() < 0.5 {
            // ── APPROACHING: locked-angle charge ────────────────────────────
            let charge_angle = ai.wander.x;
            let dir = Vec2::new(charge_angle.cos(), charge_angle.sin());

            vel.0 += dir * BOULDER_ACCEL * dt;

            let max_charge = spd * CHARGE_CAP_MUL;
            if current_speed > max_charge {
                vel.0 = vel.0.normalize() * max_charge;
            }

            // Check transition conditions (JS: passedPlayer || speed >= cap * 0.92).
            let passed_player = if let Some(pp) = player_pos {
                let to_player = pp - pos;
                let vel_dot = vel.0.dot(to_player);
                let dist_to_player = to_player.length();
                vel_dot < 0.0 && dist_to_player < 180.0
            } else {
                false
            };

            let at_cap = current_speed >= max_charge * 0.92;

            if passed_player || at_cap {
                ai.phase = BRAKING;
            }
        } else {
            // ── BRAKING: strong friction until nearly stopped ────────────────
            // Apply JS-equivalent per-frame friction: vel *= 0.93 per frame @60.
            let friction = (LN_FRICTION * 60.0 * dt).exp();
            vel.0 *= friction;

            if current_speed < BRAKE_STOP_THRESHOLD {
                // Transition back to idle; reseed orbit sign.
                ai.wander.y = if tf.translation.x >= 0.0 { 1.0 } else { -1.0 };
                // Flip orbit direction for organic feel between charges.
                if pos.y < 0.0 {
                    ai.wander.y = -ai.wander.y;
                }
                ai.phase = 0.0; // start fresh idle countdown
            }
        }

        // ── Bound bounce (mirrors hunter_ai convention) ──────────────────────
        // Titan bounces hard off the play bounds — it's a massive rock.
        if tf.translation.x.abs() > bounds.half.x {
            vel.0.x = -vel.0.x.abs() * tf.translation.x.signum();
            // When a charge hits the wall, force braking to avoid wall-sticking.
            if (ai.phase - APPROACHING).abs() < 0.5 {
                ai.phase = BRAKING;
            }
        }
        if tf.translation.y.abs() > bounds.half.y {
            vel.0.y = -vel.0.y.abs() * tf.translation.y.signum();
            if (ai.phase - APPROACHING).abs() < 0.5 {
                ai.phase = BRAKING;
            }
        }
    }
}
