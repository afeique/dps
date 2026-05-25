//! Prowler enemy — ported from `js/modules/enemy/enemy-data.js` (PROWLER entry),
//! `js/modules/enemy/movement.js` (`keepDistanceMovement`), and
//! `js/modules/render/shapes.js` (`drawEnemyProwlerShape`).
//!
//! The Prowler is a slow missile-fortress that maintains a preferred range from
//! the player (~280 px) using a three-state machine: patrol → dive → retreat.
//! JS speed 0.75 px/frame @60 fps ≈ 45 u/s.  JS size 45 → radius 22.
//!
//! Simplifications vs. JS 5.80.x:
//!   • State durations are deterministic sawtooth periods derived from
//!     `AiState.phase` rather than random wall-clock timestamps; the average
//!     duty ratios match the JS min/max midpoints.
//!   • Strafe direction is seeded from spawn position sign instead of
//!     `Math.random()` (no rand dependency in this module).
//!   • The rotating targeting-dish detail and missile-pod gradient fills are
//!     reduced to static lyon geometry (no per-frame animation in the mesh).
//!   • Velocity accumulation coefficients are scaled by `dt` so the behaviour
//!     is frame-rate independent; JS ran fixed 60 fps so the raw impulse
//!     magnitudes were effectively per-frame fractions.

use crate::components::{AiState, Enemy, EnemyKind, Ship, SpeedMul, Velocity};
use crate::resources::PlayBounds;
use crate::systems::enemy::EnemyStats;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

// ── Stats ─────────────────────────────────────────────────────────────────────

/// JS PROWLER: health 14, size 45 (radius 22), speed 0.75 px/frame @60 fps.
/// fire_cooldown midpoint of JS cooldown { min: 750, max: 3500 } ms ≈ 2.5 s.
pub fn stats() -> EnemyStats {
    EnemyStats {
        health: 14.0,
        radius: 22.0,
        // JS 0.75 px/frame × 60 fps = 45 u/s
        speed: 45.0,
        fire_cooldown: Some(2.5),
    }
}

// ── Shape ─────────────────────────────────────────────────────────────────────

/// Armored missile-fortress hull ported from `drawEnemyProwlerShape`.
///
/// JS Canvas2D is +Y-down and the nose points right (+X in canvas space).
/// Bevy 2D is +Y-up, so we negate Y on every vertex.  The nose is kept on
/// +X so the entity faces right at spawn; rotation is applied by the ECS
/// transform when the enemy tracks the player.
///
/// Geometry (two layers):
///   1. Main asymmetric hexagonal hull — wider at the rear.
///   2. Missile pod housing rectangles, one per side (±Y from centre).
///
/// The sensor dish and gradient fills from JS are omitted; the HDR stroke
/// provides the magenta bloom that reads as glowing tech detail.
pub fn shape() -> Shape {
    let r = stats().radius;
    let s = r * 0.8; // JS uses `size = radius * 0.8` for all detail placement

    // ── Main hull (7-point polygon, Canvas space → Bevy: negate Y) ─────────
    // Canvas points (nose right):
    //   ( s*1.1,  0     )  nose
    //   ( s*0.6,  s*0.7 )  front flare starboard (canvas +Y → Bevy -Y)
    //   (-s*0.5,  s*0.9 )  rear starboard
    //   (-s*1.1,  s*0.4 )  rear spur starboard
    //   (-s*1.1, -s*0.4 )  rear spur port
    //   (-s*0.5, -s*0.9 )  rear port
    //   ( s*0.6, -s*0.7 )  front flare port

    let hull = [
        Vec2::new( s * 1.1,  0.0    ),   // nose
        Vec2::new( s * 0.6, -s * 0.7),   // front flare starboard (canvas +Y negated)
        Vec2::new(-s * 0.5, -s * 0.9),   // rear starboard
        Vec2::new(-s * 1.1, -s * 0.4),   // rear spur starboard
        Vec2::new(-s * 1.1,  s * 0.4),   // rear spur port
        Vec2::new(-s * 0.5,  s * 0.9),   // rear port
        Vec2::new( s * 0.6,  s * 0.7),   // front flare port
    ];

    // ── Missile pod housings (simplified rects as open polygons) ───────────
    // JS: rect(s*0.1, podY - s*0.22, s*0.7, s*0.44) per side.
    // We approximate each pod as a four-corner outline polygon.
    let pod_x0 = s * 0.1;
    let pod_x1 = s * 0.8;  // pod_x0 + s*0.7
    let pod_half_h = s * 0.22;

    // Starboard pod (canvas +Y → Bevy -Y, podY = -s*0.55)
    let pod_cy_s = -s * 0.55;
    let pod_s = [
        Vec2::new(pod_x0, pod_cy_s - pod_half_h),
        Vec2::new(pod_x1, pod_cy_s - pod_half_h),
        Vec2::new(pod_x1, pod_cy_s + pod_half_h),
        Vec2::new(pod_x0, pod_cy_s + pod_half_h),
    ];

    // Port pod (canvas -Y → Bevy +Y, podY = +s*0.55)
    let pod_cy_p = s * 0.55;
    let pod_p = [
        Vec2::new(pod_x0, pod_cy_p - pod_half_h),
        Vec2::new(pod_x1, pod_cy_p - pod_half_h),
        Vec2::new(pod_x1, pod_cy_p + pod_half_h),
        Vec2::new(pod_x0, pod_cy_p + pod_half_h),
    ];

    let path = ShapePath::new()
        // hull
        .move_to(hull[0])
        .line_to(hull[1])
        .line_to(hull[2])
        .line_to(hull[3])
        .line_to(hull[4])
        .line_to(hull[5])
        .line_to(hull[6])
        .close()
        // starboard pod
        .move_to(pod_s[0])
        .line_to(pod_s[1])
        .line_to(pod_s[2])
        .line_to(pod_s[3])
        .close()
        // port pod
        .move_to(pod_p[0])
        .line_to(pod_p[1])
        .line_to(pod_p[2])
        .line_to(pod_p[3])
        .close();

    ShapeBuilder::with(&path)
        // Deep purple hull interior; bloom makes the edge feel lit.
        .fill(Color::linear_rgb(0.04, 0.0, 0.06))
        // Magenta emissive edge — JS glowColor '#ff44ff', boosted for HDR bloom.
        .stroke((Color::linear_rgb(8.0, 0.4, 8.0), 2.0))
        .build()
}

