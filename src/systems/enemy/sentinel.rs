//! Sentinel enemy — ported from `js/modules/enemy/enemy-data.js` (SENTINEL entry),
//! `js/modules/enemy/movement.js` (`weaverSpinupMovement`, with Sentinel-specific
//! orbital-radial variation), and `js/modules/render/shapes.js`
//! (`drawEnemySentinelShape`).
//!
//! The Sentinel is a stately orbital shield-sentinel that cycles through three
//! phases (spinning-up → arcing → cooldown), orbiting the player at ~220 px
//! while adding a sinusoidal radial oscillation (±70 px) during the arc phase
//! for an undulating, breathing orbit. It is slower and more deliberate than
//! the Weaver — a defensive mid-tier threat.
//!
//! JS speed 2.5 px/frame @60 fps ≈ 150 u/s.
//! JS size 41 → radius 20.5, rounded to 20 here.
//! fire_cooldown Some(2.2) mirrors the JS `shootRate 1.0` tier at sweep cadence.
//!
//! Simplifications vs. JS 5.99.x:
//!   • The three-burst-before-arc gate (`sentinelBurstsFired < 3`) is omitted;
//!     firing is handled by the parent `enemy_fire` system. The sentinel
//!     transitions spin-up → arc on timer only.
//!   • `faceAngle` spin is not tracked (the shape is static in Bevy — visual
//!     spin would require a Transform rotation, out of scope for this file).
//!   • Sentinel-specific `sentinelArcPhase` sinusoidal radial wave IS ported and
//!     stored in `AiState.wander.x` during the arc phase.
//!   • State machine is encoded in `AiState.phase`:
//!       0.0–SPINUP_DUR         → spinning_up
//!       SPINUP_DUR–ARC_END     → arcing
//!       ARC_END–CYCLE_DUR      → cooldown
//!     `wander.x` stores arc angle (rad), `wander.y` stores arc direction sign
//!     (initialized +1/−1) and, during arcing, the running `sentinelArcPhase`.
//!
//! AiState scratch-field layout:
//!   phase     — accumulated time within the current cycle (seconds, wraps)
//!   wander.x  — current arc angle around the player (rad)
//!   wander.y  — packed: sign(arc_dir) initially; overwritten each arc frame
//!               with the running sinusoidal `arcPhase` while the sign is
//!               preserved via a separate constant per spawn.

use crate::components::{AiState, Enemy, EnemyKind, Ship, SpeedMul, Velocity};
use crate::resources::PlayBounds;
use crate::systems::enemy::EnemyStats;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;
use std::f32::consts::TAU;

// ── Stats ─────────────────────────────────────────────────────────────────────

/// JS SENTINEL: health 10, size 41 (radius ~20), speed 2.5 px/frame @60 fps.
/// fire_cooldown Some(2.2) → matches the sweep-laser cadence tier.
pub fn stats() -> EnemyStats {
    EnemyStats {
        health: 10.0,
        radius: 20.0,
        // JS 2.5 px/frame × 60 fps = 150 u/s
        speed: 150.0,
        fire_cooldown: Some(2.2),
    }
}

// ── Shape ─────────────────────────────────────────────────────────────────────

/// Orbital shield-sentinel shape: nested hexagonal rings with 6 emitter arms
/// and a solid inner-hex hull. Ported from `drawEnemySentinelShape`.
///
/// Canvas2D is +Y-down, centered on origin. This shape is radially symmetric,
/// so the Y-flip is a no-op for the hex vertices — they land identically in
/// Bevy's +Y-up space.
///
/// The JS canvas shape uses two counter-rotating hex rings, 6 emitter arms with
/// glowing nodes, a solid inner hex hull, and a pulsing core radial gradient.
/// In lyon (static tessellation) we approximate with:
///   • outer hex ring (r = size × 1.2) — emissive HDR green stroke
///   • inner hex ring (r = size × 0.88, rotated 30°) — slightly dimmer
///   • 6 emitter-arm lines from r=0.28 to r=0.75 as part of a star polygon
///   • solid inner hex hull (r = size × 0.4) with near-black fill
///
/// All assembled as a single compound `Shape` (lyon builds one mesh).
/// HDR stroke color matches JS glowColor `#44ff44` boosted for bloom.
pub fn shape() -> Shape {
    let r = stats().radius;
    let size = r * 0.8; // JS uses `const size = opts.radius * 0.8`

    // Helper: hex vertices at given radius, optional phase offset (rad)
    let hex_pts = |radius: f32, offset: f32| -> [Vec2; 6] {
        std::array::from_fn(|i| {
            let a = (i as f32 / 6.0) * TAU + offset;
            Vec2::new(a.cos() * radius, a.sin() * radius)
        })
    };

    // ── Outer hex ring (size × 1.2, no offset) ─────────────────────────────
    let outer = hex_pts(size * 1.2, 0.0);
    let mut path = ShapePath::new().move_to(outer[0]);
    for &p in &outer[1..] {
        path = path.line_to(p);
    }
    path = path.close();

    // ── Inner hex ring (size × 0.88, 30° / π/6 offset) ─────────────────────
    let inner = hex_pts(size * 0.88, std::f32::consts::FRAC_PI_6);
    path = path.move_to(inner[0]);
    for &p in &inner[1..] {
        path = path.line_to(p);
    }
    path = path.close();

    // ── 6 emitter arms (inner node → outer node) ───────────────────────────
    // Drawn as individual line segments, grouped into the compound path.
    for i in 0..6_usize {
        let a = (i as f32 / 6.0) * TAU;
        let from = Vec2::new(a.cos() * size * 0.28, a.sin() * size * 0.28);
        let to   = Vec2::new(a.cos() * size * 0.75, a.sin() * size * 0.75);
        path = path.move_to(from).line_to(to);
    }

    // ── Solid inner hex hull (size × 0.4) ───────────────────────────────────
    let hull = hex_pts(size * 0.4, 0.0);
    path = path.move_to(hull[0]);
    for &p in &hull[1..] {
        path = path.line_to(p);
    }
    path = path.close();

    let path = path; // finalize

    ShapeBuilder::with(&path)
        // Near-black dark-green interior hull
        .fill(Color::linear_rgb(0.0, 0.02, 0.01))
        // Bright HDR green emissive edge — JS glowColor `#44ff44`, boosted for bloom.
        .stroke((Color::linear_rgb(0.4, 8.0, 0.8), 2.0))
        .build()
}

