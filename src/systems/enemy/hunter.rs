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

use crate::components::{AiState, Core, Enemy, EnemyKind, FaceTarget, SpeedMul, Velocity};
use crate::resources::PlayBounds;
use crate::systems::enemy::EnemyStats;
use crate::systems::steering::{approach, arrive};
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

/// Per-entity AiState scratch for the strafe cycle:
/// `wander.x` = current slot angle (rad) around the Core; `wander.y` = init flag
/// (0 = uninitialised); `phase` = strafe-dwell countdown (seconds).
const ORBIT_RADIUS: f32 = 230.0; // ring radius the hunters hold around the Core
const ARRIVE_RADIUS: f32 = 28.0; // within this of the slot → strafe
const SLOW_RADIUS: f32 = 130.0; // arrive deceleration zone
const STRAFE_DWELL: f32 = 1.5; // seconds held on a slot before repositioning

/// Hunter strafe AI (Reynolds steering): the triangle attackers **orbit the
/// Core** — `arrive` at a slot on a ring around it, then hold there *facing the
/// Core* (via [`FaceTarget`]) while their `FireCooldown` looses shots at it, then
/// after a short dwell **reposition** to a fresh slot and repeat. The elemental
/// reskins that reuse the hunter shape (Ashen Detonator, Tesla Wraith) share it.
pub fn ai(
    time: Res<Time>,
    bounds: Res<PlayBounds>,
    mut commands: Commands,
    core: Query<&Transform, With<Core>>,
    mut q: Query<(
        Entity,
        &Enemy,
        &mut AiState,
        &mut Velocity,
        &Transform,
        Option<&SpeedMul>,
        Has<FaceTarget>,
    )>,
) {
    let dt = time.delta_secs();
    let base_spd = stats().speed;
    let core_pos = core.single().ok().map(|t| t.translation.truncate());

    for (e, enemy, mut ai, mut vel, tf, sm, facing) in &mut q {
        if !matches!(
            enemy.kind,
            EnemyKind::Hunter | EnemyKind::AshenDetonator | EnemyKind::TeslaWraith
        ) {
            continue;
        }
        let spd = base_spd * sm.map_or(1.0, |s| s.0);
        let pos = tf.translation.truncate();

        // No Core (shouldn't happen mid-run) — drift gently toward origin.
        let Some(core_pos) = core_pos else {
            vel.0 = approach(vel.0, -pos.normalize_or_zero() * spd, spd * 0.2);
            continue;
        };

        // Init: seed the slot bearing from the spawn angle so hunters fan out.
        if ai.wander.y == 0.0 {
            ai.wander.x = (pos - core_pos).to_angle();
            ai.wander.y = 1.0;
            ai.phase = STRAFE_DWELL;
        }

        let slot = core_pos + Vec2::from_angle(ai.wander.x) * ORBIT_RADIUS;

        if pos.distance(slot) > ARRIVE_RADIUS {
            // APPROACH: fly to the slot nose-first (face heading, not the Core).
            let desired = arrive(pos, slot, spd, SLOW_RADIUS);
            vel.0 = approach(vel.0, desired, spd * 0.18);
            if facing {
                commands.entity(e).remove::<FaceTarget>();
            }
            ai.phase = STRAFE_DWELL; // dwell resets fresh once we arrive
        } else {
            // STRAFE: settle on the slot, turn to face the Core, and hold while
            // the firing system shoots at it; then reposition.
            let desired = arrive(pos, slot, spd * 0.4, SLOW_RADIUS);
            vel.0 = approach(vel.0, desired, spd * 0.18);
            if !facing {
                commands.entity(e).insert(FaceTarget(core_pos));
            }
            ai.phase -= dt;
            if ai.phase <= 0.0 {
                // Reposition: jump to a new slot (varied step + per-enemy side).
                let wobble = (pos.x * 0.017 + pos.y * 0.011).sin() * 0.8;
                let dir = if (pos.x as i32).rem_euclid(2) == 0 { 1.0 } else { -1.0 };
                ai.wander.x =
                    (ai.wander.x + dir * (2.1 + wobble)).rem_euclid(std::f32::consts::TAU);
                ai.phase = STRAFE_DWELL;
            }
        }

        // Speed cap.
        let cap = spd * 1.6;
        let v = vel.0.length();
        if v > cap {
            vel.0 = vel.0 / v * cap;
        }

        // Soft-bounce at the play edges (keep them on-screen).
        if pos.x.abs() > bounds.half.x {
            vel.0.x = -vel.0.x.abs() * pos.x.signum();
        }
        if pos.y.abs() > bounds.half.y {
            vel.0.y = -vel.0.y.abs() * pos.y.signum();
        }
    }
}
