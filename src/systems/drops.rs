//! Drop system: spawn gold and point orbs on enemy death, attract them to the
//! player, and collect them on overlap. Orbs drift via the shared `integrate`
//! system (Velocity + Transform) and expire via the shared `tick_lifetime`
//! system (Lifetime). Both gold and point orbs use HDR-emissive lyon diamonds
//! so Bloom turns them into glowing gems without any per-sprite texture.

use crate::components::*;
use crate::messages::Death;
use crate::resources::{GameRng, KillStreak, Score};
use crate::systems::enemy;
use crate::systems::wave::Wave;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

// ─── Component ───────────────────────────────────────────────────────────────

/// Marks an orb pickup. Carries the payload applied on collection — gold +
/// points (economy) and `heal` HP (health orbs, spec VI.5).
#[derive(Component, Debug, Clone, Copy)]
pub struct Orb {
    pub gold: u64,
    pub points: u64,
    pub heal: f32,
}

/// Health-orb drop cooldown (spec VI.5) — run-scoped, ticked in `spawn_drops`.
#[derive(Resource, Default)]
pub struct HealthDropTimer {
    pub timer: f32,
}

/// Base health-orb cooldown (seconds; spec VI.5 `max(12000, 25000−…)`, halved
/// at ≤25% HP).
const HEALTH_DROP_CD: f32 = 18.0;

/// Health-orb drop chance given the wave and the player's HP fraction
/// (spec VI.5, simplified): a base rate that climbs with the wave and ramps hard
/// when the player is hurt (`desperationMult`).
pub fn health_drop_rate(wave: u64, hp_pct: f32) -> f32 {
    let base = 0.70 + (wave.max(1) - 1) as f32 * 0.015 + 0.15; // +enemy term
    let desperation = 1.0 + 1.5 * (1.0 - hp_pct).powi(2);
    (base * desperation).min(1.0)
}

/// Heal amount for a wave's health orb (spec VI.5): a roll in
/// `[4+(w−1)*0.6, 8+(w−1)*0.6]`.
pub fn health_orb_heal(wave: u64, roll01: f32) -> f32 {
    let min_heal = 4.0 + (wave.max(1) - 1) as f32 * 0.6;
    (min_heal + roll01 * 4.0).round()
}

// ─── Gold drop tiers (spec VI.4) ─────────────────────────────────────────────

/// A gold drop's value tier — sets its tint + scale (spec VI.4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GoldTier {
    Bronze,
    Silver,
    Gold,
    Platinum,
}

/// Classify a gold value into its drop tier (spec VI.4 thresholds).
pub fn gold_tier(value: u64) -> GoldTier {
    match value {
        v if v >= 200 => GoldTier::Platinum,
        v if v >= 100 => GoldTier::Gold,
        v if v >= 35 => GoldTier::Silver,
        _ => GoldTier::Bronze,
    }
}

impl GoldTier {
    /// Visual scale multiplier (spec VI.4).
    fn scale(self) -> f32 {
        match self {
            GoldTier::Bronze => 0.80,
            GoldTier::Silver => 0.95,
            GoldTier::Gold => 1.00,
            GoldTier::Platinum => 1.20,
        }
    }
    /// HDR-emissive tint (spec VI.4 colors `#cd7f32`/`#dcdcdc`/`#ffd700`/`#88ddff`,
    /// pushed past 1.0 so Bloom gives the gem a glow).
    fn color(self) -> Color {
        match self {
            GoldTier::Bronze => Color::linear_rgb(7.0, 3.5, 1.0),
            GoldTier::Silver => Color::linear_rgb(7.0, 7.0, 7.5),
            GoldTier::Gold => Color::linear_rgb(9.0, 7.0, 1.0),
            GoldTier::Platinum => Color::linear_rgb(4.0, 8.0, 9.0),
        }
    }
}

/// Per-kill gold value (spec VI.5 money budget, simplified to one orb):
/// `avgMoney × goldFind × streakGold × profileBudget`, where
/// `avgMoney = (minMoney+maxMoney)/2`, `minMoney = 10+(wave−1)*3`,
/// `maxMoney = 20+(wave−1)*5`, and `goldFind = 1 + max(0,wave−1)*0.10`.
pub fn gold_value(wave: u64, profile_mul: f32, streak_gold: f32) -> u64 {
    let w = wave.max(1) as f32;
    let min_money = 10.0 + (w - 1.0) * 3.0;
    let max_money = 20.0 + (w - 1.0) * 5.0;
    let avg = (min_money + max_money) * 0.5;
    let gold_find = 1.0 + (w - 1.0) * 0.10;
    (avg * gold_find * streak_gold * profile_mul).round().max(1.0) as u64
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
    orb_shape_colored(if gold_orb {
        Color::linear_rgb(9.0, 7.0, 1.0) // HDR warm gold
    } else {
        Color::linear_rgb(1.0, 7.0, 9.0) // HDR cool cyan
    })
}

