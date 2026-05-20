//! Wasp enemy: fast evasive yellow swarm fighter.
//!
//! Ported from:
//!   - `js/modules/enemy/enemy-data.js` → `WASP` entry (health 5, speed 3.5,
//!     size 36, movePattern `wasp_zigzag`, shootPattern `wasp_machinegun`)
//!   - `js/modules/enemy/movement.js`   → `waspZigzagMovement()`
//!   - `js/modules/render/shapes.js`    → `drawEnemyWaspShape()`
//!
//! Simplifications vs JS:
//!   - Shape: lyon polygons only — no ellipses, no gradients, no engine-glow
//!     radial gradients, no per-frame pulse; just the wing + body silhouette.
//!   - Movement: two-state FSM (zigzagging / cooldown) with deterministic
//!     segment timing driven by `AiState.phase`. JS uses random segment lengths
//!     (280–480 ms); here segment duration is fixed at 0.38 s per half-cycle
//!     (same feel, no RNG needed in FixedUpdate). Cooldown hover friction
//!     mirrors the JS 0.88 multiplier.
//!   - No bullet firing: fire_cooldown is declared in stats; actual firing is
//!     wired by the parent integrator's enemy_fire system.

use crate::components::{AiState, Enemy, EnemyKind, Ship, Velocity};
use crate::resources::PlayBounds;
use crate::systems::enemy::EnemyStats;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

// ─── Stats ────────────────────────────────────────────────────────────────────

/// JS: health 5, size 36 (radius 18), speed 3.5 px/frame @ 60 fps → ~210 u/s,
/// shootRate 0.7 s (machinegun).
pub fn stats() -> EnemyStats {
    EnemyStats {
        health: 5.0,
        radius: 18.0,
        speed: 210.0,
        fire_cooldown: Some(0.7),
    }
}

// ─── Shape ────────────────────────────────────────────────────────────────────

/// Wasp silhouette: swept razor wings + thorax + head + stinger.
///
/// Authored in Canvas 2D space (+Y down, facing +X) then flipped to Bevy space
/// (+Y up) by negating all Y values.  Sized to `stats().radius` (18 px).
///
/// Emissive yellow HDR values bloom into a yellow glow under Bevy's `Bloom`.
pub fn shape() -> Shape {
    let s = stats().radius; // 18.0

    // ── Outer swept razor wing (mirrored for each side) ───────────────────
    // JS reference points (size = radius * 0.8 ≈ 14.4 px):
    //   moveTo(s*0.15, 0), lineTo(s*0.8, ±s*0.22), lineTo(-s*0.05, ±s*1.05),
    //   lineTo(-s*0.65, ±s*0.9), lineTo(-s*0.75, ±s*0.28), close
    // We use the full `s = 18` for a slightly crisper silhouette at game scale.

    // Build the four sub-paths in one ShapePath chain.
    // Bevy +Y-up: negate every JS y-coordinate.

    // Wing upper (JS side = -1 → positive y in Bevy)
    let wu = [
        Vec2::new( s * 0.15,  0.0),
        Vec2::new( s * 0.80,  s * 0.22),
        Vec2::new(-s * 0.05,  s * 1.05),
        Vec2::new(-s * 0.65,  s * 0.90),
        Vec2::new(-s * 0.75,  s * 0.28),
    ];
    // Wing lower (JS side = +1 → negative y in Bevy)
    let wl = [
        Vec2::new( s * 0.15,  0.0),
        Vec2::new( s * 0.80, -s * 0.22),
        Vec2::new(-s * 0.05, -s * 1.05),
        Vec2::new(-s * 0.65, -s * 0.90),
        Vec2::new(-s * 0.75, -s * 0.28),
    ];

    // ── Thorax (simplified as a pointed diamond, replaces JS ellipse) ─────
    // JS: ellipse center (-s*0.05, 0), radii (s*0.42, s*0.30)
    let thorax = [
        Vec2::new( s * 0.37,  0.0),       // front tip
        Vec2::new(-s * 0.05,  s * 0.30),  // top
        Vec2::new(-s * 0.47,  0.0),       // rear tip
        Vec2::new(-s * 0.05, -s * 0.30),  // bottom
    ];

    // ── Head (JS ellipse center (s*0.38, 0), radii (s*0.30, s*0.22)) ──────
    let head = [
        Vec2::new( s * 0.68,  0.0),
        Vec2::new( s * 0.38,  s * 0.22),
        Vec2::new( s * 0.08,  0.0),
        Vec2::new( s * 0.38, -s * 0.22),
    ];

    // ── Stinger tip ───────────────────────────────────────────────────────
    let stinger = [
        Vec2::new( s * 0.75,  0.0),
        Vec2::new( s * 0.58,  s * 0.10),
        Vec2::new( s * 0.58, -s * 0.10),
    ];

    // Chain all sub-paths.  Each polygon is opened with move_to, closed
    // explicitly, then the next opens a new sub-path.
    let path = ShapePath::new()
        // upper wing
        .move_to(wu[0])
        .line_to(wu[1])
        .line_to(wu[2])
        .line_to(wu[3])
        .line_to(wu[4])
        .close()
        // lower wing
        .move_to(wl[0])
        .line_to(wl[1])
        .line_to(wl[2])
        .line_to(wl[3])
        .line_to(wl[4])
        .close()
        // thorax
        .move_to(thorax[0])
        .line_to(thorax[1])
        .line_to(thorax[2])
        .line_to(thorax[3])
        .close()
        // head
        .move_to(head[0])
        .line_to(head[1])
        .line_to(head[2])
        .line_to(head[3])
        .close()
        // stinger
        .move_to(stinger[0])
        .line_to(stinger[1])
        .line_to(stinger[2])
        .close();

    ShapeBuilder::with(&path)
        // Near-black dark-yellow fill (very dark so wings read as silhouette).
        .fill(Color::linear_rgb(0.04, 0.04, 0.0))
        // Emissive yellow edge — HDR values >1.0 bloom into a yellow halo.
        .stroke((Color::linear_rgb(8.0, 7.0, 1.0), 1.5))
        .build()
}

