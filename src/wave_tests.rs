//! Phase 3 scenario tests (headless, per `docs/port-plan.md` §9).
//!
//! Covers four scenarios added in Phase 3:
//!   1. Wave spawner drives enemies onto the world over simulated time.
//!   2. Player bullet kills an enemy → `Score.kills` increments.
//!   3. Player bullet splits a tier-2 asteroid into two tier-1 children.
//!   4. An enemy with a ready `FireCooldown` fires at the player.
//!
//! Pattern mirrors `gate_tests.rs` exactly: bare `App`, `add_message`/
//! `init_resource`/`insert_resource`, manual entity spawning, ad-hoc
//! `Schedule::default()` driven against `app.world_mut()`. No plugins,
//! no rendering, no `app.update()`.

use bevy::prelude::*;
use std::time::Duration;

use crate::components::*;
use crate::messages::{Damage, Death, Fire};
use crate::resources::{PlayBounds, Score};
use crate::states::GameState;
use crate::systems::asteroids::{self, Asteroid};
use crate::systems::collision::bullet_hits_enemy;
use crate::systems::damage::apply_damage;
use crate::systems::enemy::firing::enemy_firing;
use crate::systems::wave::{self, Wave};

// ── shared helpers ────────────────────────────────────────────────────────────

/// Base app with every message channel + resource the Phase-3 systems need.
/// We register everything once here so individual tests can just add whatever
/// subset of systems they care about.
fn test_app() -> App {
    let mut app = App::new();
    app.add_message::<Damage>()
        .add_message::<Death>()
        .add_message::<Fire>()
        .add_message::<crate::messages::Knockback>()
        .init_resource::<Score>()
        .init_resource::<crate::resources::KillStreak>()
        .init_resource::<crate::resources::GameRng>()
        .init_resource::<crate::resources::EnergyMeter>()
        .init_resource::<crate::systems::shop::Upgrades>()
        .insert_resource(NextState::<GameState>::Unchanged);
    app
}

// ── 1. wave_spawns_enemies_over_time ─────────────────────────────────────────

/// Pulse-paced spawning (port spec V.3): pulse 0 fires after the intro delay,
/// then later pulses fire on the 12 s stale-timer fallback (no enemies die in
/// this test, so the ≤2-remaining trigger never hits). Wave 1 = Hunter×3 (P0)
/// then Hunter×2,Wasp×2 (P1), so the count must grow across pulses.
#[test]
fn wave_spawns_enemies_over_time() {
    let mut app = test_app();
    let world = app.world_mut();

    world.insert_resource(Wave::default());
    world.insert_resource(PlayBounds::default());

    let mut time = Time::<()>::default();
    world.insert_resource(time.clone());

    let mut step = Schedule::default();
    step.add_systems(wave::spawn_waves);

    let count = |world: &mut World| -> usize {
        world.query_filtered::<Entity, With<Enemy>>().iter(world).count()
    };

    assert_eq!(count(world), 0, "world should start empty");

    // Step 1 (dt = 1.5 s) clears the 1 s intro delay → pulse 0 (Hunter×3).
    time.advance_by(Duration::from_secs_f32(1.5));
    world.insert_resource(time.clone());
    step.run(world);
    let after_p0 = count(world);
    assert_eq!(after_p0, 3, "pulse 0 of wave 1 spawns Hunter×3 (got {after_p0})");

    // Step 2 (dt = 13 s ≥ 12 s stale) → pulse 1 (Hunter×2 + Wasp×2 = +4).
    time.advance_by(Duration::from_secs_f32(13.0));
    world.insert_resource(time.clone());
    step.run(world);
    let after_p1 = count(world);
    assert_eq!(
        after_p1, 7,
        "pulse 1 adds Hunter×2+Wasp×2 → 7 total (got {after_p1})"
    );
}

