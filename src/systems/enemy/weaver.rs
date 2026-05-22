//! Weaver enemy — ported from `js/modules/enemy/enemy-data.js` (WEAVER entry),
//! `js/modules/enemy/movement.js` (`weaverSpinupMovement`), and
//! `js/modules/render/shapes.js` (`drawEnemyWeaverShape` / `pulse_turret`).
//!
//! The Weaver spins up its angular velocity over ~2.4 s while braking, then
//! arcs around the player for ~3.6 s, then brakes and winds down over ~2.6 s.
//! JS speed 2.75 px/frame @60 fps ≈ 165 u/s → capped at 160 (Hunter tier).
//! JS size 32 → radius 16 (half-width).
//!
//! ## AiState encoding
//!
//! | Rust field  | Weaver meaning                                                |
//! |-------------|---------------------------------------------------------------|
//! | `phase`     | elapsed time in the current state (seconds, reset on trans.) |
//! | `wander.x`  | orbit angle around the player (radians)                      |
//! | `wander.y`  | packed: `floor` = state code (0/1/2), `frac` encodes arc      |
//! |             | direction + orbit radius:                                     |
//! |             |   CW  → frac in [0.000, 0.040)  (orbit_r × 0.0001)           |
//! |             |   CCW → frac in [0.500, 0.540)  (0.5 + orbit_r × 0.0001)     |
//! |             | `frac >= 0.4` ↔ CCW (+1), else CW (−1)                       |
//!
//! ## Simplifications vs. JS 5.99.x
//!   • faceAngle / visual spin is not tracked (lyon shape is static).
//!   • Particle spark emission is omitted (no JS particle pool in Rust).
//!   • SENTINEL-specific burst gating is irrelevant here.
//!   • Arc direction is deterministic from spawn X rather than `Math.random()`.

use crate::components::{AiState, Enemy, EnemyKind, Ship, SpeedMul, Velocity};
use crate::resources::PlayBounds;
use crate::systems::enemy::EnemyStats;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

// ── State codes stored in `wander.y.floor()` ─────────────────────────────────
const STATE_SPIN_UP:  f32 = 0.0;
const STATE_ARCING:   f32 = 1.0;
const STATE_COOLDOWN: f32 = 2.0;

/// Fractional tag for arc direction.  CW = 0.0, CCW = 0.5.
/// Both are then combined with `orbit_radius * 0.0001` (range 0.014..0.028)
/// so the total fractional part stays well below 0.04 (CW) or 0.54 (CCW),
/// never straddling the 0.4 boundary used to distinguish them.
const DIR_CCW: f32 = 0.5;
const DIR_CW:  f32 = 0.0;

// ── Duration constants (JS ms → seconds) ─────────────────────────────────────
const SPIN_UP_DURATION:  f32 = 2.4; // JS 2400 ms
const ARC_DURATION:      f32 = 3.6; // JS 3600 ms
const COOLDOWN_DURATION: f32 = 2.6; // JS 2600 ms

/// Orbit angular speed during arcing (JS 0.028 rad/frame @60 fps → 1.68 rad/s).
const ORBIT_RATE: f32 = 1.68;

// ── Stats ─────────────────────────────────────────────────────────────────────

/// JS WEAVER: health 5, size 32 (radius 16), speed 2.75 px/frame @60 fps.
/// fire_cooldown mirrors JS `shootRate: 1.0` (one second between shots).
pub fn stats() -> EnemyStats {
    EnemyStats {
        health: 5.0,
        radius: 16.0,
        // JS 2.75 × 60 = 165 u/s; rounded down to Hunter tier cap.
        speed: 160.0,
        fire_cooldown: Some(1.5),
    }
}

// ── Shape ─────────────────────────────────────────────────────────────────────

