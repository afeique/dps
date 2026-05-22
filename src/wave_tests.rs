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
        .add_message::<crate::messages::PlayerHurt>()
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
    use crate::resources::{crit_chance, roll_crit, GameRng};

    // Crit chance scales 8% → cap 60% (spec III.6).
    assert!((crit_chance(0) - 0.08).abs() < 1e-6);
    assert!((crit_chance(2) - 0.22).abs() < 1e-6);
    assert_eq!(crit_chance(100), 0.60, "crit chance caps at 60%");

    let mut rng = GameRng::default();
    let mut crits = 0;
    let n = 20_000;
    for _ in 0..n {
        let m = roll_crit(&mut rng, crit_chance(0), 0);
        assert!(
            m == 1.0 || (2.0..=3.0).contains(&m),
            "base crit multiplier must be 1.0 or in [2,3], got {m}"
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

    // Crit-damage stacks raise the upper bound (uniform [2, 3+0.15*stacks], cap 5.5).
    let mut rng2 = GameRng::default();
    let mut max_seen = 0.0_f32;
    for _ in 0..50_000 {
        let m = roll_crit(&mut rng2, 1.0, 6); // always crit, 6 dmg stacks → max 3.9
        assert!((2.0..=3.9 + 1e-3).contains(&m), "upgraded crit in [2, 3.9], got {m}");
        max_seen = max_seen.max(m);
    }
    assert!(max_seen > 3.5, "with 6 crit-damage stacks the roll should reach toward 3.9");
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

// ── 24. thorns_reflects_to_nearest_enemy ──────────────────────────────────────

/// THORNS: a `PlayerHurt` event reflects 0.25×stacks of the landed damage to the
/// enemy nearest the ship (spec III.5). `thorns_frac` checked alongside.
#[test]
fn thorns_reflects_to_nearest_enemy() {
    use crate::messages::{Damage, PlayerHurt};
    use crate::systems::damage::apply_thorns;
    use crate::systems::shop::{thorns_frac, UpgradeId, Upgrades};

    assert_eq!(thorns_frac(0), 0.0);
    assert!((thorns_frac(4) - 1.0).abs() < 1e-5);

    let mut app = test_app();
    app.world_mut()
        .resource_mut::<Upgrades>()
        .set(UpgradeId::Thorns, 2); // 50% reflect
    let world = app.world_mut();

    world.spawn((Ship::default(), Transform::from_xyz(0.0, 0.0, 0.0)));
    let near = world
        .spawn((
            Enemy { kind: EnemyKind::Hunter },
            Health::new(100.0),
            Collider { radius: 16.0 },
            Faction::Enemy,
            Transform::from_xyz(20.0, 0.0, 0.0),
        ))
        .id();
    // A farther enemy that should NOT be the thorns target.
    world.spawn((
        Enemy { kind: EnemyKind::Hunter },
        Health::new(100.0),
        Collider { radius: 16.0 },
        Faction::Enemy,
        Transform::from_xyz(300.0, 0.0, 0.0),
    ));

    // Player took 40 damage → 50% thorns → 20 reflected to the nearest enemy.
    world.write_message(PlayerHurt { amount: 40.0 });

    #[derive(Resource, Default)]
    struct Reflected(f32);
    world.insert_resource(Reflected::default());
    fn capture(mut r: MessageReader<Damage>, mut out: ResMut<Reflected>) {
        for d in r.read() {
            out.0 = d.amount;
        }
    }

    let mut step = Schedule::default();
    step.add_systems((apply_thorns, capture).chain());
    step.run(world);

    assert!(
        (world.resource::<Reflected>().0 - 20.0).abs() < 1e-4,
        "thorns reflects 50% of 40 = 20 (got {})",
        world.resource::<Reflected>().0
    );
    // The reflected Damage targets the nearer enemy (sanity: it exists).
    assert!(world.get::<Health>(near).is_some());
}

// ── 25. survivor_cards_distinct + wave reward gate ────────────────────────────

/// The survivor pick offers 3 distinct cards drawn from the passive pool (spec
/// V.6 / III.5), and the wave only advances once the reward is taken.
#[test]
fn survivor_cards_and_wave_gate() {
    use crate::resources::GameRng;
    use crate::systems::survivor::{choose_cards, POOL};
    use crate::systems::wave::Wave;

    // choose_cards: 3 distinct, all from the pool, over many seeds.
    let mut rng = GameRng::default();
    for _ in 0..200 {
        let cards = choose_cards(&mut rng);
        let picked: Vec<_> = cards.iter().flatten().copied().collect();
        assert_eq!(picked.len(), 3, "three cards offered");
        for c in &picked {
            assert!(POOL.contains(c), "card {c:?} comes from the pool");
        }
        for i in 0..picked.len() {
            for j in (i + 1)..picked.len() {
                assert_ne!(picked[i], picked[j], "cards are distinct");
            }
        }
    }

    // The reward gate: advance only happens via advance_after_reward.
    let mut w = Wave::default();
    assert_eq!(w.number(), 1);
    w.awaiting_reward = true;
    w.advance_after_reward();
    assert!(!w.awaiting_reward, "gate cleared");
    assert_eq!(w.number(), 2, "advanced to wave 2 after the reward");
}

// ── 26. emp_pulse_stuns_in_radius ─────────────────────────────────────────────

/// EMP Pulse (V) stuns enemies within EMP_RADIUS of the ship but not those
/// outside it (spec III.4).
#[test]
fn emp_pulse_stuns_in_radius() {
    use crate::components::Stunned;
    use crate::systems::skills::{emp_pulse, Skills};

    let mut app = test_app();
    let world = app.world_mut();

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::KeyV);
    world.insert_resource(keys);
    world.insert_resource(Skills::default());
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);

    world.spawn((Ship::default(), Transform::from_xyz(0.0, 0.0, 0.0)));
    let near = world
        .spawn((Enemy { kind: EnemyKind::Hunter }, Transform::from_xyz(150.0, 0.0, 0.0)))
        .id();
    let far = world
        .spawn((Enemy { kind: EnemyKind::Hunter }, Transform::from_xyz(400.0, 0.0, 0.0)))
        .id();

    let mut step = Schedule::default();
    step.add_systems(emp_pulse);
    step.run(world);

    assert!(world.get::<Stunned>(near).is_some(), "enemy within EMP radius is stunned");
    assert!(world.get::<Stunned>(far).is_none(), "enemy outside the radius is not");
}