// ── AI ────────────────────────────────────────────────────────────────────────

/// Prowler `keep_distance` AI — faithful to JS `keepDistanceMovement` within
/// the two available `AiState` scratch fields:
///
/// | Rust field  | JS equivalent                                              |
/// |-------------|-------------------------------------------------------------|
/// | `phase`     | accumulated time (s) — used for state-timer sawtooth and   |
/// |             | the slow lateral breathing in patrol.                       |
/// | `wander.x`  | strafe direction sign (+1.0 or −1.0, a.k.a. `prowlerStrafeDir`). |
/// | `wander.y`  | encoded current state: 0=patrol, 1=dive, 2=retreat.        |
///
/// State machine (deterministic periods approximating JS random windows):
///   patrol   → dive     every PATROL_PERIOD  s  (if player within 400 u)
///   dive     → retreat  after DIVE_PERIOD    s
///   retreat  → patrol   after RETREAT_PERIOD s
///
/// Average JS durations (midpoints of random ranges):
///   patrol  ~2750 ms → 2.75 s
///   dive    ~950 ms  → 0.95 s
///   retreat ~1400 ms → 1.40 s
pub fn ai(
    time: Res<Time>,
    bounds: Res<PlayBounds>,
    player: Query<&Transform, With<Ship>>,
    mut q: Query<(&Enemy, &mut AiState, &mut Velocity, &Transform, Option<&SpeedMul>)>,
) {
    let dt = time.delta_secs();
    let spd = stats().speed;

    // Preferred patrol orbit radius (JS: 280 + sin(now*0.0009)*40 → ~280).
    const PATROL_DIST: f32 = 280.0;
    // Tolerance band before applying radial correction.
    const DIST_TOLERANCE: f32 = 30.0;
    // Emergency retreat threshold: if player is closer than this, pull back.
    const RETREAT_THRESHOLD: f32 = 180.0;

    // State durations (seconds, midpoints of JS random ranges).
    const PATROL_PERIOD:  f32 = 2.75;  // JS: 2000 + rand*1500 → avg 2750 ms
    const DIVE_PERIOD:    f32 = 0.95;  // JS:  700 + rand* 500 → avg  950 ms
    const RETREAT_PERIOD: f32 = 1.40;  // JS: 1000 + rand* 800 → avg 1400 ms

    // State encoding stored in wander.y
    const STATE_PATROL:  f32 = 0.0;
    const STATE_DIVE:    f32 = 1.0;
    const STATE_RETREAT: f32 = 2.0;

    for (enemy, mut ai, mut vel, tf, sm) in &mut q {
        let spd = spd * sm.map_or(1.0, |s| s.0);
        // Spore Carrier / Warden / Lumen Drone (EN) reuse the prowler keep-distance.
        if !matches!(
            enemy.kind,
            EnemyKind::Prowler | EnemyKind::SporeCarrier | EnemyKind::Warden | EnemyKind::LumenDrone
        ) {
            continue;
        }

        // ── Init: first tick (phase == 0, wander == ZERO) ──────────────────
        if ai.phase == 0.0 && ai.wander == Vec2::ZERO {
            // Seed strafe direction from spawn side; entities on opposite sides
            // strafe opposite ways, giving the impression of purposeful
            // encirclement without a rand dependency.
            let strafe_dir = if tf.translation.x >= 0.0 { 1.0_f32 } else { -1.0_f32 };
            // wander.x = strafe dir, wander.y = state (patrol = 0)
            ai.wander = Vec2::new(strafe_dir, STATE_PATROL);
            // Stagger phase so multiple Prowlers don't all dive simultaneously.
            ai.phase = tf.translation.x.abs() % PATROL_PERIOD;
        }

        // Advance timer.
        ai.phase += dt;

        // Unpack state.
        let strafe_dir = ai.wander.x.signum();
        let state = ai.wander.y;

        // ── Compute player-relative geometry ──────────────────────────────
        let pos = tf.translation.truncate();
        let (player_pos, dist, angle) = match player.single() {
            Ok(pt) => {
                let d = pt.translation.truncate() - pos;
                let dist = d.length().max(1.0);
                let angle = d.y.atan2(d.x);
                (pt.translation.truncate(), dist, angle)
            }
            Err(_) => {
                // No player — drift toward origin.
                let d = -pos;
                let dist = d.length().max(1.0);
                let angle = d.y.atan2(d.x);
                (Vec2::ZERO, dist, angle)
            }
        };
        let _ = player_pos; // suppress unused warning

        // ── State transitions (sawtooth on phase) ──────────────────────────
        let (new_state, new_strafe_dir, reset_phase) = if state == STATE_PATROL {
            if ai.phase >= PATROL_PERIOD {
                if dist < 400.0 {
                    (STATE_DIVE, strafe_dir, true)
                } else {
                    // Stay patrolling but flip strafe direction.
                    (STATE_PATROL, -strafe_dir, true)
                }
            } else {
                (state, strafe_dir, false)
            }
        } else if state == STATE_DIVE {
            if ai.phase >= DIVE_PERIOD {
                (STATE_RETREAT, strafe_dir, true)
            } else {
                (state, strafe_dir, false)
            }
        } else {
            // retreat
            if ai.phase >= RETREAT_PERIOD {
                (STATE_PATROL, -strafe_dir, true)
            } else {
                (state, strafe_dir, false)
            }
        };

        if reset_phase {
            ai.phase = 0.0;
        }
        ai.wander = Vec2::new(new_strafe_dir, new_state);

        // ── Compute strafe direction vector (perpendicular to player bearing)
        let strafe_angle = angle + std::f32::consts::FRAC_PI_2 * new_strafe_dir;

        // ── State impulses (JS impulses were per-frame fractions at 60 fps;
        //    multiply by dt*60 to convert to frame-rate-independent values) ─
        let dt60 = dt * 60.0;

        if new_state == STATE_PATROL {
            // Orbit at ~280 u, drifting laterally.
            // JS: breathing ideal dist: 280 + sin(now*0.0009)*40
            let ideal_dist = PATROL_DIST + (ai.phase * 0.054).sin() * 40.0; // 0.054 ≈ 0.0009*60
            if dist < ideal_dist - DIST_TOLERANCE {
                // Too close — move away from player.
                vel.0.x += (-angle).cos() * 0.06 * dt60;
                vel.0.y += (-angle).sin() * 0.06 * dt60;
            } else if dist > ideal_dist + DIST_TOLERANCE {
                // Too far — move toward player.
                vel.0.x += angle.cos() * 0.05 * dt60;
                vel.0.y += angle.sin() * 0.05 * dt60;
            }
            // Lateral strafe.
            vel.0.x += strafe_angle.cos() * 0.04 * dt60;
            vel.0.y += strafe_angle.sin() * 0.04 * dt60;
        } else if new_state == STATE_DIVE {
            // Rush toward player.
            vel.0.x += angle.cos() * 0.18 * dt60;
            vel.0.y += angle.sin() * 0.18 * dt60;
        } else {
            // STATE_RETREAT — pull back hard + strafe to evade return fire.
            vel.0.x += (-angle).cos() * 0.14 * dt60;
            vel.0.y += (-angle).sin() * 0.14 * dt60;
            vel.0.x += strafe_angle.cos() * 0.05 * dt60;
            vel.0.y += strafe_angle.sin() * 0.05 * dt60;
        }

        // ── Emergency retreat if player too close (JS prowler-specific guard)
        if dist < RETREAT_THRESHOLD {
            vel.0.x += (-angle).cos() * 0.8 * dt60;
            vel.0.y += (-angle).sin() * 0.8 * dt60;
        }

        // ── Speed cap (dive gets a higher ceiling, JS: dive*3.0, others*1.8)
        let cap_mul = if new_state == STATE_DIVE { 3.0 } else { 1.8 };
        let max_v = spd * cap_mul;
        let v = vel.0.length();
        if v > max_v {
            vel.0 = vel.0 / v * max_v;
        }

        // ── Soft-bounce at play bounds (mirrors hunter_ai convention) ───────
        if tf.translation.x.abs() > bounds.half.x {
            vel.0.x = -vel.0.x.abs() * tf.translation.x.signum();
        }
        if tf.translation.y.abs() > bounds.half.y {
            vel.0.y = -vel.0.y.abs() * tf.translation.y.signum();
        }
    }
}
