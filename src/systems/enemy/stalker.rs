//! Stalker enemy — ported from `js/modules/enemy/enemy-data.js` (STALKER entry),
//! `js/modules/enemy/movement.js` (`arcMovement`), and
//! `js/modules/render/shapes.js` (`drawEnemyStalkerShape`).
//!
//! The Stalker is a cloaked stealth interceptor with mantis-like blade wings.
//! It uses a three-phase arc movement pattern:
//!   1. **Swooping** — sweeps along a circular arc around an anchor point that
//!      slowly drifts toward the player (JS `arcSwoopDuration` 3 s).
//!   2. **Charging** — decelerates to a halt and locks onto the player while
//!      the laser charges (JS `arcChargeDuration` 1.2 s).
//!   3. **Firing** — holds still, aimed at the player; the parent `enemy_fire`
//!      system fires the shot (JS `arcFiringDuration` 0.8 s).
//!
//! JS speed 3.1 px/frame @60 fps ≈ 186 u/s → capped at 180 here.
//! JS size 38 → radius 19 (half-width).
//!
//! Simplifications vs. JS:
//!   • No rand crate — arc radius, start angle, and sweep direction are derived
//!     deterministically from spawn position so every Stalker diverges visually
//!     without a PRNG dependency.
//!   • Three-phase timer stored in `AiState.phase` (accumulated seconds);
//!     `AiState.wander` packs the current arc angle (x) and sweep direction
//!     sign (y, +1 or −1).
//!   • No separate laser-charge animation; the parent systems handle firing.

use crate::components::{AiState, Enemy, EnemyKind, Ship, Velocity};
use crate::resources::PlayBounds;
use crate::systems::enemy::EnemyStats;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

// ── Stats ─────────────────────────────────────────────────────────────────────

/// JS STALKER: health 7, size 38 (radius 19), speed 3.1 px/frame @60 fps.
/// fire_cooldown mirrors JS `shootRate: 0.3` mapped to a 2.0 s between-burst
/// gap (JS cooldown min 1500 ms, centrally rounded up for Bevy fixed step).
pub fn stats() -> EnemyStats {
    EnemyStats {
        health: 7.0,
        radius: 19.0,
        // JS 3.1 px/frame × 60 fps = 186 u/s; rounded to 180 to match the
        // JS intent (threat tier: fast swooping interceptor).
        speed: 180.0,
        fire_cooldown: Some(2.0),
    }
}

// ── Shape ─────────────────────────────────────────────────────────────────────

/// Cloaked stealth interceptor silhouette — mantis blade wings, plasma cyan
/// edges. Ported from `render/shapes.js` `drawEnemyStalkerShape`.
///
/// JS Canvas2D is +Y-down; Bevy 2D is +Y-up so every Y coordinate is negated.
/// The JS ship faces right (+X in canvas space, nose at `size*1.3, 0`);
/// in Bevy we rotate the whole shape 90° so the nose points +Y (ship forward).
/// We achieve that by swapping the Canvas role of X→Y and Y→X with negation:
///   Canvas (cx, cy)  →  Bevy (cy_negated, cx)
/// i.e.  bevy_x = −canvas_y,  bevy_y = +canvas_x.
pub fn shape() -> Shape {
    let s = stats().radius * 0.92; // JS `size = opts.radius * 0.92`

    // Helper: canvas (cx, cy) → Bevy Vec2 with Y-flip and 90° rotation so
    // the nose (originally at +X canvas) now points +Y (Bevy up/forward).
    // Transform: bevy_x = -cy,  bevy_y = cx
    let cv = |cx: f32, cy: f32| -> Vec2 { Vec2::new(-cy, cx) };

    // ── Main hull — narrow swept fuselage (6 canvas points) ─────────────────
    //   Canvas:  moveTo(s*1.3, 0), lineTo(s*0.4,-s*0.22), lineTo(-s*0.5,-s*0.18),
    //            lineTo(-s*0.75, 0), lineTo(-s*0.5, s*0.18), lineTo(s*0.4, s*0.22)
    let hull = [
        cv( s * 1.3,    0.0),       // nose tip
        cv( s * 0.4,   -s * 0.22),  // upper shoulder
        cv(-s * 0.5,   -s * 0.18),  // upper rear
        cv(-s * 0.75,   0.0),       // tail
        cv(-s * 0.5,    s * 0.18),  // lower rear
        cv( s * 0.4,    s * 0.22),  // lower shoulder
    ];

    // ── Upper mantis blade arm (5 canvas points) ────────────────────────────
    let blade_up = [
        cv( s * 0.55, -s * 0.2),    // root at hull
        cv( s * 1.05, -s * 0.85),   // blade tip (forward-angled)
        cv( s * 0.05, -s * 1.1),    // swept back wingtip
        cv(-s * 0.45, -s * 0.55),   // rear root
        cv(-s * 0.35, -s * 0.18),   // hull attach
    ];

    // ── Lower mantis blade arm (mirror of upper) ────────────────────────────
    let blade_dn = [
        cv( s * 0.55,  s * 0.2),
        cv( s * 1.05,  s * 0.85),
        cv( s * 0.05,  s * 1.1),
        cv(-s * 0.45,  s * 0.55),
        cv(-s * 0.35,  s * 0.18),
    ];

    // Build hull path
    let mut path = ShapePath::new().move_to(hull[0]);
    for &p in &hull[1..] {
        path = path.line_to(p);
    }
    let path = path.close();

    // Build upper blade path
    let mut path = path.move_to(blade_up[0]);
    for &p in &blade_up[1..] {
        path = path.line_to(p);
    }
    let path = path.close();

    // Build lower blade path
    let mut path = path.move_to(blade_dn[0]);
    for &p in &blade_dn[1..] {
        path = path.line_to(p);
    }
    let path = path.close();

    ShapeBuilder::with(&path)
        // Near-black stealth hull interior
        .fill(Color::linear_rgb(0.0, 0.05, 0.06))
        // JS glowColor '#66ffff' — boosted for HDR bloom (cyan plasma edge)
        .stroke((Color::linear_rgb(0.4, 8.0, 9.0), 1.5))
        .build()
}