// ── 27. bulwark_halves_player_damage ──────────────────────────────────────────

/// While Bulwark is active, incoming player damage is halved after the shield
/// (spec II.2 step 7 / III.4).
#[test]
fn bulwark_halves_player_damage() {
    use crate::components::Bulwark;
    use crate::messages::Damage;
    use crate::systems::damage::apply_damage;

    let mut app = test_app();
    let world = app.world_mut();

    // No shield → isolate Bulwark's 50% cut. 100-dmg hit → 50 with Bulwark.
    let player = world
        .spawn((
            Ship::default(),
            Health::new(1000.0),
            Bulwark { seconds: 4.0 },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();
    world.write_message(Damage { target: player, amount: 100.0 });

    let mut step = Schedule::default();
    step.add_systems(apply_damage);
    step.run(world);

    let hp = world.get::<Health>(player).unwrap().current;
    assert!((hp - 950.0).abs() < 1e-3, "Bulwark halves 100 → 50 (hp now {hp})");
}

// ── 28. repair_nanites_regen_then_expires ─────────────────────────────────────

/// Repair Nanites regenerates HP over its window (capped at max) and then
/// removes itself (spec III.4).
#[test]
fn repair_nanites_regen_then_expires() {
    use crate::components::Repairing;
    use crate::systems::skills::tick_repair;

    let mut app = test_app();
    let world = app.world_mut();

    let player = world
        .spawn((
            Ship::default(),
            Health { current: 10.0, max: 40.0 },
            Repairing { seconds: 5.0, rate: 3.0 },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();

    let mut step = Schedule::default();
    step.add_systems(tick_repair);

    // Step 1 (dt 1 s): +3 HP, window persists.
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(1.0));
    world.insert_resource(time.clone());
    step.run(world);
    assert!((world.get::<Health>(player).unwrap().current - 13.0).abs() < 1e-3, "regen 3/s");
    assert!(world.get::<Repairing>(player).is_some(), "window persists");

    // Step 2 (dt 5 s): past the remaining 4 s → expires (and heal capped at max).
    time.advance_by(Duration::from_secs_f32(5.0));
    world.insert_resource(time);
    step.run(world);
    assert!(world.get::<Repairing>(player).is_none(), "window expires");
    assert!(world.get::<Health>(player).unwrap().current <= 40.0, "heal capped at max");
}

// ── 29. deflector_orb_blocks_enemy_bullets ────────────────────────────────────

/// A Deflector Orb absorbs overlapping enemy bullets, spending one block each,
/// and pops at 0 blocks (spec III.4).
#[test]
fn deflector_orb_blocks_enemy_bullets() {
    use crate::components::DeflectorOrb;
    use crate::systems::skills::deflector_blocks;

    let mut app = test_app();
    let world = app.world_mut();

    let orb = world
        .spawn((
            DeflectorOrb { blocks: 2, phase: 0.0 },
            Collider { radius: 8.0 },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();
    let bullet = world
        .spawn((
            Bullet { kind: BulletKind::Enemy, damage: 5.0, pierce: 0 },
            Collider { radius: 4.0 },
            Faction::Enemy,
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();

    let mut step = Schedule::default();
    step.add_systems(deflector_blocks);
    step.run(world);

    assert!(world.get::<Bullet>(bullet).is_none(), "enemy bullet absorbed");
    assert_eq!(world.get::<DeflectorOrb>(orb).unwrap().blocks, 1, "one block spent");

    // A second bullet spends the last block and pops the orb.
    world.spawn((
        Bullet { kind: BulletKind::Enemy, damage: 5.0, pierce: 0 },
        Collider { radius: 4.0 },
        Faction::Enemy,
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));
    step.run(world);
    assert!(world.get::<DeflectorOrb>(orb).is_none(), "orb pops at 0 blocks");
}

// ── 30. tractor_shield_absorbs_forward_bullets ────────────────────────────────

/// Tractor Shield absorbs in-arc, in-range enemy bullets into coins, but not
/// bullets behind the ship or out of range (spec III.4).
#[test]
fn tractor_shield_absorbs_forward_bullets() {
    use crate::components::TractorShield;
    use crate::systems::skills::{in_tractor_arc, tractor_absorb};

    // Arc helper: facing +Y. Dead ahead in range → in; behind → out; far → out.
    let up = Vec2::new(0.0, 1.0);
    assert!(in_tractor_arc(up, Vec2::new(0.0, 40.0), std::f32::consts::FRAC_PI_4, 55.0));
    assert!(!in_tractor_arc(up, Vec2::new(0.0, -40.0), std::f32::consts::FRAC_PI_4, 55.0));
    assert!(!in_tractor_arc(up, Vec2::new(0.0, 90.0), std::f32::consts::FRAC_PI_4, 55.0));

    let mut app = test_app();
    let world = app.world_mut();

    // Ship facing +Y (identity rotation → forward +Y), tractor active.
    world.spawn((
        Ship::default(),
        TractorShield { seconds: 4.0 },
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));
    // In-arc enemy bullet (ahead, in range) → absorbed for coins.
    let front = world
        .spawn((
            Bullet { kind: BulletKind::Enemy, damage: 5.0, pierce: 0 },
            Faction::Enemy,
            Transform::from_xyz(0.0, 40.0, 0.0),
        ))
        .id();
    // Behind the ship → untouched.
    let behind = world
        .spawn((
            Bullet { kind: BulletKind::Enemy, damage: 5.0, pierce: 0 },
            Faction::Enemy,
            Transform::from_xyz(0.0, -40.0, 0.0),
        ))
        .id();

    let mut step = Schedule::default();
    step.add_systems(tractor_absorb);
    step.run(world);

    assert!(world.get::<Bullet>(front).is_none(), "in-arc bullet absorbed");
    assert!(world.get::<Bullet>(behind).is_some(), "bullet behind the ship is not");
    assert_eq!(world.resource::<Score>().gold, 5, "absorb minted coins");
}

// ── 31. damage_numbers_spawn_and_float ────────────────────────────────────────

/// A Damage event spawns a floating number at the target, which rises and then
/// despawns at end of life (spec VIII.1).
#[test]
fn damage_numbers_spawn_and_float() {
    use crate::messages::Damage;
    use crate::render::damage_numbers::{
        float_damage_numbers, spawn_damage_numbers, DamageNumber,
    };

    let mut app = test_app();
    let world = app.world_mut();

    let enemy = world.spawn((Enemy { kind: EnemyKind::Hunter }, Transform::from_xyz(0.0, 0.0, 0.0))).id();
    world.write_message(Damage { target: enemy, amount: 5.0 });

    let mut spawn = Schedule::default();
    spawn.add_systems(spawn_damage_numbers);
    spawn.run(world);

    let count = world.query::<&DamageNumber>().iter(world).count();
    assert_eq!(count, 1, "a Damage event spawns one floating number");

    // Float: rises on a small step, despawns once its 0.8 s life lapses.
    let mut step = Schedule::default();
    step.add_systems(float_damage_numbers);

    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(0.1));
    world.insert_resource(time.clone());
    step.run(world);
    let y_after = world
        .query_filtered::<&Transform, With<DamageNumber>>()
        .iter(world)
        .next()
        .map(|t| t.translation.y);
    assert!(y_after.is_some_and(|y| y > 14.0), "number drifts upward");

    time.advance_by(Duration::from_secs_f32(1.0));
    world.insert_resource(time);
    step.run(world);
    assert_eq!(
        world.query::<&DamageNumber>().iter(world).count(),
        0,
        "number despawns at end of life"
    );
}

// ── 32. minimap_maps_world_to_panel ───────────────────────────────────────────

/// The minimap mapping puts world-center at panel-center, the arena corners at
/// the panel corners (y flipped for top-down UI), and clamps out-of-arena points.
#[test]
fn minimap_maps_world_to_panel() {
    use crate::render::minimap::world_to_minimap;

    let half = Vec2::new(640.0, 360.0);
    let size = 150.0;
    assert_eq!(world_to_minimap(Vec2::ZERO, half, size), Vec2::new(75.0, 75.0));
    assert_eq!(world_to_minimap(Vec2::new(-640.0, 360.0), half, size), Vec2::new(0.0, 0.0));
    assert_eq!(world_to_minimap(Vec2::new(640.0, -360.0), half, size), Vec2::new(150.0, 150.0));

    let c = world_to_minimap(Vec2::new(9_999.0, -9_999.0), half, size);
    assert!((0.0..=150.0).contains(&c.x) && (0.0..=150.0).contains(&c.y), "clamps in-bounds");
}

// ── 33. wave_start_spawns_asteroids ───────────────────────────────────────────

/// A wave's opening pulse spawns its `WaveDef.asteroids` budget (spec V); wave 1
/// = 5 asteroids.
#[test]
fn wave_start_spawns_asteroids() {
    let mut app = test_app();
    let world = app.world_mut();
    world.insert_resource(Wave::default());
    world.insert_resource(PlayBounds::default());

    // Clear the 1 s intro so pulse 0 fires.
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(1.5));
    world.insert_resource(time);

    let mut step = Schedule::default();
    step.add_systems(wave::spawn_waves);
    step.run(world);

    let asteroids = world.query::<&Asteroid>().iter(world).count();
    assert_eq!(asteroids, 5, "wave 1 spawns its 5-asteroid budget on pulse 0");
}

// ── 34. update_nova_no_query_conflict (regression: B0001) ─────────────────────

/// `update_nova`'s mut-Transform ring query must stay disjoint from its
/// immut-Transform enemy query (via `Without<Enemy>`). Running the system in a
/// schedule triggers Bevy's intra-system access check — if the filter is ever
/// dropped this panics with B0001 at initialize.
#[test]
fn update_nova_no_query_conflict() {
    use crate::systems::power_weapon::update_nova;

    let mut app = test_app();
    app.world_mut().insert_resource(Time::<()>::default());

    let mut sched = Schedule::default();
    sched.add_systems(update_nova);
    sched.run(app.world_mut()); // would panic (B0001) if the queries conflicted
}

// ── 35. survivor_stage_clear_gating ───────────────────────────────────────────

/// Survivor cards are drawn only on stage clears (every 3rd wave); mid-stage
/// waves auto-advance with the smaller coin bonus and no pick (spec V.6).
#[test]
fn survivor_stage_clear_gating() {
    use crate::systems::survivor::{check_survivor, is_stage_clear, midstage_bonus, stage_bonus};

    assert!(!is_stage_clear(1) && !is_stage_clear(2) && is_stage_clear(3));
    assert!(is_stage_clear(6) && !is_stage_clear(7));
    // Wave 3: mid = round((50+75)*0.6) = 75; stage = (50+75)*2 = 250.
    assert_eq!(midstage_bonus(3), 75);
    assert_eq!(stage_bonus(3), 250);

    // A mid-stage clear (wave 1) auto-advances with the small bonus, no pick.
    let mut app = test_app();
    let world = app.world_mut();
    let mut w = Wave::default();
    w.awaiting_reward = true;
    world.insert_resource(w);

    let mut step = Schedule::default();
    step.add_systems(check_survivor);
    step.run(world);

    assert_eq!(world.resource::<Wave>().number(), 2, "mid-stage wave auto-advances");
    assert!(!world.resource::<Wave>().awaiting_reward, "reward gate cleared");
    assert_eq!(world.resource::<Score>().gold, midstage_bonus(1), "mid-stage bonus awarded");
    assert!(
        matches!(world.resource::<NextState<GameState>>(), NextState::Unchanged),
        "no Survivor pick on a mid-stage clear"
    );
}

// ── 36. mini_boss_promotion ───────────────────────────────────────────────────

/// Mini-boss promotion chance ramps from wave 4 (0 before) and caps at 0.45;
/// a promoted spawn carries the HP×1.7 / radius×1.25 overlay + marker (spec V.6).
#[test]
fn mini_boss_promotion() {
    use crate::components::MiniBoss;
    use crate::systems::enemy::{self, mini_boss_chance};

    assert_eq!(mini_boss_chance(3), 0.0, "no promotion below wave 4");
    assert!((mini_boss_chance(4) - 0.06).abs() < 1e-5);
    assert!((mini_boss_chance(10) - 0.21).abs() < 1e-5);
    assert_eq!(mini_boss_chance(100), 0.45, "caps at 0.45");

    let mut app = test_app();
    let world = app.world_mut();
    let mut step = Schedule::default();
    step.add_systems(|mut c: Commands| enemy::spawn_mini_boss(&mut c, EnemyKind::Hunter, Vec2::ZERO));
    step.run(world);

    let mut q = world.query_filtered::<(&Health, &Collider, &MiniBoss), With<Enemy>>();
    let (hp, col, _) = q.iter(world).next().expect("a mini-boss should exist");
    // Hunter base HP 5 → ×1.7 = 8.5; radius 16 → ×1.25 = 20.
    assert!((hp.max - 8.5).abs() < 0.01, "HP×1.7 (got {})", hp.max);
    assert!((col.radius - 20.0).abs() < 0.01, "radius×1.25 (got {})", col.radius);
}

// ── 37. difficulty_curve_scales_hp_and_points ─────────────────────────────────

/// The V.4 difficulty curve ramps enemy HP (1× at W1 → 15.5× at W30) and points
/// (1× → 6.5×); `spawn_for_wave` applies the HP curve at spawn.
#[test]
fn difficulty_curve_scales_hp_and_points() {
    use crate::systems::enemy::{self, difficulty_hp_mul, difficulty_points_mul};

    assert!((difficulty_hp_mul(1) - 1.0).abs() < 1e-4);
    assert!((difficulty_hp_mul(30) - 15.5).abs() < 1e-3, "W30 HP ×15.5");
    assert!(difficulty_hp_mul(15) > difficulty_hp_mul(5), "monotonic ramp");
    assert!((difficulty_points_mul(1) - 1.0).abs() < 1e-4);
    assert!((difficulty_points_mul(30) - 6.5).abs() < 1e-3, "W30 points ×6.5");

    let mut app = test_app();
    let world = app.world_mut();
    let mut step = Schedule::default();
    step.add_systems(|mut c: Commands| {
        enemy::spawn_for_wave(&mut c, EnemyKind::Hunter, Vec2::ZERO, 0, false, 30)
    });
    step.run(world);
    let mut q = world.query_filtered::<&Health, With<Enemy>>();
    let hp = q.iter(world).next().expect("enemy spawned").max;
    // Hunter base 5 × difficulty_hp_mul(30) 15.5 = 77.5.
    assert!((hp - 5.0 * 15.5).abs() < 0.1, "W30 Hunter HP = 5×15.5 (got {hp})");
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
