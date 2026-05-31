//! Prismshards (P$) — the rainbow-crystal currency.
//!
//! Every enemy death scatters a **prismatic shard**: a small HDR crystalline gem
//! in a rainbow hue. Shards gently **home toward the Core** (the player's tower —
//! a magnetism/attraction pull) and are *collected* when they reach it, banking
//! their value into the run-scoped [`PrismShards`] balance (shown on the HUD as
//! `P$`). This is the spend currency for upgrades (the shop hookup comes later);
//! for now the loop is collect-on-kill → magnetize → bank.
//!
//! Mirrors the gold-orb idiom in [`crate::systems::drops`] but (a) a distinct,
//! prettier rainbow visual, (b) magnetism targets the **Core**, not a ship, and
//! (c) a separate balance so build-gold (towers) and prismshards (upgrades) stay
//! distinct. Shards drift via the shared `integrate` (`Velocity`) and expire via
//! the shared `tick_lifetimes` (`Lifetime`); `animate_prismshards` only spins +
//! pulses them (Transform), so it never fights the physics for position.

use crate::components::{Collider, Core, Lifetime, Velocity};
use crate::messages::{Death, Pickup};
use crate::render::reaction_fx::{unit_ring, Shockwave};
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;
use std::f32::consts::TAU;

// ─── Currency balance ────────────────────────────────────────────────────────

/// Run-scoped prismshard (P$) balance. Reset to 0 at the start of each run
/// (`reset_prismshards`); spent on upgrades (future). Distinct from `Score.gold`
/// (the tower build currency).
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct PrismShards(pub u64);

/// A collectable prismshard drop; `value` P$ is banked on pickup.
#[derive(Component, Debug, Clone, Copy)]
pub struct PrismShard {
    pub value: u64,
}

/// Idle-animation state: resting scale (so the pulse multiplies it) + a phase
/// offset so a cluster shimmers out of lockstep. Mirrors `drops::OrbGlow`.
#[derive(Component, Debug, Clone, Copy)]
pub struct PrismGlow {
    pub base_scale: f32,
    pub phase: f32,
}

// ─── Tuning ──────────────────────────────────────────────────────────────────

/// Idle spin (rad/sec) + pulse (rate rad/sec, amplitude as ±fraction of scale).
const PRISM_SPIN: f32 = 1.4;
const PRISM_PULSE_RATE: f32 = 4.5;
const PRISM_PULSE_AMP: f32 = 0.18;
/// Cruise speed (world u/s) a shard homes toward the Core at, and how fast its
/// velocity eases onto that cruise vector (per-second lerp factor) — a *gentle*
/// magnetic pull, not a snap.
const CRUISE_SPEED: f32 = 235.0;
const HOMING_LERP: f32 = 2.4;
/// Pickup happens when a shard gets within this fraction of the Core's radius.
const COLLECT_FRAC: f32 = 0.9;
/// How long an (uncollected) shard lingers — long enough to always reach the Core.
const SHARD_LIFE: f32 = 25.0;

/// P$ value for a kill: chunky for bosses, a bit more for mini-bosses, 1 for a
/// regular enemy. Pure, for testing.
pub fn prismshard_value(boss_tier: u8, mini_boss: bool) -> u64 {
    if boss_tier > 0 {
        8 + boss_tier as u64 * 4
    } else if mini_boss {
        4
    } else {
        1
    }
}

/// Gently steer `vel` toward a cruise-speed vector pointed at `center` — the pure
/// magnetism step (extracted so it's testable). Returns the new velocity.
pub fn home_velocity(pos: Vec2, center: Vec2, vel: Vec2, cruise: f32, lerp_t: f32) -> Vec2 {
    let to = center - pos;
    let d = to.length();
    if d < 1.0 {
        return vel;
    }
    let desired = to / d * cruise;
    vel.lerp(desired, lerp_t.clamp(0.0, 1.0))
}