// ── AI ────────────────────────────────────────────────────────────────────────

/// Stalker arc-swoop AI — faithful to JS `arcMovement` within the two available
/// `AiState` scratch fields:
///
/// | Rust field   | Role                                                      |
/// |--------------|-----------------------------------------------------------|
/// | `phase`      | Accumulated time within the current sub-phase (seconds).  |
/// |              | Resets to 0 at each phase transition.                     |
/// | `wander.x`   | Current arc angle around the anchor point (radians).      |
/// | `wander.y`   | Packed state encoding:                                    |
/// |              |   • fractional part: sweep direction (+0.25 CW, −0.25 CCW)|
/// |              |   • integer part:  0 = swooping, 1 = charging, 2 = firing |
///
/// Because `AiState.phase` resets at each transition, the JS `arcTimer` maps
/// cleanly to `phase` seconds. The JS `arcState` string maps to the integer
/// part of `wander.y`.
///
/// JS durations (converted from ms):
///   swooping  3000 ms → 3.0 s
///   charging  1200 ms → 1.2 s
///   firing     800 ms → 0.8 s
pub fn ai(
    time: Res<Time>,
    bounds: Res<PlayBounds>,
    player: Query<&Transform, With<Ship>>,
    mut q: Query<(&Enemy, &mut AiState, &mut Velocity, &Transform)>,
) {
    let dt = time.delta_secs();
    let spd = stats().speed;

    // Phase durations (seconds)
    const SWOOP_DUR:   f32 = 3.0;
    const CHARGE_DUR:  f32 = 1.2;
    const FIRING_DUR:  f32 = 0.8;

    // Arc geometry
    // JS `arcRadius = 120 + Math.random() * 80` → fixed 160 (midpoint)
    const ARC_RADIUS:  f32 = 160.0;
    // JS `arcSweepSpeed = 0.8 + Math.random() * 0.4` → fixed 1.0 rad/s
    const SWEEP_SPEED: f32 = 1.0;

    // Encoded state constants stored in wander.y integer part
    const STATE_SWOOP:   f32 = 0.0;
    const STATE_CHARGE:  f32 = 1.0;
    const STATE_FIRE:    f32 = 2.0;

    // Sweep direction stored as fractional offset: +0.25 CW, −0.25 CCW
    const DIR_CW:  f32 =  0.25_f32;
    const DIR_CCW: f32 = -0.25_f32;

    for (enemy, mut ai, mut vel, tf) in &mut q {
        if enemy.kind != EnemyKind::Stalker {
            continue;
        }

        // ── Init: first time this entity is seen (phase == 0, wander == 0)
        if ai.phase == 0.0 && ai.wander == Vec2::ZERO {
            // Deterministic "random" from spawn position.
            // Arc start angle: angle from origin to spawn position.
            let start_angle = tf.translation.y.atan2(tf.translation.x);
            // Sweep direction: CW if spawned on right side, CCW otherwise.
            let dir_frac = if tf.translation.x >= 0.0 { DIR_CW } else { DIR_CCW };
            // Stagger initial phase so multiple stalkers don't pulse together.
            ai.phase = (tf.translation.x.abs() % SWOOP_DUR).max(0.0);
            ai.wander = Vec2::new(start_angle, STATE_SWOOP + dir_frac);
        }

        // Advance timer
        ai.phase += dt;

        // Decode state and sweep direction from wander.y
        let state     = ai.wander.y.floor();
        let dir_frac  = ai.wander.y.fract(); // +0.25 or −0.25 (approx)
        let sweep_dir = if dir_frac >= 0.0 { 1.0_f32 } else { -1.0_f32 };

        // Current arc angle
        let arc_angle = ai.wander.x;

        // Player position (or fall back to origin if no player)
        let player_pos = match player.single() {
            Ok(pt) => pt.translation.truncate(),
            Err(_) => Vec2::ZERO,
        };

        let pos = tf.translation.truncate();

        // ── Phase dispatch ────────────────────────────────────────────────────
        if state == STATE_SWOOP {
            // JS: arc center drifts toward player at `centerFollowSpeed = 0.02`
            // per tick @60fps ≈ 1.2 per second. We use an exponential approach:
            // blending factor dt * 1.2 (same effect as JS 0.02 × 60 × dt).
            // We store the implicit arc center as (player − offset at start);
            // a simpler and equivalent formulation: track the anchor directly
            // as the player position minus a fixed offset, which is what JS does
            // with slow lerp. We approximate by computing the target point on the
            // arc relative to current player position (the lerp makes it "follow
            // at a lag", which at 60 fps gives an anchor that is always slightly
            // behind the player — good enough for the swoop feel).
            let anchor = player_pos; // simplified: arc center = player

            // Advance arc angle
            let new_angle = arc_angle + sweep_dir * SWEEP_SPEED * dt;
            ai.wander.x = new_angle;

            // Target position on arc around anchor
            let target = anchor + Vec2::new(new_angle.cos() * ARC_RADIUS,
                                            new_angle.sin() * ARC_RADIUS);

            // Steer toward target — JS sets vel directly to (dir/dist)*speed*1.2
            let delta = target - pos;
            let dist  = delta.length().max(1.0);
            vel.0 = (delta / dist) * spd * 1.2;

            // Transition to charging after SWOOP_DUR
            if ai.phase >= SWOOP_DUR {
                ai.phase = 0.0;
                ai.wander.y = STATE_CHARGE + dir_frac;
            }
        } else if state == STATE_CHARGE {
            // JS: `vel *= 0.8` per frame @60fps  ≈ 0.8^60 ≈ 0 in 1 s.
            // Equivalent exponential decay: vel *= exp(-ln(1/0.8)*60*dt)
            // ln(1/0.8) ≈ 0.2231 → decay_rate = 0.2231 * 60 ≈ 13.39
            let decay = (-13.39_f32 * dt).exp();
            vel.0 *= decay;

            // Transition to firing after CHARGE_DUR
            if ai.phase >= CHARGE_DUR {
                ai.phase = 0.0;
                // Zero out residual velocity before firing hold
                vel.0 = Vec2::ZERO;
                ai.wander.y = STATE_FIRE + dir_frac;
            }
        } else {
            // STATE_FIRE: hold still, aimed at player (parent system fires)
            // JS: `vel *= 0.9` per frame — gentle drift-stop
            let decay = (-6.32_f32 * dt).exp(); // ln(1/0.9)*60 ≈ 6.32
            vel.0 *= decay;

            // Transition back to swooping; generate new arc parameters
            if ai.phase >= FIRING_DUR {
                ai.phase = 0.0;
                // New start angle = current relative angle from player
                let new_start = (pos - player_pos).to_angle();
                // New sweep direction: flip for variety, keyed on angle
                let new_dir_frac = if (new_start * 3.0).sin() >= 0.0 { DIR_CW } else { DIR_CCW };
                ai.wander = Vec2::new(new_start, STATE_SWOOP + new_dir_frac);
            }
        }

        // ── Speed cap ─────────────────────────────────────────────────────────
        let v = vel.0.length();
        if v > spd * 1.5 {
            vel.0 = vel.0 / v * (spd * 1.5);
        }

        // ── Soft-bounce at play bounds (mirrors hunter_ai convention) ─────────
        if tf.translation.x.abs() > bounds.half.x {
            vel.0.x = -vel.0.x.abs() * tf.translation.x.signum();
        }
        if tf.translation.y.abs() > bounds.half.y {
            vel.0.y = -vel.0.y.abs() * tf.translation.y.signum();
        }
    }
}
