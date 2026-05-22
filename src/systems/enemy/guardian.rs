//! Guardian enemy — tanky green fortress that patrols in axis-aligned cardinal
//! bursts (the JS `squareMovement` pattern) and fires spread shots.
//!
//! JS source of truth:
//!   • `js/modules/enemy/enemy-data.js`  — GUARDIAN entry (health 12, speed 1.25, size 48)
//!   • `js/modules/enemy/movement.js`    — `squareMovement` / `squareBurstState`
//!   • `js/modules/render/shapes.js`     — `drawEnemyGuardianShape`

use crate::components::{AiState, Enemy, EnemyKind, Ship, SpeedMul, Velocity};
use crate::resources::PlayBounds;
use crate::systems::enemy::EnemyStats;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;
use std::f32::consts::TAU;

// ── Stats ─────────────────────────────────────────────────────────────────────

/// Canonical stats for the Guardian.
///
/// Mapping from JS:
///   • health 12 → kept as-is
///   • size 48  → radius 24 (half-size, consistent with other enemies)
///   • speed 1.25 px/frame @ 60 fps → ~75 world-units/s; bumped slightly to 80
///     so it reads as threatening at the larger world scale
///   • fire_cooldown ~2.2 s (JS `cooldown.min` 2250 ms; slow spread cadence)
pub fn stats() -> EnemyStats {
    EnemyStats {
        health: 12.0,
        radius: 24.0,
        speed: 80.0,
        fire_cooldown: Some(2.2),
    }
}

// ── Shape ─────────────────────────────────────────────────────────────────────