/// A boss-tier TITAN spawns with the HP/size overlay from `boss_tier_mul`
/// (port spec IV.7): tier 1 = 4.0× HP, 1.35× size. Wave 3's final pulse holds
/// a tier-1 boss TITAN; here we spawn one directly and check the overlay.
#[test]
fn boss_titan_gets_tier_overlay() {
    use crate::systems::enemy;

    let mut app = test_app();
    let world = app.world_mut();

    let mut step = Schedule::default();
    step.add_systems(|mut commands: Commands| {
        enemy::spawn_tiered(&mut commands, EnemyKind::Titan, Vec2::ZERO, 1);
    });
    step.run(world);

    let mut q = world.query_filtered::<(&Health, &Collider, &Boss), With<Enemy>>();
    let (hp, col, boss) = q.iter(world).next().expect("a boss titan should exist");
    // Base TITAN HP is 20 in the JS roster; tier-1 overlay = ×4.0 → 80.
    assert_eq!(boss.tier, 1, "tier recorded");
    assert!((hp.max - 80.0).abs() < 0.01, "tier-1 HP = base×4.0 (got {})", hp.max);
    // Collider grows by the 1.35× size multiplier.
    let base_radius = col.radius / 1.35;
    assert!(base_radius > 1.0, "collider scaled by tier size mult (got {})", col.radius);
}

// ── 2. player_bullet_kills_enemy_increments_score_and_emits_death ────────────