// ─── AI / Movement ────────────────────────────────────────────────────────────

/// Zigzag segment half-period in seconds (JS: 280–480 ms random; fixed here).
const ZIGZAG_PERIOD: f32 = 0.38;
/// Cooldown hover duration in seconds (JS: 2000–2800 ms random; fixed here).
const COOLDOWN_DURATION: f32 = 2.4;
/// Cooldown friction per frame (mirrors JS 0.88 / frame → per-dt below).
const COOLDOWN_FRICTION: f32 = 0.88;
/// Number of zigzag half-segments before entering cooldown (JS: 3–5).
const ZIGZAG_COUNT_MAX: f32 = 4.0;
/// Lateral multiplier: JS uses speed * 2.6 for the perpendicular dart.
const ZIG_LATERAL: f32 = 2.6;
/// Forward drift weight while zigzagging (JS adds 0.4 px/frame toward player).
const DRIFT_TOWARD: f32 = 24.0; // 0.4 px/frame * 60 fps = 24 u/s

/// `AiState` packing:
///   `phase`  — encodes both the mode and the accumulated timer:
///              ≥ 0.0 → zigzagging; timer = phase (counts up to ZIGZAG_PERIOD)
///              < 0.0 → cooldown; timer = -phase (counts up to COOLDOWN_DURATION)
///   `wander` — x: zigzag sign (+1 or −1); y: accumulated segment count
///
/// FixedUpdate system — `integrate` applies the written `Velocity`.
pub fn ai(
    time: Res<Time>,
    bounds: Res<PlayBounds>,
    player: Query<&Transform, With<Ship>>,
    mut q: Query<(&Enemy, &mut AiState, &mut Velocity, &Transform)>,
) {
    let dt = time.delta_secs();
    let speed = stats().speed;

    // Player position (best-effort; skip AI tick if no player).
    let player_pos = match player.single() {
        Ok(tf) => tf.translation.truncate(),
        Err(_) => return,
    };

    for (enemy, mut ai, mut vel, tf) in &mut q {
        if enemy.kind != EnemyKind::Wasp {
            continue;
        }

        let pos = tf.translation.truncate();

        // ── Initialise on first tick (wander.x == 0.0) ────────────────────
        // Use the entity's position to pick a pseudo-random initial sign.
        if ai.wander.x == 0.0 {
            ai.wander.x = if pos.x >= 0.0 { 1.0 } else { -1.0 };
            ai.wander.y = 0.0; // segment counter
            ai.phase = 0.0;    // zigzag mode, timer = 0
        }

        let in_cooldown = ai.phase < 0.0;

        if in_cooldown {
            // ── Cooldown: bleed speed via friction, then re-enter zigzag ──
            // Per-dt friction: v *= friction^(dt*60) ≈ v * friction^(dt/0.01667)
            // For simplicity apply a dt-scaled lerp-toward-zero.
            let f = COOLDOWN_FRICTION.powf(dt * 60.0);
            vel.0 *= f;

            ai.phase -= dt; // phase is negative; subtract dt → more negative
            if ai.phase < -COOLDOWN_DURATION {
                // Re-enter zigzag with a fresh direction.
                ai.phase = 0.0;
                ai.wander.x = -ai.wander.x; // flip lateral sign
                ai.wander.y = 0.0;
            }
        } else {
            // ── Zigzagging ────────────────────────────────────────────────
            let to_player = player_pos - pos;
            let dist = to_player.length();

            // Direction toward player (unit vector, safe for dist ≈ 0).
            let toward = if dist > 1.0 {
                to_player / dist
            } else {
                Vec2::Y
            };

            // Perpendicular to the toward-player axis, scaled by sign.
            // JS: perpAngle = toPlayerAngle + (PI/2) * zigzagDirection
            let sign = ai.wander.x; // +1 or -1
            let perp = Vec2::new(-toward.y, toward.x) * sign;

            // Velocity = strong lateral dart + gentle forward drift.
            vel.0 = perp * (speed * ZIG_LATERAL) + toward * DRIFT_TOWARD;

            // Clamp to max speed so ZIG_LATERAL doesn't over-shoot.
            let spd = vel.0.length();
            if spd > speed * ZIG_LATERAL {
                vel.0 = vel.0 / spd * (speed * ZIG_LATERAL);
            }

            // Advance segment timer.
            ai.phase += dt;
            if ai.phase >= ZIGZAG_PERIOD {
                // Flip perpendicular direction and count the segment.
                ai.wander.x = -ai.wander.x;
                ai.wander.y += 1.0;
                ai.phase = 0.0;

                if ai.wander.y >= ZIGZAG_COUNT_MAX {
                    // Enter cooldown: a small negative phase flips into the
                    // cooldown branch, which counts down to -COOLDOWN_DURATION.
                    ai.phase = -0.001;
                    ai.wander.y = 0.0;
                }
            }
        }

        // ── Soft boundary bounce (same pattern as drifter_ai) ─────────────
        if tf.translation.x.abs() > bounds.half.x {
            vel.0.x = -vel.0.x.abs() * tf.translation.x.signum();
        }
        if tf.translation.y.abs() > bounds.half.y {
            vel.0.y = -vel.0.y.abs() * tf.translation.y.signum();
        }
    }
}