/// Guardian silhouette: an armored emerald fortress.
///
/// Ported from `drawEnemyGuardianShape` (render/shapes.js). The JS shape is
/// authored in Canvas space (+Y down, centered on origin). All Y coordinates
/// are negated to convert to Bevy's +Y-up space. Gradient fills, the dashed
/// shield ring, and the muzzle-glow circle are omitted (lyon is path-only);
/// the structural geometry is faithfully reproduced:
///   1. Central alternating-radius hexagon hull (6 vertices, even/odd radii)
///   2. Six facet panel triangles fanning from origin
///   3. Swept battle wings (primary + secondary rear blade) on both sides
///   4. Forward cannon barrel (rectangle approximated as a thin parallelogram)
///
/// The HDR-emissive green stroke values are tuned to bloom via the camera.
pub fn shape() -> Shape {
    let s = stats().radius * 0.8; // JS uses `size = opts.radius * 0.8`

    // ── Central hexagonal hull ────────────────────────────────────────────
    // Even vertices: outer r = s*0.68; odd vertices: inner r = s*0.58
    let hull_pts: Vec<Vec2> = (0..6)
        .map(|i| {
            let a = (i as f32 / 6.0) * TAU;
            let r = s * if i % 2 == 0 { 0.68 } else { 0.58 };
            Vec2::new(a.cos() * r, a.sin() * r) // Y already correct (radially symmetric)
        })
        .collect();

    let mut path = ShapePath::new().move_to(hull_pts[0]);
    for &p in &hull_pts[1..] {
        path = path.line_to(p);
    }
    let hull = path.close();

    let shape_hull = ShapeBuilder::with(&hull)
        .fill(Color::linear_rgb(0.0, 0.01, 0.03))   // #001a08 near-black green body
        .stroke((Color::linear_rgb(0.5, 8.0, 1.5), 2.5)) // emissive #00ff66 edge → bloom
        .build();

    // We cannot return multiple shapes from one function (the parent spawns
    // one entity per call), so we compose the full silhouette into a single
    // ShapePath by appending every sub-path.  Lyon treats each `move_to` as a
    // new sub-path; they all share the same fill/stroke when built together.
    //
    // Composed order: hull → facet panels → right wing → left wing → cannon
    let mut full = ShapePath::new();

    // Re-add hull
    full = full.move_to(hull_pts[0]);
    for &p in &hull_pts[1..] {
        full = full.line_to(p);
    }
    full = full.close();

    // ── Facet panel triangles ─────────────────────────────────────────────
    for i in 0..6usize {
        let a1 = (i as f32 / 6.0) * TAU;
        let a2 = ((i + 1) as f32 / 6.0) * TAU;
        let r1 = s * if i % 2 == 0 { 0.68 } else { 0.58 };
        let r2 = s * if (i + 1) % 2 == 0 { 0.68 } else { 0.58 };
        let p1 = Vec2::new(a1.cos() * r1, a1.sin() * r1);
        let p2 = Vec2::new(a2.cos() * r2, a2.sin() * r2);
        full = full.move_to(Vec2::ZERO);
        full = full.line_to(p1);
        full = full.line_to(p2);
        full = full.close();
    }

    // ── Swept battle wings (mirrored on ±Y) ──────────────────────────────
    // JS canvas: `side` is ±1 applied to the Y axis (Canvas +Y down = Bevy −Y).
    // Negate JS Y coords: Canvas `side * size * K` → Bevy `−side * s * K`
    // because Canvas +Y-down vs Bevy +Y-up. We enumerate both sides.
    for &side in &[1.0_f32, -1.0_f32] {
        // Primary swept wing (5 vertices)
        //   JS: (s*0.25, 0), (s*1.5, side*s*0.3), (s*1.25, side*s*0.9),
        //       (-s*0.5, side*s*1.1), (-s*0.7, side*s*0.35)
        let w = [
            Vec2::new(s * 0.25, 0.0),
            Vec2::new(s * 1.5,  -side * s * 0.3),
            Vec2::new(s * 1.25, -side * s * 0.9),
            Vec2::new(-s * 0.5, -side * s * 1.1),
            Vec2::new(-s * 0.7, -side * s * 0.35),
        ];
        full = full.move_to(w[0]);
        for &p in &w[1..] { full = full.line_to(p); }
        full = full.close();

        // Secondary rear blade (3 vertices)
        //   JS: (-s*0.4, side*s*0.2), (-s*1.2, side*s*0.85), (-s*0.95, side*s*0.38)
        let b = [
            Vec2::new(-s * 0.4,  -side * s * 0.2),
            Vec2::new(-s * 1.2,  -side * s * 0.85),
            Vec2::new(-s * 0.95, -side * s * 0.38),
        ];
        full = full.move_to(b[0]);
        full = full.line_to(b[1]);
        full = full.line_to(b[2]);
        full = full.close();
    }

    // ── Forward cannon barrel (rect as thin quad) ─────────────────────────
    // JS: ctx.rect(s*0.55, -s*0.1, s*0.7, s*0.2) in Canvas space
    // In Bevy +Y-up: x same, Y negated → rect from (s*0.55, -s*0.1) to
    // (s*0.55 + s*0.7, s*0.1) — the rect is symmetric around Y=0 so no flip.
    let cx = s * 0.55;
    let cy = s * 0.10; // half-height
    let cw = s * 0.70; // full width
    let cannon = [
        Vec2::new(cx,      -cy),
        Vec2::new(cx + cw, -cy),
        Vec2::new(cx + cw,  cy),
        Vec2::new(cx,       cy),
    ];
    full = full.move_to(cannon[0]);
    for &p in &cannon[1..] { full = full.line_to(p); }
    full = full.close();

    let _ = shape_hull; // single-path version supersedes the split attempt above

    ShapeBuilder::with(&full)
        .fill(Color::linear_rgb(0.0, 0.02, 0.005))    // dark emerald body
        .stroke((Color::linear_rgb(0.4, 6.0, 1.2), 1.8)) // emissive green edge
        .build()
}

// ── AI (FixedUpdate) ──────────────────────────────────────────────────────────