// ── AI ────────────────────────────────────────────────────────────────────────

/// Sentinel weaver-spinup AI — faithful to JS `weaverSpinupMovement` with
/// Sentinel-specific sinusoidal orbital-radius variation.
///
/// Three-phase state machine encoded into `AiState.phase` (accumulated seconds):
///
/// ```text
/// phase mod CYCLE_DUR:
///   [0,          SPINUP_DUR)   → spinning_up  (hold with friction, no motion)
///   [SPINUP_DUR, ARC_END)      → arcing        (orbit player at weaverArcRadius ±70)
///   [ARC_END,    CYCLE_DUR)    → cooldown       (friction brake, approach player)
/// ```
///
/// | Rust field   | JS equivalent                                    |
/// |--------------|--------------------------------------------------|
/// | `phase`      | accumulated cycle time; drives state transition  |
/// | `wander.x`   | `weaverArcAngle` — current orbit angle (rad)    |
/// | `wander.y`   | `sentinelArcPhase` (sin wave phase) during arc;  |
/// |              | at init encodes arc_dir sign (±1) via ±0.5 bias  |
///
/// Because `AiState` has only two Vec2 fields and the arc direction must be
/// sticky, the arc direction sign is embedded in `wander.y`'s initial value as
/// a ±1 sentinel (no pun intended) that is preserved by always adding a small
/// positive increment to it — so its sign never changes. The per-entity sticky
/// direction is derived from `wander.y.signum()` at every frame.
pub fn ai(
    time: Res<Time>,
    bounds: Res<PlayBounds>,
    player: Query<&Transform, With<Ship>>,
    mut q: Query<(&Enemy, &mut AiState, &mut Velocity, &Transform, Option<&SpeedMul>)>,
) {
    let dt = time.delta_secs();
    let spd = stats().speed;

    // ── Phase durations (seconds, matching JS ms / 1000) ─────────────────────
    const SPINUP_DUR: f32 = 2.4;  // JS weaverSpinUpDuration 2400 ms
    const ARC_DUR:    f32 = 3.6;  // JS weaverArcDuration    3600 ms
    const COOL_DUR:   f32 = 2.6;  // JS weaverCooldownDuration 2600 ms
    const CYCLE_DUR:  f32 = SPINUP_DUR + ARC_DUR + COOL_DUR; // 8.6 s per cycle
    const ARC_END:    f32 = SPINUP_DUR + ARC_DUR;            // 6.0 s

    // JS weaverArcRadius clamped to max(140, min(dist, 280))
    const BASE_ARC_RADIUS: f32 = 220.0;
    // JS Sentinel sinusoidal radial oscillation ±70 px
    const RADIAL_WAVE_AMP: f32 = 70.0;
    // JS sentinelArcPhase increment per frame @60fps = 0.052; convert to per-second.
    const ARC_WAVE_FREQ:   f32 = 0.052 * 60.0; // ≈ 3.12 rad/s

    // JS orbit: 0.028 rad/frame @60fps → rad/s
    const ORBIT_OMEGA: f32 = 0.028 * 60.0; // 1.68 rad/s
    // JS arc speed multiplier: config.speed × 2.8
    const ARC_SPEED_MUL: f32 = 2.8;

    for (enemy, mut ai, mut vel, tf, sm) in &mut q {
        let spd = spd * sm.map_or(1.0, |s| s.0);
        if enemy.kind != EnemyKind::Sentinel {
            continue;
        }

        // ── Init: first frame (phase == 0, wander == ZERO) ───────────────────
        if ai.phase == 0.0 && ai.wander == Vec2::ZERO {
            // Derive initial arc angle from entity position relative to player
            // (if available) or world origin.
            let init_angle = match player.single() {
                Ok(pt) => {
                    let d = tf.translation.truncate() - pt.translation.truncate();
                    d.y.atan2(d.x)
                }
                Err(_) => tf.translation.y.atan2(tf.translation.x),
            };
            // Choose arc direction from X position (same trick as Hunter)
            let dir = if tf.translation.x >= 0.0 { 1.0_f32 } else { -1.0_f32 };
            ai.wander = Vec2::new(init_angle, dir); // wander.y starts as ±1
            // Stagger phase to desync simultaneous spawns
            ai.phase = tf.translation.x.abs() % SPINUP_DUR;
        }

        // Advance cycle time
        ai.phase += dt;

        // Position within the current cycle
        let cycle_t = ai.phase % CYCLE_DUR;

        // Recover arc direction from the sign of wander.y (preserved throughout)
        let arc_dir = ai.wander.y.signum(); // ±1

        // ── State dispatch ───────────────────────────────────────────────────

        if cycle_t < SPINUP_DUR {
            // ── spinning_up: hold in place with friction ──────────────────────
            // JS: `this.vel *= 0.88` every frame
            vel.0 *= 0.88_f32.powf(dt * 60.0);

            // Soft drift toward player so the sentinel doesn't idle mid-field
            if let Ok(pt) = player.single() {
                let delta = pt.translation.truncate() - tf.translation.truncate();
                let dist = delta.length().max(1.0);
                // Very gentle — 3% of speed, just enough to stay menacing
                vel.0 += (delta / dist) * spd * 0.03;
            }

            // Clamp spin-up velocity
            let v = vel.0.length();
            let cap = spd * 0.5;
            if v > cap {
                vel.0 = vel.0 / v * cap;
            }

        } else if cycle_t < ARC_END {
            // ── arcing: orbit around player with sinusoidal radial wave ───────

            // Advance arc angle (JS: orbitRate 0.028 rad/frame)
            ai.wander.x += arc_dir * ORBIT_OMEGA * dt;
            let arc_angle = ai.wander.x;

            // Advance and store sinusoidal arc phase.
            // The initial arc_dir sign (±1) in wander.y is consumed; we now
            // overwrite wander.y with the running wave phase. Its sign is not
            // inspected from this point — arc_dir is a local derived from the
            // first-ever value. To survive a potential compiler warning we only
            // mutate wander.y here (the sign won't flip because we only add
            // small positives to its absolute value while preserving its sign
            // via the atan offset trick — actually simpler: we track the wave
            // phase as the MAGNITUDE of wander.y, always positive, sign of
            // wander.y remains the arc_dir from init).
            //
            // Practical encoding: wander.y = arc_dir_sign × arc_phase_magnitude
            //   arc_phase = |wander.y|    arc_dir = sign(wander.y)
            let arc_phase_mag = ai.wander.y.abs() + ARC_WAVE_FREQ * dt;
            ai.wander.y = arc_dir * arc_phase_mag; // preserve sign, accumulate magnitude

            let arc_phase = arc_phase_mag;
            let radial_offset = arc_phase.sin() * RADIAL_WAVE_AMP;
            // JS: orbitR = max(60, weaverArcRadius + radialOffset)
            let orbit_r = (BASE_ARC_RADIUS + radial_offset).max(60.0);

            // Target position on the orbit
            let (player_x, player_y) = match player.single() {
                Ok(pt) => (pt.translation.x, pt.translation.y),
                Err(_) => (0.0, 0.0),
            };
            let tx = player_x + arc_angle.cos() * orbit_r;
            let ty = player_y + arc_angle.sin() * orbit_r;

            // JS: vel = (dir/dist) * arcSpeed (direct set, not accumulate)
            let pos = tf.translation.truncate();
            let delta = Vec2::new(tx, ty) - pos;
            let dist = delta.length().max(1.0);
            let arc_spd = spd * ARC_SPEED_MUL;
            vel.0 = (delta / dist) * arc_spd;

            // Speed cap — JS has no explicit cap here but we guard anyway
            let v = vel.0.length();
            if v > arc_spd * 1.2 {
                vel.0 = vel.0 / v * arc_spd * 1.2;
            }

        } else {
            // ── cooldown: friction brake ──────────────────────────────────────
            // JS: `this.vel *= 0.9` each frame
            vel.0 *= 0.9_f32.powf(dt * 60.0);

            // Wind-down speed cap
            let v = vel.0.length();
            let cap = spd * 0.6;
            if v > cap {
                vel.0 = vel.0 / v * cap;
            }
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