// ─── Shape ───────────────────────────────────────────────────────────────────

/// HDR rainbow color at `hue` (degrees), `gain` past 1.0 so Bloom makes it glow.
fn hdr_hue(hue: f32, gain: f32) -> Color {
    let l = Color::hsl(hue.rem_euclid(360.0), 0.95, 0.6).to_linear();
    Color::linear_rgb(l.red * gain, l.green * gain, l.blue * gain)
}

/// A small crystalline gem silhouette in a rainbow `hue` — a tall faceted shard
/// (fill + a brighter, hue-shifted stroke so each crystal reads with a prismatic
/// edge). A field of these in spread hues is the "prismatic shards of rainbows".
fn prism_shard_shape(hue: f32) -> Shape {
    const R: f32 = 7.0;
    let pts = [
        Vec2::new(0.0, R),           // top point
        Vec2::new(R * 0.5, R * 0.4), // upper-right facet
        Vec2::new(R * 0.46, -R * 0.5),
        Vec2::new(0.0, -R), // bottom point
        Vec2::new(-R * 0.46, -R * 0.5),
        Vec2::new(-R * 0.5, R * 0.4), // upper-left facet
    ];
    let mut path = ShapePath::new().move_to(pts[0]);
    for p in &pts[1..] {
        path = path.line_to(*p);
    }
    ShapeBuilder::with(&path.close())
        .fill(hdr_hue(hue, 3.0))
        .stroke((hdr_hue(hue + 40.0, 6.0), 1.3))
        .build()
}

// ─── Spawn (reads Death) ─────────────────────────────────────────────────────

/// On each *enemy* `Death`, scatter a prismshard at the death position. Hue is
/// spread per-shard (off a `Local` counter) so a field reads as a rainbow; bosses
/// /mini-bosses drop a larger, higher-value crystal. The shard pops outward with
/// a little drift, then `attract_prismshards` reels it toward the Core.
pub fn spawn_prismshards(
    mut commands: Commands,
    mut deaths: MessageReader<Death>,
    mut seed: Local<u32>,
) {
    for d in deaths.read() {
        if d.kind.is_none() {
            continue; // player death drops no currency
        }
        *seed = seed.wrapping_add(1);
        let s = *seed;
        let value = prismshard_value(d.boss_tier, d.mini_boss);
        let hue = (s.wrapping_mul(47) % 360) as f32; // walk the colour wheel
        let phase = (s & 0xFF) as f32 / 255.0 * TAU;
        // Golden-angle pop direction so a cluster spreads instead of stacking.
        let ang = s as f32 * 2.399_963_2;
        let drift = Vec2::new(ang.cos(), ang.sin()) * 45.0;
        let scale = if d.boss_tier > 0 {
            1.8
        } else if d.mini_boss {
            1.4
        } else {
            1.0
        };
        commands.spawn((
            PrismShard { value },
            PrismGlow { base_scale: scale, phase },
            prism_shard_shape(hue),
            Transform::from_xyz(d.position.x, d.position.y, 0.45).with_scale(Vec3::splat(scale)),
            Velocity(drift),
            Collider { radius: 9.0 },
            Lifetime { seconds: SHARD_LIFE },
        ));
    }
}

// ─── Magnetism (toward the Core) ─────────────────────────────────────────────

/// Gently magnetize every shard toward the Core. The pull eases each shard's
/// velocity onto a cruise-speed vector aimed at the Core, so they drift in like
/// filings to a magnet rather than teleporting. `integrate` then moves them.
pub fn attract_prismshards(
    time: Res<Time>,
    core: Query<&Transform, With<Core>>,
    mut q: Query<(&mut Velocity, &Transform), With<PrismShard>>,
) {
    let Ok(ctf) = core.single() else {
        return; // no Core (e.g. not in a run) — leave shards drifting
    };
    let center = ctf.translation.truncate();
    let t = (HOMING_LERP * time.delta_secs()).min(1.0);
    for (mut vel, tf) in &mut q {
        vel.0 = home_velocity(tf.translation.truncate(), center, vel.0, CRUISE_SPEED, t);
    }
}