/// Guardian square-patrol AI.
///
/// Implements the JS `squareMovement` / `squareBurstState` pattern, simplified
/// for the ECS model (no `window.innerWidth`, no per-enemy mutable heap state
/// beyond `AiState`):
///
/// Two pieces of state are packed into `AiState`:
///   • `phase`  — encodes the current FSM + elapsed leg-timer as a float:
///                positive values = bursting (seconds remaining on this leg)
///                zero / negative = waiting (abs value = wait-time remaining)
///   • `wander` — the cardinal direction unit vector for the current burst leg
///
/// Burst distance is fixed at ~110 world-units per leg (≈ screen/6 at 720 p).
/// Wait between legs: 2.0 s.  Burst duration cap: 1.2 s (mirrors JS values).
/// Direction sequence: random cardinal (±X or ±Y) each leg.
pub fn ai(
    time: Res<Time>,
    bounds: Res<PlayBounds>,
    _player: Query<&Transform, With<Ship>>,
    mut q: Query<(&Enemy, &mut AiState, &mut Velocity, &Transform, Option<&SpeedMul>)>,
) {
    let dt = time.delta_secs();
    let spd = stats().speed;

    // Burst travel distance per leg (world units).
    const BURST_DIST: f32 = 110.0;
    // How long one burst leg can last before we force a stop (safety cap).
    const BURST_DUR: f32 = 1.2;
    // Wait between bursts (seconds).
    const WAIT_DUR: f32 = 2.0;

    for (enemy, mut ai, mut vel, tf, sm) in &mut q {
        let spd = spd * sm.map_or(1.0, |s| s.0);
        if enemy.kind != EnemyKind::Guardian {
            continue;
        }

        // Bounce off play-area edges (keep Guardian inside the field).
        if tf.translation.x.abs() > bounds.half.x {
            if ai.wander.x.signum() == tf.translation.x.signum() {
                ai.wander.x = -ai.wander.x;
            }
        }
        if tf.translation.y.abs() > bounds.half.y {
            if ai.wander.y.signum() == tf.translation.y.signum() {
                ai.wander.y = -ai.wander.y;
            }
        }

        // `phase > 0`  → bursting: `phase` counts down seconds remaining on leg
        // `phase <= 0` → waiting:  `phase` counts up toward 0 (starts at -WAIT_DUR)
        if ai.phase > 0.0 {
            // ── Bursting phase ──────────────────────────────────────────
            ai.phase -= dt;
            vel.0 = ai.wander * spd;

            if ai.phase <= 0.0 {
                // Burst complete → enter wait
                vel.0 = Vec2::ZERO;
                ai.phase = -WAIT_DUR;
            }
        } else {
            // ── Waiting phase ───────────────────────────────────────────
            // Apply friction (JS: vel *= 0.85 each frame @ 60 fps)
            vel.0 *= 0.85_f32.powf(dt * 60.0);
            ai.phase += dt;

            if ai.phase >= 0.0 {
                // Choose a random cardinal direction for the next leg.
                // We use `phase` as a cheap pseudo-random seed since we have
                // no rng resource here; the pattern still varies per-entity
                // because each enemy spawns with a distinct `wander` seed and
                // the phase drift accumulates independently.
                let choices = [
                    Vec2::new(1.0, 0.0),
                    Vec2::new(-1.0, 0.0),
                    Vec2::new(0.0, 1.0),
                    Vec2::new(0.0, -1.0),
                ];
                // Deterministic but varied: pick index from wander magnitude seed.
                let seed = (tf.translation.x * 3.7 + tf.translation.y * 1.3).abs() as usize;
                // Rotate through choices based on how many legs have elapsed
                // (we track this implicitly via wander.x/y parity).
                let current_idx = match (ai.wander.x as i32, ai.wander.y as i32) {
                    (1, 0)  => 0,
                    (-1, 0) => 1,
                    (0, 1)  => 2,
                    _       => 3,
                };
                // Pick the next direction (not the same axis as current leg to
                // enforce square turns; alternate X/Y for a grid-walk feel).
                let next_idx = if current_idx < 2 {
                    // Was horizontal → go vertical
                    2 + (seed % 2)
                } else {
                    // Was vertical → go horizontal
                    seed % 2
                };
                ai.wander = choices[next_idx];

                // Burst duration proportional to BURST_DIST (capped at BURST_DUR).
                let burst_secs = (BURST_DIST / spd).min(BURST_DUR);
                ai.phase = burst_secs;
                vel.0 = ai.wander * spd;
            }
        }
    }
}