/// Build the orb diamond in an arbitrary color (gold orbs tint it per their
/// value tier; point orbs stay cyan). Axis-aligned: tips at ±`R` on Y,
/// ±`R*0.55` on X.
fn orb_shape_colored(color: Color) -> Shape {
    const R: f32 = 5.0;
    let pts = [
        Vec2::new(0.0, R),         // top
        Vec2::new(R * 0.55, 0.0),  // right
        Vec2::new(0.0, -R),        // bottom
        Vec2::new(-R * 0.55, 0.0), // left
    ];
    let path = ShapePath::new()
        .move_to(pts[0])
        .line_to(pts[1])
        .line_to(pts[2])
        .line_to(pts[3])
        .close();
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

/// On every *enemy* `Death`, spawn a gold orb + a point orb (player deaths drop
/// nothing). The gold orb's value scales with the wave, the kill-streak gold
/// multiplier, and the enemy's drop profile (spec VI.5), and its value tier sets
/// its tint + scale (spec VI.4). The point orb carries the enemy's roster points.
///
/// Each orb gets:
/// - `Collider { radius: 8.0 }` (larger than visual — forgiving pickup zone)
/// - `Velocity` — small deterministic drift so orbs spread out naturally
/// - `Lifetime { seconds: 12.0 }` — uncollected orbs fade before they clutter
///
/// Simplified vs. spec VI.5: the money budget is one orb (not split into pixel
/// coins + chunky shapes), and drops are unconditional (the `rate` gate and
/// cooldown-gated health orbs are deferred).
pub fn spawn_drops(
    mut commands: Commands,
    mut deaths: MessageReader<Death>,
    time: Res<Time>,
    wave: Res<Wave>,
    streak: Res<KillStreak>,
    mut hdt: ResMut<HealthDropTimer>,
    mut rng: ResMut<GameRng>,
    player_hp: Query<&Health, With<Ship>>,
) {
    let wave_n = wave.number() as u64;
    let streak_gold = streak.gold_multiplier();
    let hp_pct = player_hp
        .single()
        .ok()
        .map(|h| (h.current / h.max).clamp(0.0, 1.0))
        .unwrap_or(1.0);

    // Health-orb cooldown ticks every frame (drops are only enabled at ≤0).
    hdt.timer = (hdt.timer - time.delta_secs()).max(0.0);

    for death in deaths.read() {
        let Some(kind) = death.kind else {
            continue; // player death — no drops
        };
        let idx = death.entity.to_bits() as u32;
        let drift = drift_from_index(idx) * 28.0; // world-units / second
        let base_z = 0.5_f32;

        // Gold orb — value scales with wave × Gold Find × streak × drop profile;
        // the resulting tier sets its tint + scale. Bosses use the 2.4× budget,
        // mini-bosses the 1.8× miniboss budget (spec VI.5).
        let profile = if death.boss_tier > 0 {
            enemy::drop_budget_mul(kind, true)
        } else if death.mini_boss {
            1.8
        } else {
            enemy::drop_budget_mul(kind, false)
        };
        let value = gold_value(wave_n, profile, streak_gold);
        let tier = gold_tier(value);
        commands.spawn((
            Orb { gold: value, points: 0, heal: 0.0 },
            orb_shape_colored(tier.color()),
            Transform::from_xyz(death.position.x, death.position.y, base_z)
                .with_scale(Vec3::splat(tier.scale())),
            Velocity(drift),
            Collider { radius: 8.0 },
            Lifetime { seconds: 12.0 },
        ));

        // Point orb — per-tier boss points (spec IV.7), 2× for a mini-boss
        // (spec V.6), or the enemy's roster value scaled by the V.4 points curve;
        // offset off the gold orb.
        let points = if death.boss_tier > 0 {
            enemy::boss_points(death.boss_tier)
        } else {
            let base = if death.mini_boss {
                enemy::points(kind) * 2
            } else {
                enemy::points(kind)
            };
            (base as f32 * enemy::difficulty_points_mul(wave_n)).round() as u64
        };
        commands.spawn((
            Orb { gold: 0, points, heal: 0.0 },
            shape_orb(false),
            Transform::from_xyz(death.position.x + 6.0, death.position.y, base_z),
            Velocity(-drift * 0.8), // opposite drift so they spread apart
            Collider { radius: 8.0 },
            Lifetime { seconds: 12.0 },
        ));

        // Health orb — cooldown-gated + desperation-boosted (spec VI.5). One per
        // cooldown window; the cooldown halves when the player is critically hurt.
        if hdt.timer <= 0.0 && rng.next_f32() < health_drop_rate(wave_n, hp_pct) {
            let heal = health_orb_heal(wave_n, rng.next_f32());
            commands.spawn((
                Orb { gold: 0, points: 0, heal },
                orb_shape_colored(Color::linear_rgb(1.0, 9.0, 3.0)), // HDR green
                Transform::from_xyz(death.position.x - 6.0, death.position.y, base_z),
                Velocity(drift * 0.5),
                Collider { radius: 9.0 },
                Lifetime { seconds: 14.0 },
            ));
            hdt.timer = if hp_pct <= 0.25 { HEALTH_DROP_CD * 0.5 } else { HEALTH_DROP_CD };
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
/// `Score`, apply any heal to the player's `Health` (capped at max), and despawn
/// the orb immediately.
pub fn collect_orbs(
    mut commands: Commands,
    mut score: ResMut<Score>,
    mut player: Query<(&Transform, &Collider, &mut Health), With<Ship>>,
    orbs: Query<(Entity, &Transform, &Collider, &Orb)>,
) {
    let Ok((ptf, pc, mut hp)) = player.single_mut() else {
        return; // no player — nothing to collect
    };
    let player_pos = ptf.translation.truncate();

    for (orb_e, otf, oc, orb) in &orbs {
        let reach = pc.radius + oc.radius;
        let d2 = player_pos.distance_squared(otf.translation.truncate());
        if d2 <= reach * reach {
            score.gold = score.gold.saturating_add(orb.gold);
            score.points = score.points.saturating_add(orb.points);
            if orb.heal > 0.0 {
                // Over-fill allowed; `damage::overheal_to_tanks` converts the
                // overflow into tank progress + clamps (spec II.2).
                hp.current += orb.heal;
            }
            commands.entity(orb_e).despawn();
        }
    }
}