/// `pulse_turret` — three-spoke spinning-wheel laser turret (Weaver).
///
/// Ported from `drawEnemyWeaverShape` (`render/shapes.js`):
///   • Outer body ring (radius `size` = r × 0.8 = 12.8 u).
///   • Three spoke arms at 0°/120°/240°, each to 85 % of `size`.
///   • Small nozzle cap (radius `size × 0.18`) at each spoke tip.
///   • Central core disc (radius `size × 0.28`).
///
/// The shape is rotationally symmetric so no +Y / −Y flip is needed.
/// HDR stroke `linear_rgb(8.0, 8.0, 0.8)` matches glowColor `#ffff44`
/// and drives Bevy Bloom for the yellow halo.
pub fn shape() -> Shape {
    let r    = stats().radius;
    let size = r * 0.8; // 12.8 u

    let mut path = ShapePath::new();

    // Outer body ring (12-segment polygon).
    path = path.move_to(Vec2::new(size, 0.0));
    for j in 1..=12_i32 {
        let a = (j as f32 / 12.0) * std::f32::consts::TAU;
        path = path.line_to(Vec2::new(a.cos() * size, a.sin() * size));
    }
    path = path.close();

    // Three spokes + nozzle caps.
    for i in 0..3_i32 {
        let angle = (i as f32 / 3.0) * std::f32::consts::TAU;
        let tip = Vec2::new(angle.cos() * size * 0.85, angle.sin() * size * 0.85);
        path = path.move_to(Vec2::ZERO).line_to(tip);

        // Nozzle cap: 6-segment polygon centred at the spoke tip.
        let cap_c = Vec2::new(angle.cos() * size, angle.sin() * size);
        let cap_r = size * 0.18;
        path = path.move_to(cap_c + Vec2::new(cap_r, 0.0));
        for j in 1..=6_i32 {
            let a = (j as f32 / 6.0) * std::f32::consts::TAU;
            path = path.line_to(cap_c + Vec2::new(a.cos() * cap_r, a.sin() * cap_r));
        }
        path = path.close();
    }

    // Central core disc (8-segment polygon).
    let core_r = size * 0.28;
    path = path.move_to(Vec2::new(core_r, 0.0));
    for j in 1..=8_i32 {
        let a = (j as f32 / 8.0) * std::f32::consts::TAU;
        path = path.line_to(Vec2::new(a.cos() * core_r, a.sin() * core_r));
    }
    path = path.close();

    ShapeBuilder::with(&path)
        // Near-black yellow-tinted hull interior.
        .fill(Color::linear_rgb(0.04, 0.04, 0.0))
        // Yellow-white emissive edge → Bloom halo (JS glowColor `#ffff44`).
        .stroke((Color::linear_rgb(8.0, 8.0, 0.8), 2.0))
        .build()
}

// ── AI ────────────────────────────────────────────────────────────────────────

