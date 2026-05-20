//! Drop system: spawn gold and point orbs on enemy death, attract them to the
//! player, and collect them on overlap. Orbs drift via the shared `integrate`
//! system (Velocity + Transform) and expire via the shared `tick_lifetime`
//! system (Lifetime). Both gold and point orbs use HDR-emissive lyon diamonds
//! so Bloom turns them into glowing gems without any per-sprite texture.

use crate::components::*;
use crate::messages::Death;
use crate::resources::Score;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

// ─── Component ───────────────────────────────────────────────────────────────

/// Marks an orb pickup. Carries the economy payload applied on collection.
#[derive(Component, Debug, Clone, Copy)]
pub struct Orb {
    pub gold: u64,
    pub points: u64,
}

// ─── Shape builders ──────────────────────────────────────────────────────────

/// A small 4-pointed diamond (rhombus) used for both orb kinds.
///
/// `gold_orb = true`  → HDR warm gold  `linear_rgb(9.0, 7.0, 1.0)`
/// `gold_orb = false` → HDR cool cyan  `linear_rgb(1.0, 7.0, 9.0)`
///
/// The diamond is axis-aligned: tips at ±`R` on Y, ±`R*0.55` on X, giving a
/// slightly tall gem silhouette. Bevy 2D is +Y up, which matches our intent.
pub fn shape_orb(gold_orb: bool) -> Shape {
    const R: f32 = 5.0;
    // Top → right → bottom → left  (axis-aligned diamond)
    let pts = [
        Vec2::new(0.0, R),          // top
        Vec2::new(R * 0.55, 0.0),   // right
        Vec2::new(0.0, -R),         // bottom
        Vec2::new(-R * 0.55, 0.0),  // left
    ];

    let path = ShapePath::new()
        .move_to(pts[0])
        .line_to(pts[1])
        .line_to(pts[2])
        .line_to(pts[3])
        .close();

    let color = if gold_orb {
        Color::linear_rgb(9.0, 7.0, 1.0) // HDR warm gold
    } else {
        Color::linear_rgb(1.0, 7.0, 9.0) // HDR cool cyan
    };

    ShapeBuilder::with(&path)
        .fill(color)
        .stroke((color, 1.5))
        .build()
}

// ─── Spawn drops ─────────────────────────────────────────────────────────────

/// Deterministic drift seed from an entity's raw index. Returns a Vec2 in
/// roughly `[-1, 1]²` without any runtime RNG dependency.
#[inline]
fn drift_from_index(index: u32) -> Vec2 {
    // Simple integer hash (Wang hash) → two floats in [-1, 1].
    let h1 = index.wrapping_mul(2_654_435_761).wrapping_add(0x9e37_79b9);
    let h2 = h1.wrapping_mul(2_246_822_519).wrapping_add(0x27d4_eb2f);
    let fx = ((h1 >> 8) & 0xFFFF) as f32 / 32767.5 - 1.0; // [-1, 1]
    let fy = ((h2 >> 8) & 0xFFFF) as f32 / 32767.5 - 1.0;
    Vec2::new(fx, fy)
}