/// A zero-HP-after-hit enemy at the bullet's position: collision should despawn
/// both, add 1 kill to `Score`, and write a `Death` message.
#[test]
fn player_bullet_kills_enemy_increments_score_and_emits_death() {
    let mut app = test_app();
    let world = app.world_mut();

    // Enemy with exactly 1 HP so any damage kills it.
    let enemy = world
        .spawn((
            Enemy {
                kind: EnemyKind::Drifter,
            },
            Health::new(1.0),
            Collider { radius: 18.0 },
            Faction::Enemy,
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();

    // Bullet at the same position, 10 damage — overkill, deterministic.
    let bullet = world
        .spawn((
            Bullet {
                kind: BulletKind::Player,
                damage: 10.0,
                pierce: 0,
            },
            Collider { radius: 3.0 },
            Faction::Player,
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();

    let mut step = Schedule::default();
    step.add_systems((bullet_hits_enemy, apply_damage).chain());
    step.run(world);

    assert!(
        world.get::<Health>(enemy).is_none(),
        "the enemy should be despawned after a lethal bullet hit"
    );
    assert!(
        world.get::<Bullet>(bullet).is_none(),
        "the bullet should be despawned on impact"
    );
    assert_eq!(
        world.resource::<Score>().kills,
        1,
        "a killed enemy should increment Score::kills"
    );
}

// ── 3. asteroid_splits_when_shot ─────────────────────────────────────────────

/// Shooting a tier-2 asteroid despawns it and spawns exactly two tier-1
/// children. We manually spawn the asteroid without calling `asteroids::shape`
/// so no lyon/render infrastructure is required.
#[test]
fn asteroid_splits_when_shot() {
    let mut app = test_app();
    let world = app.world_mut();

    // Tier-2 asteroid at origin with a downward velocity.
    let _asteroid = world
        .spawn((
            Asteroid { tier: 2 },
            Collider { radius: 36.0 },
            Velocity(Vec2::new(0.0, -50.0)),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();

    // Player bullet overlapping the asteroid exactly.
    let _bullet = world
        .spawn((
            Bullet {
                kind: BulletKind::Player,
                damage: 10.0,
                pierce: 0,
            },
            Collider { radius: 3.0 },
            Faction::Player,
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();

    let mut step = Schedule::default();
    step.add_systems(asteroids::asteroid_hits);
    step.run(world);

    // Original tier-2 gone; two tier-1 children present.
    let asteroid_count = world
        .query::<&Asteroid>()
        .iter(world)
        .count();
    assert_eq!(
        asteroid_count,
        2,
        "a tier-2 asteroid shot by a player bullet should split into exactly 2 tier-1 fragments"
    );

    let all_tier_1 = world
        .query::<&Asteroid>()
        .iter(world)
        .all(|a| a.tier == 1);
    assert!(all_tier_1, "all surviving asteroids should be tier 1");

    // Bullet must be consumed.
    let bullet_count = world
        .query_filtered::<Entity, With<Bullet>>()
        .iter(world)
        .count();
    assert_eq!(bullet_count, 0, "the bullet should be despawned on asteroid impact");
}

// ── 4. firing_enemy_emits_fire ────────────────────────────────────────────────

/// An enemy whose `FireCooldown.timer == 0` should emit at least one `Fire`
/// message the moment `enemy_firing` runs (timer already expired).
#[test]
fn firing_enemy_emits_fire() {
    let mut app = test_app();
    let world = app.world_mut();

    // Player ship — `enemy_firing` returns early if no player exists.
    world.spawn((
        Ship::default(),
        Transform::from_xyz(0.0, -100.0, 0.0), // separated so aim_dir != ZERO
    ));

    // Hunter at origin with an already-expired fire timer (timer == 0).
    world.spawn((
        Enemy {
            kind: EnemyKind::Hunter,
        },
        FireCooldown {
            cooldown: 1.0,
            timer: 0.0, // ready to fire immediately
        },
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));

    // A tiny `dt` so the timer tick does not accidentally re-arm it before
    // the cooldown-expiry branch runs.
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16)); // ~1 frame
    world.insert_resource(time);

    // We need a system that checks the Fire message. The cleanest headless
    // approach is a counting system run in the same schedule *after* enemy_firing.
    // We accumulate into a local (single-element vec captured by value won't
    // work across schedule boundary), so we use a temporary resource instead.
    #[derive(Resource, Default)]
    struct FireCount(u32);
    world.insert_resource(FireCount::default());

    // Tally system: reads every `Fire` message written by `enemy_firing`.
    fn count_fires(mut reader: MessageReader<Fire>, mut count: ResMut<FireCount>) {
        for _ in reader.read() {
            count.0 += 1;
        }
    }

    let mut step = Schedule::default();
    step.add_systems((enemy_firing, count_fires).chain());
    step.run(world);

    let fired = world.resource::<FireCount>().0;
    assert!(
        fired >= 1,
        "an enemy with a ready FireCooldown should emit at least one Fire message"
    );
}

// ── 5. kill_streak_multiplier_tiers ──────────────────────────────────────────

/// The streak multiplier (spec III.6) is 1.0 below 10 kills, steps to 1.25 at
/// 10, 2.00 at 60, caps at 3.00 by 200 — and only applies while the buff window
/// is open; breaking the streak drops it to 1.0.
#[test]
fn kill_streak_multiplier_tiers() {
    use crate::resources::{streak_mult, KillStreak, STREAK_BUFF_SECS};

    assert_eq!(streak_mult(0), 1.0);
    assert_eq!(streak_mult(9), 1.0);
    assert_eq!(streak_mult(10), 1.25);
    assert_eq!(streak_mult(59), 1.85);
    assert_eq!(streak_mult(60), 2.00);
    assert_eq!(streak_mult(250), 3.00, "caps at the 200-kill tier");

    let mut s = KillStreak::default();
    assert_eq!(s.multiplier(), 1.0, "no kills → no buff");
    for _ in 0..10 {
        s.on_kill();
    }
    assert_eq!(s.timer, STREAK_BUFF_SECS, "each kill refreshes the window");
    assert_eq!(s.multiplier(), 1.25, "10 kills inside the window → 1.25×");

    s.timer = 0.0; // window lapsed
    assert_eq!(s.multiplier(), 1.0, "lapsed window → multiplier off (count kept)");
    assert_eq!(s.kills, 10, "…but the count persists until damage");

    s.break_streak();
    assert_eq!(s.kills, 0);
    assert_eq!(s.multiplier(), 1.0);
}

// ── 6. crit_roll_bounds ──────────────────────────────────────────────────────

/// `roll_crit` returns exactly 1.0 (normal) or a 2.0–3.0× crit, at roughly the
/// 8% base rate over many rolls (spec III.6). Seeded RNG → deterministic.
#[test]
fn crit_roll_bounds() {
    use crate::resources::{roll_crit, GameRng};

    let mut rng = GameRng::default();
    let mut crits = 0;
    let n = 20_000;
    for _ in 0..n {
        let m = roll_crit(&mut rng);
        assert!(
            m == 1.0 || (2.0..=3.0).contains(&m),
            "crit multiplier must be 1.0 or in [2,3], got {m}"
        );
        if m > 1.0 {
            crits += 1;
        }
    }
    let rate = crits as f32 / n as f32;
    assert!(
        (0.05..0.11).contains(&rate),
        "crit rate should sit near the 8% base (got {rate})"
    );
}

// ── 7. energy_meter_gain_and_spend ───────────────────────────────────────────

/// The power-weapon energy meter charges +4 per hit (capped at 100) and a
/// power-weapon fire only succeeds when its energy cost is affordable (spec III.3).
#[test]
fn energy_meter_gain_and_spend() {
    use crate::resources::{EnergyMeter, ENERGY_MAX, ENERGY_PER_HIT};

    let mut e = EnergyMeter::default();
    assert_eq!(e.current, 0.0);
    assert!(!e.try_spend(20.0), "can't fire with no energy");

    // 25 hits → would be 100, capped at the max.
    for _ in 0..25 {
        e.gain(ENERGY_PER_HIT);
    }
    assert_eq!(e.current, ENERGY_MAX, "energy caps at {ENERGY_MAX}");

    assert!(e.try_spend(55.0), "Missile Salvo (55) affordable at full");
    assert!((e.current - 45.0).abs() < 0.01, "45 energy left after a 55 spend");
    assert!(!e.try_spend(60.0), "Lance Beam (60) not affordable with 45");
}

// ── 8. nova_ring_band_hit ─────────────────────────────────────────────────────

/// A Nova ring damages an enemy only while its expanding front (a 30 px band
/// around the current radius) overlaps the enemy's disc (spec III.3).
#[test]
fn nova_ring_band_hit() {
    use crate::systems::power_weapon::nova_band_hits;

    let center = Vec2::ZERO;
    let enemy = Vec2::new(100.0, 0.0);
    let er = 16.0;
    let band = 30.0;

    // Front far inside the enemy — not yet reached.
    assert!(!nova_band_hits(center, 40.0, band, enemy, er), "front at r=40 hasn't reached d=100");
    // Front sweeping across the enemy.
    assert!(nova_band_hits(center, 100.0, band, enemy, er), "front at r=100 overlaps d=100");
    assert!(nova_band_hits(center, 90.0, band, enemy, er), "band reaches the near edge");
    // Front well past the enemy.
    assert!(!nova_band_hits(center, 200.0, band, enemy, er), "front at r=200 has passed d=100");
}

// ── 9. mine_detonates_on_nearby_enemy ────────────────────────────────────────

/// An armed mine detonates when an enemy is inside its trigger radius, emitting
/// `Damage` to enemies in the blast radius and despawning itself (spec III.3).
#[test]
fn mine_detonates_on_nearby_enemy() {
    use crate::systems::power_weapon::{lay_mine, update_mines, Mine};

    let mut app = test_app();
    let world = app.world_mut();

    // Enemy 40 px from the mine (inside the 60 px trigger).
    world.spawn((
        Enemy { kind: EnemyKind::Hunter },
        Health::new(5.0),
        Collider { radius: 16.0 },
        Faction::Enemy,
        Transform::from_xyz(40.0, 0.0, 0.0),
    ));
    // Lay a mine at the origin via Commands (deferred), applied by a setup step.
    let mut setup = Schedule::default();
    setup.add_systems(|mut c: Commands| lay_mine(&mut c, Vec2::ZERO));
    setup.run(world);

    // dt past the 0.6 s arm delay → armed + triggered this step.
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(1.0));
    world.insert_resource(time);

    // Count emitted Damage via a tally system (same pattern as the firing test).
    #[derive(Resource, Default)]
    struct DmgCount(u32);
    world.insert_resource(DmgCount::default());
    fn count_dmg(mut reader: MessageReader<Damage>, mut count: ResMut<DmgCount>) {
        for _ in reader.read() {
            count.0 += 1;
        }
    }

    let mut step = Schedule::default();
    step.add_systems((update_mines, count_dmg).chain());
    step.run(world);

    let damages = world.resource::<DmgCount>().0;
    assert!(damages >= 1, "mine should emit blast Damage (got {damages})");
    let mines_left = world.query::<&Mine>().iter(world).count();
    assert_eq!(mines_left, 0, "the mine should despawn after detonating");
}

// ── 10. beam_ray_first_hit ────────────────────────────────────────────────────

/// `beam_ray_hit_dist` (the Lance ray test) reports the forward distance to an
/// enemy the beam strikes — within `half_width + radius` of the axis — and
/// `None` for enemies behind the nose, past the range, or off to the side.
#[test]
fn beam_ray_first_hit() {
    use crate::systems::power_weapon::beam_ray_hit_dist;

    let origin = Vec2::ZERO;
    let dir = Vec2::new(0.0, 1.0); // straight up
    let range = 360.0;
    let half_w = 3.0;

    // Dead ahead, in range → hit at the forward distance.
    assert_eq!(
        beam_ray_hit_dist(origin, dir, range, half_w, Vec2::new(0.0, 100.0), 16.0),
        Some(100.0),
        "on-axis enemy reports its forward distance"
    );
    // Behind the nose → miss.
    assert_eq!(
        beam_ray_hit_dist(origin, dir, range, half_w, Vec2::new(0.0, -50.0), 16.0),
        None
    );
    // Past the 360 px reach → miss.
    assert_eq!(
        beam_ray_hit_dist(origin, dir, range, half_w, Vec2::new(0.0, 400.0), 16.0),
        None
    );
    // Off-axis beyond half_w + radius (3 + 16 = 19) → miss.
    assert_eq!(
        beam_ray_hit_dist(origin, dir, range, half_w, Vec2::new(40.0, 100.0), 16.0),
        None
    );
    // Just inside the perpendicular reach → hit.
    assert!(beam_ray_hit_dist(origin, dir, range, half_w, Vec2::new(18.0, 100.0), 16.0).is_some());
}

// ── 11. lance_beam_damages_target_and_expires ─────────────────────────────────

/// A live Lance beam ticks `dps × dt` damage into the enemy in front of the ship
/// each step, then despawns once its 3 s life lapses (spec III.3).
#[test]
fn lance_beam_damages_target_and_expires() {
    use crate::systems::power_weapon::{spawn_beam, update_beams, Beam, BeamKind};

    let mut app = test_app();
    let world = app.world_mut();

    // Player ship at origin, facing +Y (identity rotation → fwd = +Y).
    world.spawn((
        Ship::default(),
        Intent::default(),
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));

    // Enemy 120 px straight ahead (on the +Y beam axis) with generous HP.
    let enemy = world
        .spawn((
            Enemy { kind: EnemyKind::Titan },
            Health::new(50.0),
            Collider { radius: 20.0 },
            Faction::Enemy,
            Transform::from_xyz(0.0, 120.0, 0.0),
        ))
        .id();

    // Lay a Lance beam at the nose (deferred spawn, applied by a setup step).
    let mut setup = Schedule::default();
    setup.add_systems(|mut c: Commands| spawn_beam(&mut c, BeamKind::Lance, Vec2::new(0.0, 22.0)));
    setup.run(world);

    let mut step = Schedule::default();
    step.add_systems((update_beams, apply_damage).chain());

    // Step 1 (dt = 0.5 s): the beam ticks damage into the enemy in front.
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(0.5));
    world.insert_resource(time.clone());
    step.run(world);

    let hp = world.get::<Health>(enemy).expect("enemy alive").current;
    assert!(hp < 50.0, "lance beam should damage the enemy in front (hp now {hp})");
    assert_eq!(
        world.query::<&Beam>().iter(world).count(),
        1,
        "beam still live mid-duration"
    );

    // Step 2 (dt = 3.0 s): total life now exceeds BEAM_DURATION → beam despawns.
    time.advance_by(Duration::from_secs_f32(3.0));
    world.insert_resource(time);
    step.run(world);

    assert_eq!(
        world.query::<&Beam>().iter(world).count(),
        0,
        "beam despawns at end of life"
    );
}

// ── 12. gold_value_scales_and_tiers ───────────────────────────────────────────

/// Gold value scales with wave × Gold Find × streak × drop profile (spec VI.5)
/// and classifies into the bronze/silver/gold/platinum tiers (spec VI.4).
#[test]
fn gold_value_scales_and_tiers() {
    use crate::systems::drops::{gold_tier, gold_value, GoldTier};

    // Wave 1, standard profile, no streak: avg (10+20)/2 = 15, goldFind 1.0 → 15.
    let v1 = gold_value(1, 1.0, 1.0);
    assert_eq!(v1, 15);
    assert_eq!(gold_tier(v1), GoldTier::Bronze);

    // Tier thresholds.
    assert_eq!(gold_tier(34), GoldTier::Bronze);
    assert_eq!(gold_tier(35), GoldTier::Silver);
    assert_eq!(gold_tier(99), GoldTier::Silver);
    assert_eq!(gold_tier(100), GoldTier::Gold);
    assert_eq!(gold_tier(199), GoldTier::Gold);
    assert_eq!(gold_tier(200), GoldTier::Platinum);

    // Wave 10 boss with a full streak gold mult pushes value into platinum:
    // min 37, max 65, avg 51; goldFind 1.9 → 51×1.9×1.5×2.4 ≈ 349.
    let v_boss = gold_value(10, 2.4, 1.5);
    assert!(v_boss > v1, "later wave + boss profile yields more gold");
    assert_eq!(gold_tier(v_boss), GoldTier::Platinum);
}

// ── 13. upgrade_cost_formula ──────────────────────────────────────────────────

/// Shop cost = `base × 13 × 1.6^owned`, rounded to 500, floored at 500 (spec VIII.4).
#[test]
fn upgrade_cost_formula() {
    use crate::systems::shop::upgrade_cost;

    // base 1200, owned 0: 1200×13 = 15600 → nearest 500 = 15500.
    assert_eq!(upgrade_cost(1200, 0), 15500);
    // owned 1: ×1.6 = 24960 → nearest 500 = 25000.
    assert_eq!(upgrade_cost(1200, 1), 25000);
    // 500 floor for a tiny base.
    assert_eq!(upgrade_cost(1, 0), 500);
    // strictly increasing per owned stack.
    assert!(upgrade_cost(1500, 2) > upgrade_cost(1500, 1));
}

// ── 14. spawn_pos_edges ────────────────────────────────────────────────────────

/// Wave spawns land just outside exactly one edge (spec V.5) and stay within the
/// 250 px offscreen-cull margin so they aren't immediately reaped.
#[test]
fn spawn_pos_edges() {
    use crate::systems::wave::spawn_pos;

    let bounds = PlayBounds::default(); // half = (640, 360)
    for seq in 1..=8u32 {
        let p = spawn_pos(seq, &bounds);
        let outside_x = p.x.abs() > bounds.half.x;
        let outside_y = p.y.abs() > bounds.half.y;
        assert!(
            outside_x ^ outside_y,
            "seq {seq}: should be outside exactly one edge (got {p:?})"
        );
        assert!(
            p.x.abs() <= bounds.half.x + 250.0 && p.y.abs() <= bounds.half.y + 250.0,
            "seq {seq}: should stay within the cull margin (got {p:?})"
        );
    }
}

// ── 15. health_orb_rate_and_heal ──────────────────────────────────────────────

/// The health-orb drop rate stays in [0,1] and ramps up when the player is hurt
/// (desperation), and the heal amount scales with the wave (spec VI.5).
#[test]
fn health_orb_rate_and_heal() {
    use crate::systems::drops::{health_drop_rate, health_orb_heal};

    let full = health_drop_rate(1, 1.0);
    let hurt = health_drop_rate(1, 0.1);
    assert!((0.0..=1.0).contains(&full));
    assert!(hurt >= full && hurt <= 1.0, "low HP raises the rate (cap 1.0)");

    // Wave 1 heal rolls in [4, 8]; higher waves heal more.
    assert_eq!(health_orb_heal(1, 0.0), 4.0);
    assert_eq!(health_orb_heal(1, 1.0), 8.0);
    assert!(health_orb_heal(10, 0.5) > health_orb_heal(1, 0.5));
}

// ── 16. boss_rages_at_one_third_hp ────────────────────────────────────────────

/// A boss at ≤33% HP rages once (spec IV.7): gains the `Raged` marker + an
/// invuln window, has its fire cooldown cut ×0.66, and fires a 16-bullet tantrum.
#[test]
fn boss_rages_at_one_third_hp() {
    use crate::components::{Boss, Raged};
    use crate::systems::enemy::boss_rage;

    let mut app = test_app();
    let world = app.world_mut();

    let boss = world
        .spawn((
            Enemy { kind: EnemyKind::Titan },
            Boss { tier: 1 },
            Health { current: 24.0, max: 80.0 }, // 30% < 33% → rage
            FireCooldown { cooldown: 2.0, timer: 1.0 },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();

    #[derive(Resource, Default)]
    struct FireCount(u32);
    world.insert_resource(FireCount::default());
    fn count(mut r: MessageReader<Fire>, mut c: ResMut<FireCount>) {
        for _ in r.read() {
            c.0 += 1;
        }
    }

    let mut step = Schedule::default();
    step.add_systems((boss_rage, count).chain());
    step.run(world);

    assert!(world.get::<Raged>(boss).is_some(), "boss should rage at ≤33% HP");
    assert!(
        world.get::<Invulnerable>(boss).is_some(),
        "rage grants an invuln window"
    );
    assert!(
        world.resource::<FireCount>().0 >= 16,
        "rage fires a 16-bullet tantrum"
    );
    let cd = world.get::<FireCooldown>(boss).unwrap().cooldown;
    assert!((cd - 2.0 * 0.66).abs() < 0.01, "fire cooldown cut ×0.66 (got {cd})");
}

// ── 17. weapon_trait_helpers ──────────────────────────────────────────────────

/// The primary-weapon trait math (spec III.2): `_RAPID` = ×0.88^stacks (faster),
/// `_MULTI` fan = `min(0.8, 0.12*(count−1))`.
#[test]
fn weapon_trait_helpers() {
    use crate::systems::weapons::{multishot_fan, rapid_cooldown_mult};

    assert_eq!(rapid_cooldown_mult(0), 1.0);
    assert!((rapid_cooldown_mult(1) - 0.88).abs() < 1e-5);
    assert!(rapid_cooldown_mult(4) < rapid_cooldown_mult(1), "more stacks → faster");

    assert_eq!(multishot_fan(1), 0.0);
    assert!((multishot_fan(2) - 0.12).abs() < 1e-5);
    assert!(multishot_fan(100) <= 0.8, "fan width is capped at 0.8 rad");
}

// ── 18. burning_dots_then_expires ─────────────────────────────────────────────

/// A `Burning` status ticks `dps × dt` damage into its enemy and removes itself
/// once its duration lapses (spec III.3 Lance burn).
#[test]
fn burning_dots_then_expires() {
    use crate::components::Burning;
    use crate::systems::status::tick_burning;

    let mut app = test_app();
    let world = app.world_mut();

    let e = world
        .spawn((
            Enemy { kind: EnemyKind::Hunter },
            Health::new(20.0),
            Burning { dps: 6.0, secs: 0.3 },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();

    let mut step = Schedule::default();
    step.add_systems((tick_burning, apply_damage).chain());

    // Step 1 (dt 0.1 s): 0.6 dmg dealt, burn persists.
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(0.1));
    world.insert_resource(time.clone());
    step.run(world);
    assert!(world.get::<Health>(e).unwrap().current < 20.0, "burn deals damage");
    assert!(world.get::<Burning>(e).is_some(), "burn persists mid-duration");

    // Step 2 (dt 0.5 s): pushes past the remaining 0.2 s → burn removed.
    time.advance_by(Duration::from_secs_f32(0.5));
    world.insert_resource(time);
    step.run(world);
    assert!(world.get::<Burning>(e).is_none(), "burn expires");
}

// ── 19. weapon_trait_homing_explode_helpers ───────────────────────────────────

/// `_HOMING` / `_EXPLODE` trait math (spec III.2): homing rad/sec = min(0.4,
/// 0.09×stacks)×60, off at 0; explode radius = 30 + 10×stacks, off at 0.
#[test]
fn weapon_trait_homing_explode_helpers() {
    use crate::systems::shop::{explosion_radius, homing_turn_rate};

    assert_eq!(homing_turn_rate(0), 0.0);
    assert!((homing_turn_rate(1) - 0.09 * 60.0).abs() < 1e-4);
    assert!((homing_turn_rate(10) - 0.4 * 60.0).abs() < 1e-4, "capped at 0.4 rad/frame");

    assert_eq!(explosion_radius(0), 0.0);
    assert_eq!(explosion_radius(1), 40.0);
    assert_eq!(explosion_radius(3), 60.0);
}

// ── 21. knockback_shoves_target ───────────────────────────────────────────────

/// `knock_chance` = 0.15×stacks, and `apply_knockback` nudges the target's
/// position by the impulse (spec III.2/III.6 `_KNOCK`).
#[test]
fn knockback_shoves_target() {
    use crate::messages::Knockback;
    use crate::systems::collision::apply_knockback;
    use crate::systems::shop::knock_chance;

    assert_eq!(knock_chance(0), 0.0);
    assert!((knock_chance(2) - 0.30).abs() < 1e-5);

    let mut app = test_app();
    let world = app.world_mut();
    let e = world.spawn(Transform::from_xyz(10.0, 0.0, 0.0)).id();

    // Queue a shove via the message, then run the applier.
    world.write_message(Knockback {
        target: e,
        impulse: Vec2::new(16.0, 0.0),
    });
    let mut step = Schedule::default();
    step.add_systems(apply_knockback);
    step.run(world);

    let x = world.get::<Transform>(e).unwrap().translation.x;
    assert!((x - 26.0).abs() < 1e-4, "target shoved +16 px on x (got {x})");
}

// ── 22. vampirism_heals_on_hit ────────────────────────────────────────────────

/// VAMPIRISM: a player bullet hit heals the ship for a fraction of the damage
/// dealt (spec III.5). Pure helpers checked alongside.
#[test]
fn vampirism_heals_on_hit() {
    use crate::systems::collision::bullet_hits_enemy;
    use crate::systems::shop::{dodge_chance, vampirism_frac, UpgradeId, Upgrades};

    assert_eq!(vampirism_frac(0), 0.0);
    assert!((vampirism_frac(5) - 0.25).abs() < 1e-5);
    assert_eq!(dodge_chance(0), 0.0);
    assert!((dodge_chance(20) - 0.5).abs() < 1e-5, "dodge caps at 0.5");

    let mut app = test_app();
    app.world_mut()
        .resource_mut::<Upgrades>()
        .set(UpgradeId::Vampirism, 5); // 25% lifesteal
    let world = app.world_mut();

    // Hurt player (well below max) so the heal is observable.
    let player = world
        .spawn((
            Ship::default(),
            Health { current: 10.0, max: 40.0 },
            Collider { radius: 16.0 },
            Faction::Player,
            Transform::from_xyz(500.0, 0.0, 0.0), // far from the enemy
        ))
        .id();
    // Enemy + overlapping player bullet at the origin.
    world.spawn((
        Enemy { kind: EnemyKind::Hunter },
        Health::new(100.0),
        Collider { radius: 16.0 },
        Faction::Enemy,
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));
    world.spawn((
        Bullet { kind: BulletKind::Player, damage: 10.0, pierce: 0 },
        Collider { radius: 3.0 },
        Faction::Player,
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));

    let mut step = Schedule::default();
    step.add_systems((bullet_hits_enemy, apply_damage).chain());
    step.run(world);

    let hp = world.get::<Health>(player).unwrap().current;
    assert!(hp > 10.0, "vampirism should heal the player on a hit (hp now {hp})");
}

// ── 23. dodge_ignores_about_half ──────────────────────────────────────────────

/// DODGE: with 10 stacks (50% chance), roughly half of many incoming hits are
/// ignored entirely (spec III.5). Seeded RNG → deterministic count.
#[test]
fn dodge_ignores_about_half() {
    use crate::systems::damage::apply_damage;
    use crate::systems::shop::{UpgradeId, Upgrades};

    let mut app = test_app();
    app.world_mut()
        .resource_mut::<Upgrades>()
        .set(UpgradeId::Dodge, 10); // 50% dodge
    let world = app.world_mut();

    let player = world
        .spawn((
            Ship::default(),
            Health { current: 10000.0, max: 10000.0 },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();

    // 1000 one-damage hits; ~half should be dodged.
    for _ in 0..1000 {
        world.write_message(Damage { target: player, amount: 1.0 });
    }
    let mut step = Schedule::default();
    step.add_systems(apply_damage);
    step.run(world);

    let lost = 10000.0 - world.get::<Health>(player).unwrap().current;
    assert!(
        (350.0..650.0).contains(&lost),
        "≈half of 1000 hits should land with 50% dodge (got {lost})"
    );
}

// ── 20. explosive_bullet_splashes_nearby ──────────────────────────────────────

/// An explosive player bullet damages the enemy it hits *and* splashes other
/// enemies within the blast radius, but not those outside it (spec III.2).
#[test]
fn explosive_bullet_splashes_nearby() {
    use crate::systems::collision::bullet_hits_enemy;
    use crate::systems::shop::{UpgradeId, Upgrades};

    let mut app = test_app();
    // One explode stack → blast radius 40.
    app.world_mut()
        .resource_mut::<Upgrades>()
        .set(UpgradeId::ExplodeShot, 1);
    let world = app.world_mut();

    let spawn_enemy = |world: &mut World, x: f32| -> Entity {
        world
            .spawn((
                Enemy { kind: EnemyKind::Hunter },
                Health::new(100.0),
                Collider { radius: 16.0 },
                Faction::Enemy,
                Transform::from_xyz(x, 0.0, 0.0),
            ))
            .id()
    };
    let primary = spawn_enemy(world, 0.0);
    let near = spawn_enemy(world, 30.0); // within 40 + radius
    let far = spawn_enemy(world, 200.0); // well outside

    world.spawn((
        Bullet { kind: BulletKind::Player, damage: 5.0, pierce: 0 },
        Collider { radius: 3.0 },
        Faction::Player,
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));

    let mut step = Schedule::default();
    step.add_systems((bullet_hits_enemy, apply_damage).chain());
    step.run(world);

    assert!(world.get::<Health>(primary).unwrap().current < 100.0, "primary hit");
    assert!(world.get::<Health>(near).unwrap().current < 100.0, "nearby enemy splashed");
    assert_eq!(
        world.get::<Health>(far).unwrap().current,
        100.0,
        "distant enemy is outside the blast"
    );
}