// ─── Collect (at the Core) ───────────────────────────────────────────────────

/// Bank + despawn every shard that has reached the Core, popping a rainbow
/// pickup sparkle. Emits `Pickup` so the pickup chime plays.
pub fn collect_prismshards(
    mut commands: Commands,
    mut bank: ResMut<PrismShards>,
    mut pickup: MessageWriter<Pickup>,
    core: Query<(&Transform, &Collider), With<Core>>,
    shards: Query<(Entity, &Transform, &PrismShard)>,
) {
    let Ok((ctf, cc)) = core.single() else {
        return;
    };
    let center = ctf.translation.truncate();
    let reach = cc.radius * COLLECT_FRAC;
    let reach2 = reach * reach;
    for (e, tf, shard) in &shards {
        if center.distance_squared(tf.translation.truncate()) <= reach2 {
            bank.0 = bank.0.saturating_add(shard.value);
            pickup.write(Pickup);
            commands.spawn((
                Shockwave { age: 0.0, peak: 26.0 },
                unit_ring(Color::linear_rgb(7.0, 5.0, 9.0)), // bright prismatic violet-white
                Transform::from_translation(tf.translation.truncate().extend(0.35)),
            ));
            commands.entity(e).despawn();
        }
    }
}

// ─── Idle animation ──────────────────────────────────────────────────────────

/// Spin + pulse idle shards so the crystals shimmer and turn. Pure presentation:
/// writes only Transform rotation + scale; `integrate`/`attract_prismshards` own
/// position. Mirrors `drops::animate_orbs`.
pub fn animate_prismshards(time: Res<Time>, mut q: Query<(&PrismGlow, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (g, mut tf) in &mut q {
        tf.rotation = Quat::from_rotation_z(t * PRISM_SPIN + g.phase);
        let pulse = 1.0 + PRISM_PULSE_AMP * (t * PRISM_PULSE_RATE + g.phase).sin();
        tf.scale = Vec3::splat(g.base_scale * pulse);
    }
}

// ─── Reset ───────────────────────────────────────────────────────────────────

/// Zero the prismshard balance at the start of a fresh run.
pub fn reset_prismshards(mut bank: ResMut<PrismShards>) {
    bank.0 = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_scales_boss_then_miniboss_then_normal() {
        assert_eq!(prismshard_value(0, false), 1);
        assert_eq!(prismshard_value(0, true), 4);
        assert!(prismshard_value(1, false) > prismshard_value(0, true));
        assert!(prismshard_value(3, false) > prismshard_value(1, false));
    }

    #[test]
    fn homing_pulls_toward_the_core() {
        // A shard out on +X should gain leftward (−X) velocity toward an origin Core.
        let v = home_velocity(Vec2::new(120.0, 0.0), Vec2::ZERO, Vec2::ZERO, 235.0, 0.5);
        assert!(v.x < 0.0, "homes toward the core on −X, got {v:?}");
        assert!(v.y.abs() < 1e-3, "no lateral drift on-axis");
    }

    #[test]
    fn homing_eases_not_snaps() {
        // With a small lerp factor the velocity only partly approaches the cruise.
        let v = home_velocity(Vec2::new(0.0, 100.0), Vec2::ZERO, Vec2::ZERO, 235.0, 0.25);
        assert!(v.length() < 235.0, "eased onto the cruise, not instant");
        assert!(v.y < 0.0, "moving down toward the core");
    }

    #[test]
    fn homing_holds_at_the_core() {
        // Right on top of the core → unchanged (avoids divide-by-zero jitter).
        let v = home_velocity(Vec2::ZERO, Vec2::ZERO, Vec2::new(5.0, 0.0), 235.0, 0.5);
        assert_eq!(v, Vec2::new(5.0, 0.0));
    }
}