/// Spawn one or two orbs at the position of every `Death` message.
///
/// Parity of `death.entity.index()` selects gold vs point orb as the primary
/// drop. Every 3rd kill (index divisible by 3) drops *both* a gold and a point
/// orb so the field stays lively.
///
/// Each orb gets:
/// - `Collider { radius: 8.0 }` (larger than visual — forgiving pickup zone)
/// - `Velocity` — small deterministic drift so orbs spread out naturally
/// - `Lifetime { seconds: 12.0 }` — uncollected orbs fade before they clutter
pub fn spawn_drops(mut commands: Commands, mut deaths: MessageReader<Death>) {
    for death in deaths.read() {
        let idx = death.entity.to_bits() as u32;
        let drift = drift_from_index(idx) * 28.0; // world-units / second
        let base_z = 0.5_f32;

        let gold_primary = idx % 2 == 0;
        let drop_both = idx % 3 == 0;

        // Primary drop
        if gold_primary || drop_both {
            // gold values: 1–3 (hash into tiny range)
            let gold_amt = 1 + (idx % 3) as u64;
            commands.spawn((
                Orb { gold: gold_amt, points: 0 },
                shape_orb(true),
                Transform::from_xyz(death.position.x, death.position.y, base_z),
                Velocity(drift),
                Collider { radius: 8.0 },
                Lifetime { seconds: 12.0 },
            ));
        }

        if !gold_primary || drop_both {
            // point values: 25, 50, or 100
            let pts_tiers = [25_u64, 50, 100];
            let pts_amt = pts_tiers[((idx / 2) % 3) as usize];
            // Offset slightly so the two orbs don't stack on top of each other.
            let offset = if drop_both { Vec2::new(6.0, 0.0) } else { Vec2::ZERO };
            commands.spawn((
                Orb { gold: 0, points: pts_amt },
                shape_orb(false),
                Transform::from_xyz(
                    death.position.x + offset.x,
                    death.position.y + offset.y,
                    base_z,
                ),
                Velocity(-drift * 0.8), // opposite drift so they spread apart
                Collider { radius: 8.0 },
                Lifetime { seconds: 12.0 },
            ));
        }
    }
}

// ─── Attract orbs ────────────────────────────────────────────────────────────

/// Magnetic pickup feel: steer nearby orbs toward the player.
///
/// Orbs within `ATTRACT_RADIUS` world units accelerate toward the player at a
/// rate proportional to their distance (closer → faster, capped at
/// `MAX_ATTRACT_SPEED`). The `integrate` system then moves them.
pub fn attract_orbs(
    time: Res<Time>,
    player: Query<&Transform, With<Ship>>,
    mut q: Query<(&mut Velocity, &Transform), With<Orb>>,
) {
    const ATTRACT_RADIUS: f32 = 140.0;
    const MAX_ATTRACT_SPEED: f32 = 320.0;
    const ATTRACT_ACCEL: f32 = 480.0; // units / s²

    let Ok(ptf) = player.single() else {
        return; // no player (e.g. GameOver) — skip cleanly
    };
    let player_pos = ptf.translation.truncate();
    let dt = time.delta_secs();

    for (mut vel, orb_tf) in &mut q {
        let orb_pos = orb_tf.translation.truncate();
        let to_player = player_pos - orb_pos;
        let dist = to_player.length();
        if dist > ATTRACT_RADIUS || dist < 0.001 {
            continue;
        }
        // Strength ramps from 0 at the outer edge to full at contact.
        let strength = (1.0 - dist / ATTRACT_RADIUS).powi(2);
        let dir = to_player / dist;
        vel.0 += dir * ATTRACT_ACCEL * strength * dt;

        // Clamp so orbs don't rocket past the player.
        let speed = vel.0.length();
        if speed > MAX_ATTRACT_SPEED {
            vel.0 = vel.0 / speed * MAX_ATTRACT_SPEED;
        }
    }
}

// ─── Collect orbs ────────────────────────────────────────────────────────────

/// Circle-overlap test: player vs every orb. On hit, add the orb's payload to
/// `Score` and despawn it immediately.
pub fn collect_orbs(
    mut commands: Commands,
    mut score: ResMut<Score>,
    player: Query<(&Transform, &Collider), With<Ship>>,
    orbs: Query<(Entity, &Transform, &Collider, &Orb)>,
) {
    let Ok((ptf, pc)) = player.single() else {
        return; // no player — nothing to collect
    };
    let player_pos = ptf.translation.truncate();

    for (orb_e, otf, oc, orb) in &orbs {
        let reach = pc.radius + oc.radius;
        let d2 = player_pos.distance_squared(otf.translation.truncate());
        if d2 <= reach * reach {
            score.gold = score.gold.saturating_add(orb.gold);
            score.points = score.points.saturating_add(orb.points);
            commands.entity(orb_e).despawn();
        }
    }
}