/// Weaver spinup AI — faithful to JS `weaverSpinupMovement` within the two
/// available `AiState` scratch fields (see module-level encoding table).
///
/// Phase cycle: **spin_up** → **arcing** → **cooldown** → repeat.
///
/// - **spin_up**: hold position (vel × 0.88^dt×60), wait `SPIN_UP_DURATION`.
/// - **arcing**: orbit the player at the locked radius, vel set directly to
///   `speed × 2.8` toward the arc target point.
/// - **cooldown**: friction brake (vel × 0.9^dt×60), wait `COOLDOWN_DURATION`.
pub fn ai(
    time: Res<Time>,
    bounds: Res<PlayBounds>,
    player: Query<&Transform, With<Ship>>,
    mut q: Query<(&Enemy, &mut AiState, &mut Velocity, &Transform, Option<&SpeedMul>)>,
) {
    let dt  = time.delta_secs();
    let spd = stats().speed;

    for (enemy, mut ai, mut vel, tf, sm) in &mut q {
        let spd = spd * sm.map_or(1.0, |s| s.0);
        if enemy.kind != EnemyKind::Weaver {
            continue;
        }

        // ── First-call init ──────────────────────────────────────────────────
        if ai.wander == Vec2::ZERO && ai.phase == 0.0 {
            let dir_tag = if tf.translation.x >= 0.0 { DIR_CCW } else { DIR_CW };
            let init_angle = match player.single() {
                Ok(pt) => {
                    let dx = tf.translation.x - pt.translation.x;
                    let dy = tf.translation.y - pt.translation.y;
                    dy.atan2(dx)
                }
                Err(_) => 0.0,
            };
            ai.wander = Vec2::new(init_angle, STATE_SPIN_UP + dir_tag);
            // Stagger phase so concurrent spawns don't all transition simultaneously.
            ai.phase = tf.translation.x.abs() % SPIN_UP_DURATION;
        }

        // Decode state code and direction tag from wander.y.
        let raw_state  = ai.wander.y.floor();
        // frac encodes direction + orbit radius (see module-level table).
        let frac = ai.wander.y.fract().abs();
        // frac >= 0.4 ↔ CCW (+1.0), else CW (−1.0).
        let (arc_dir, orbit_r_encoded) = if frac >= 0.4 {
            (1.0_f32, frac - 0.5)
        } else {
            (-1.0_f32, frac)
        };
        // dir_tag to re-pack on state transitions (preserves direction).
        let dir_tag = if arc_dir > 0.0 { DIR_CCW } else { DIR_CW };

        // Advance phase timer.
        ai.phase += dt;

        // ── State machine ────────────────────────────────────────────────────
        if raw_state == STATE_SPIN_UP {
            // Hold position with exponential friction.
            vel.0 *= 0.88_f32.powf(dt * 60.0);

            if ai.phase >= SPIN_UP_DURATION {
                // Lock orbit radius from current distance to the player.
                let orbit_radius = match player.single() {
                    Ok(pt) => {
                        let dx = tf.translation.x - pt.translation.x;
                        let dy = tf.translation.y - pt.translation.y;
                        // Refresh orbit angle at transition.
                        ai.wander.x = dy.atan2(dx);
                        (dx * dx + dy * dy).sqrt().clamp(140.0, 280.0)
                    }
                    Err(_) => 220.0,
                };
                // Pack: floor = STATE_ARCING, frac = dir_tag + orbit_r×0.0001.
                // orbit_r ∈ [140,280] → orbit_r×0.0001 ∈ [0.014,0.028] — well
                // below the 0.4 boundary, so direction decode is unambiguous.
                ai.wander.y = STATE_ARCING + dir_tag + orbit_radius * 0.0001;
                ai.phase = 0.0;
            }

        } else if raw_state == STATE_ARCING {
            let orbit_radius = orbit_r_encoded * 10_000.0;

            // Advance orbit angle.
            ai.wander.x += arc_dir * ORBIT_RATE * dt;

            let target_pos = match player.single() {
                Ok(pt) => Vec2::new(
                    pt.translation.x + ai.wander.x.cos() * orbit_radius,
                    pt.translation.y + ai.wander.x.sin() * orbit_radius,
                ),
                Err(_) => Vec2::new(-tf.translation.x * 0.5, -tf.translation.y * 0.5),
            };

            let pos   = tf.translation.truncate();
            let delta = target_pos - pos;
            let dist  = delta.length().max(1.0);
            // JS: `const arcSpeed = this.config.speed * 2.8`
            vel.0 = (delta / dist) * (spd * 2.8);

            // Speed cap.
            let v = vel.0.length();
            let max_v = spd * 3.0;
            if v > max_v {
                vel.0 = vel.0 / v * max_v;
            }

            if ai.phase >= ARC_DURATION {
                // Preserve orbit angle but drop orbit-radius encoding (not
                // needed during cooldown or spin_up; re-locked next transition).
                ai.wander.y = STATE_COOLDOWN + dir_tag;
                ai.phase = 0.0;
            }

        } else {
            // cooldown: friction brake, then restart cycle.
            vel.0 *= 0.9_f32.powf(dt * 60.0);

            if ai.phase >= COOLDOWN_DURATION {
                ai.wander.y = STATE_SPIN_UP + dir_tag;
                ai.phase = 0.0;
            }
        }

        // ── Soft-bounce at play-field edges ──────────────────────────────────
        if tf.translation.x.abs() > bounds.half.x {
            vel.0.x = -vel.0.x.abs() * tf.translation.x.signum();
        }
        if tf.translation.y.abs() > bounds.half.y {
            vel.0.y = -vel.0.y.abs() * tf.translation.y.signum();
        }
    }
}
