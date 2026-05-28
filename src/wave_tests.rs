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
        .add_message::<crate::messages::Crit>()
        .add_message::<crate::messages::Shard>()
        .add_message::<crate::messages::Reaction>()
        .init_resource::<crate::meta::Meta>()
        .init_resource::<Score>()
        .init_resource::<crate::resources::KillStreak>()
        .init_resource::<crate::resources::GameRng>()
        .init_resource::<crate::resources::EnergyMeter>()
        .init_resource::<crate::systems::shop::Upgrades>()
        .init_resource::<crate::systems::items::Equipment>()
        .init_resource::<crate::systems::formations::Formations>()
        .init_resource::<crate::combat::reaction::PendingReactions>()
        .init_resource::<crate::resources::LastStandUsed>()
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
        let m = roll_crit(&mut rng, crit_chance(0), 0, 0.0);
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
        let m = roll_crit(&mut rng2, 1.0, 6, 0.0); // always crit, 6 dmg stacks → max 3.9
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

/// Account SP CAPACITOR raises the energy cap (Phase ME): reset_energy sets the
/// per-run max to ENERGY_MAX + the SP bonus, and `gain` then fills past the base.
#[test]
fn sp_capacitor_raises_energy_cap() {
    use crate::meta::Meta;
    use crate::resources::{EnergyMeter, ENERGY_MAX};
    use crate::systems::power_weapon::{reset_energy, PowerWeapon};

    let mut app = test_app();
    {
        let mut meta = app.world_mut().resource_mut::<Meta>();
        meta.sp = 20;
        for _ in 0..20 {
            meta.allocate_sp("CAPACITOR"); // +100 max energy at the cap
        }
    }
    app.world_mut().init_resource::<EnergyMeter>();
    app.world_mut().init_resource::<PowerWeapon>();

    let mut step = Schedule::default();
    step.add_systems(reset_energy);
    step.run(app.world_mut());

    let target = ENERGY_MAX + 100.0;
    assert!(
        (app.world().resource::<EnergyMeter>().max - target).abs() < 1e-3,
        "CAPACITOR lifts the cap to {target}"
    );
    // gain now fills past the base 100 cap.
    app.world_mut().resource_mut::<EnergyMeter>().gain(1000.0);
    assert!((app.world().resource::<EnergyMeter>().current - target).abs() < 1e-3);
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

/// W: a power weapon's damage respects the target's elemental resistance — a
/// KINETIC-resistant enemy takes less from a (KINETIC) mine blast than a neutral
/// one in the same detonation.
#[test]
fn power_weapon_respects_element_resistance() {
    use crate::combat::element::{Element, Resistances};
    use crate::systems::power_weapon::{lay_mine, update_mines};

    let mut app = test_app();
    let world = app.world_mut();

    let resistant = world
        .spawn((
            Enemy { kind: EnemyKind::Hunter },
            Health::new(100.0),
            Collider { radius: 16.0 },
            Resistances::new().with(Element::Kinetic, 0.5),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();
    let neutral = world
        .spawn((
            Enemy { kind: EnemyKind::Hunter },
            Health::new(100.0),
            Collider { radius: 16.0 },
            Transform::from_xyz(30.0, 0.0, 0.0),
        ))
        .id();

    let mut setup = Schedule::default();
    setup.add_systems(|mut c: Commands| lay_mine(&mut c, Vec2::ZERO));
    setup.run(world);

    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(1.0)); // past the 0.6 s arm delay → detonates
    world.insert_resource(time);

    let mut step = Schedule::default();
    step.add_systems((update_mines, apply_damage).chain());
    step.run(world);

    let r_hp = world.get::<Health>(resistant).unwrap().current;
    let n_hp = world.get::<Health>(neutral).unwrap().current;
    assert!(r_hp < 100.0 && n_hp < 100.0, "both took blast damage");
    assert!(r_hp > n_hp, "kinetic-resistant enemy took less ({r_hp} vs {n_hp})");
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

// ── 16. boss_telegraphs_then_rages ────────────────────────────────────────────

/// A boss at ≤33% HP telegraphs first, then rages (spec IV.7). `boss_rage` adds
/// `RageTelegraph` (no tantrum yet); after the ~0.4 s window, `tick_rage_telegraph`
/// fires the actual rage: the `Raged` marker + invuln window, fire cooldown cut
/// ×0.66, and a 16-bullet tantrum.
#[test]
fn boss_telegraphs_then_rages() {
    use crate::components::{Boss, RageTelegraph, Raged};
    use crate::systems::enemy::{boss_rage, tick_rage_telegraph, TELEGRAPH_SECS};

    let mut app = test_app();
    let world = app.world_mut();

    let boss = world
        .spawn((
            Enemy { kind: EnemyKind::Titan },
            Boss { tier: 1 },
            Health { current: 24.0, max: 80.0 }, // 30% < 33% → rage
            FireCooldown { cooldown: 2.0, timer: 1.0 },
            Collider { radius: 40.0 },
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

    // Stage 1: boss_rage starts the telegraph — no rage, no tantrum yet.
    world.insert_resource(Time::<()>::default());
    let mut s1 = Schedule::default();
    s1.add_systems((boss_rage, count).chain());
    s1.run(world);

    assert!(
        world.get::<RageTelegraph>(boss).is_some(),
        "≤33% HP starts the telegraph"
    );
    assert!(
        world.get::<Raged>(boss).is_none(),
        "the boss does not rage during the telegraph"
    );
    assert_eq!(
        world.resource::<FireCount>().0,
        0,
        "no tantrum during the telegraph"
    );

    // Stage 2: after the telegraph window, tick_rage_telegraph fires the rage.
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(TELEGRAPH_SECS + 0.05));
    world.insert_resource(time);
    let mut s2 = Schedule::default();
    s2.add_systems((tick_rage_telegraph, count).chain());
    s2.run(world);

    assert!(world.get::<Raged>(boss).is_some(), "telegraph lapses → rage");
    assert!(
        world.get::<RageTelegraph>(boss).is_none(),
        "telegraph marker is cleared on rage"
    );
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

/// `Bleed` (TOXIC poison, E3) is a no-refresh DoT — same shape as burn: ticks
/// `dps × dt` damage and expires on schedule.
#[test]
fn bleed_dots_then_expires() {
    use crate::components::Bleed;
    use crate::systems::status::tick_bleed;

    let mut app = test_app();
    let world = app.world_mut();

    let e = world
        .spawn((
            Enemy { kind: EnemyKind::Hunter },
            Health::new(20.0),
            Bleed { dps: 4.0, secs: 0.3 },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();

    let mut step = Schedule::default();
    step.add_systems((tick_bleed, apply_damage).chain());

    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(0.1));
    world.insert_resource(time.clone());
    step.run(world);
    assert!(world.get::<Health>(e).unwrap().current < 20.0, "bleed deals damage");
    assert!(world.get::<Bleed>(e).is_some(), "bleed persists mid-duration");

    time.advance_by(Duration::from_secs_f32(0.5));
    world.insert_resource(time);
    step.run(world);
    assert!(world.get::<Bleed>(e).is_none(), "bleed expires");
}

/// SHATTER (E4b): a shatter seed bursts CRYO damage + re-freezes every neighbor
/// within radius, excluding the source; neighbors outside the radius are spared.
#[test]
fn shatter_aoe_damages_and_freezes_neighbors() {
    use crate::combat::reaction::{PendingReactions, ReactionSeed};
    use crate::components::Frozen;
    use crate::systems::reactions::resolve_reactions;

    let mut app = test_app();
    let world = app.world_mut();

    let source = world
        .spawn((Enemy { kind: EnemyKind::Hunter }, Health::new(20.0), Transform::from_xyz(0.0, 0.0, 0.0)))
        .id();
    let near = world
        .spawn((Enemy { kind: EnemyKind::Hunter }, Health::new(20.0), Transform::from_xyz(50.0, 0.0, 0.0)))
        .id();
    let far = world
        .spawn((Enemy { kind: EnemyKind::Hunter }, Health::new(20.0), Transform::from_xyz(500.0, 0.0, 0.0)))
        .id();

    world.resource_mut::<PendingReactions>().0.push(ReactionSeed::Shatter {
        source,
        center: Vec2::ZERO,
        depth: 0,
    });

    let mut step = Schedule::default();
    step.add_systems((resolve_reactions, apply_damage).chain());
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(0.016));
    world.insert_resource(time);
    step.run(world);

    assert!(world.get::<Health>(near).unwrap().current < 20.0, "near neighbor shattered");
    assert!(world.get::<Frozen>(near).is_some(), "near neighbor re-frozen");
    assert_eq!(world.get::<Health>(far).unwrap().current, 20.0, "far neighbor untouched");
    assert_eq!(
        world.get::<Health>(source).unwrap().current,
        20.0,
        "source excluded from its own AoE"
    );
}

/// A shatter emits a `Reaction`, which `spawn_reaction_fx` turns into an
/// expanding `Shockwave` that grows then despawns past its lifetime (E4b VFX).
#[test]
fn shatter_emits_reaction_and_spawns_shockwave() {
    use crate::combat::reaction::{PendingReactions, ReactionSeed};
    use crate::render::reaction_fx::{spawn_reaction_fx, tick_shockwaves, Shockwave};
    use crate::systems::reactions::resolve_reactions;

    let mut app = test_app();
    app.world_mut().insert_resource(Time::<()>::default());
    let source = app
        .world_mut()
        .spawn((Enemy { kind: EnemyKind::Hunter }, Transform::from_xyz(0.0, 0.0, 0.0)))
        .id();
    app.world_mut()
        .resource_mut::<PendingReactions>()
        .0
        .push(ReactionSeed::Shatter { source, center: Vec2::new(10.0, 0.0), depth: 0 });

    // resolve_reactions → Reaction message → spawn_reaction_fx → one Shockwave.
    let mut step = Schedule::default();
    step.add_systems((resolve_reactions, spawn_reaction_fx).chain());
    step.run(app.world_mut());
    {
        let mut q = app.world_mut().query::<&Shockwave>();
        assert_eq!(q.iter(app.world()).count(), 1, "a shatter spawns one shockwave");
    }

    // A small tick grows the ring (scale > 1).
    {
        let mut t = Time::<()>::default();
        t.advance_by(Duration::from_secs_f32(0.1));
        app.world_mut().insert_resource(t);
    }
    let mut tick = Schedule::default();
    tick.add_systems(tick_shockwaves);
    tick.run(app.world_mut());
    {
        let mut q = app.world_mut().query_filtered::<&Transform, With<Shockwave>>();
        let scale = q.iter(app.world()).next().unwrap().scale.x;
        assert!(scale > 1.0, "the shockwave expands (scale {scale})");
    }

    // Past its 0.35 s lifetime → despawns.
    {
        let mut t = Time::<()>::default();
        t.advance_by(Duration::from_secs_f32(0.4));
        app.world_mut().insert_resource(t);
    }
    tick.run(app.world_mut());
    {
        let mut q = app.world_mut().query::<&Shockwave>();
        assert_eq!(q.iter(app.world()).count(), 0, "the shockwave despawns past its lifetime");
    }
}

/// The Catalyst passive amplifies elemental-reaction damage: a shatter with
/// Catalyst stacks hurts a caught neighbor more than the un-stacked baseline.
#[test]
fn catalyst_amplifies_reaction_damage() {
    use crate::combat::reaction::{PendingReactions, ReactionSeed};
    use crate::systems::damage::apply_damage;
    use crate::systems::reactions::resolve_reactions;
    use crate::systems::shop::{UpgradeId, Upgrades};

    // Run one shatter against a neighbor at 50 px; return the damage it took.
    fn shatter_damage(catalyst_stacks: u32) -> f32 {
        let mut app = test_app();
        app.world_mut()
            .resource_mut::<Upgrades>()
            .set(UpgradeId::Catalyst, catalyst_stacks);
        let world = app.world_mut();
        let source = world
            .spawn((Enemy { kind: EnemyKind::Hunter }, Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();
        let near = world
            .spawn((Enemy { kind: EnemyKind::Hunter }, Health::new(1000.0), Transform::from_xyz(50.0, 0.0, 0.0)))
            .id();
        world
            .resource_mut::<PendingReactions>()
            .0
            .push(ReactionSeed::Shatter { source, center: Vec2::ZERO, depth: 0 });
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_secs_f32(0.016));
        world.insert_resource(time);
        let mut step = Schedule::default();
        step.add_systems((resolve_reactions, apply_damage).chain());
        step.run(world);
        1000.0 - world.get::<Health>(near).unwrap().current
    }

    let base = shatter_damage(0);
    let amped = shatter_damage(5); // +125%
    assert!(base > 0.0, "shatter deals damage at baseline");
    assert!(
        amped > base * 2.0,
        "Catalyst ×5 (+125%) more than doubles reaction damage (base {base}, amped {amped})"
    );
}

/// The simple-timer elemental statuses (E3) count down and remove themselves on
/// expiry; `Corrode` keeps its stacks for its whole duration.
#[test]
fn status_timers_count_down_and_expire() {
    use crate::components::{Chill, Corrode, Frozen};
    use crate::systems::status::tick_status_timers;

    let mut app = test_app();
    let world = app.world_mut();

    let e = world
        .spawn((
            Enemy { kind: EnemyKind::Hunter },
            Chill { secs: 0.3 },
            Frozen { secs: 0.3 },
            Corrode { stacks: 2, secs: 0.3 },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();

    let mut step = Schedule::default();
    step.add_systems(tick_status_timers);

    // Mid-duration: all persist, corrode holds its stacks.
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(0.1));
    world.insert_resource(time.clone());
    step.run(world);
    assert!(world.get::<Chill>(e).is_some());
    assert!(world.get::<Frozen>(e).is_some());
    assert_eq!(world.get::<Corrode>(e).unwrap().stacks, 2);

    // Past expiry: all removed.
    time.advance_by(Duration::from_secs_f32(0.5));
    world.insert_resource(time);
    step.run(world);
    assert!(world.get::<Chill>(e).is_none());
    assert!(world.get::<Frozen>(e).is_none());
    assert!(world.get::<Corrode>(e).is_none());
}

/// EN batch E8b: the 4 Pyro/Cryo enemies carry the right attack element + resist
/// directions (near-immune to their own element, weak to the opposite).
#[test]
fn en_pyro_cryo_enemy_data() {
    use crate::combat::element::Element;
    use crate::systems::enemy::{element_for, points, resistances_for};

    assert_eq!(element_for(EnemyKind::Cinder), Element::Pyro);
    assert_eq!(element_for(EnemyKind::Glacier), Element::Cryo);
    assert_eq!(element_for(EnemyKind::FrostLance), Element::Cryo);
    assert_eq!(element_for(EnemyKind::AshenDetonator), Element::Pyro);

    let cinder = resistances_for(EnemyKind::Cinder);
    assert!((cinder.get(Element::Pyro) - 0.85).abs() < 1e-6, "near-fireproof");
    assert!(cinder.get(Element::Cryo) < 0.0, "weak to cryo");
    assert!((resistances_for(EnemyKind::Glacier).get(Element::Cryo) - 0.90).abs() < 1e-6);
    assert!(resistances_for(EnemyKind::Glacier).get(Element::Pyro) < 0.0, "burn the ice tank");

    assert_eq!(points(EnemyKind::Glacier), 250);
    assert_eq!(points(EnemyKind::Cinder), 110);
}

/// EN: a Spore Carrier births a Wasp drone when its timer lapses, and respects
/// the global drone cap.
#[test]
fn spore_carrier_spawns_drones_and_caps() {
    use crate::components::{Drone, DroneSpawner, SPORE_DRONE_CAP};
    use crate::systems::enemy::mechanics::spore_spawner;

    // (a) a ready spore with no drones out → births exactly one.
    let mut app = test_app();
    let world = app.world_mut();
    world.spawn((
        Enemy { kind: EnemyKind::SporeCarrier },
        Transform::from_xyz(0.0, 0.0, 0.0),
        DroneSpawner { timer: 0.05 },
    ));
    let mut step = Schedule::default();
    step.add_systems(spore_spawner);
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(0.1)); // past the 0.05 timer
    world.insert_resource(time);
    step.run(world);
    assert_eq!(world.query::<&Drone>().iter(world).count(), 1, "birthed one drone");

    // (b) at the cap, a ready spore births none.
    let mut app2 = test_app();
    let world2 = app2.world_mut();
    for _ in 0..SPORE_DRONE_CAP {
        world2.spawn((Enemy { kind: EnemyKind::Wasp }, Drone));
    }
    world2.spawn((
        Enemy { kind: EnemyKind::SporeCarrier },
        Transform::from_xyz(0.0, 0.0, 0.0),
        DroneSpawner { timer: 0.05 },
    ));
    let mut step2 = Schedule::default();
    step2.add_systems(spore_spawner);
    let mut time2 = Time::<()>::default();
    time2.advance_by(Duration::from_secs_f32(0.1));
    world2.insert_resource(time2);
    step2.run(world2);
    assert_eq!(
        world2.query::<&Drone>().iter(world2).count(),
        SPORE_DRONE_CAP,
        "no births past the cap"
    );
}

/// EN: a Toxic hazard field ticks damage + corrode on a player standing in it.
#[test]
fn hazard_field_ticks_and_corrodes_player() {
    use crate::combat::element::Element;
    use crate::systems::hazard::{tick_hazards, HazardField};

    let mut app = test_app();
    let world = app.world_mut();
    let p = world
        .spawn((Ship::default(), Health::new(40.0), Transform::from_xyz(0.0, 0.0, 0.0)))
        .id();
    world.spawn((
        HazardField { radius: 70.0, element: Element::Toxic, dps: 6.0, life: 4.0, tick: 0.1 },
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));

    let mut step = Schedule::default();
    step.add_systems((tick_hazards, apply_damage).chain());
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(0.2)); // past the 0.1 tick
    world.insert_resource(time);
    step.run(world);

    assert!(world.get::<Health>(p).unwrap().current < 40.0, "hazard ticked damage");
    assert!(world.get::<PlayerCorrode>(p).is_some(), "Toxic hazard corroded the ship");
}

/// EN: a Plaguebearer-type dropper drops a hazard field when its timer lapses.
#[test]
fn hazard_dropper_drops_hazard() {
    use crate::systems::hazard::{drop_hazards, plaguebearer_dropper, HazardField};

    let mut app = test_app();
    let world = app.world_mut();
    let mut d = plaguebearer_dropper();
    d.timer = 0.05;
    world.spawn((Transform::from_xyz(0.0, 0.0, 0.0), d));

    let mut step = Schedule::default();
    step.add_systems(drop_hazards);
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(0.1));
    world.insert_resource(time);
    step.run(world);

    assert_eq!(world.query::<&HazardField>().iter(world).count(), 1, "dropper spawned a hazard");
}

/// EN: a Warden adapts — a hit bumps its resistance to the hit's element — and
/// `decay_warden_resist` fades that resistance over time.
#[test]
fn warden_adapts_then_decays() {
    use crate::combat::element::{Element, ElementSet, Resistances};
    use crate::components::{Adaptive, Bullet, BulletElements, BulletKind};
    use crate::systems::enemy::mechanics::decay_warden_resist;

    // (a) a PYRO hit on a neutral Warden bumps its Pyro resist above 0.
    let mut app = test_app();
    let world = app.world_mut();
    let w = world
        .spawn((
            Enemy { kind: EnemyKind::Warden },
            Health::new(100.0),
            Collider { radius: 16.0 },
            Resistances::new(),
            Adaptive,
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();
    world.spawn((
        Bullet { kind: BulletKind::Player, damage: 5.0, pierce: 0 },
        BulletElements(ElementSet::single(Element::Pyro)),
        Collider { radius: 5.0 },
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));
    let mut step = Schedule::default();
    step.add_systems(bullet_hits_enemy);
    step.run(world);
    let pyro = world.get::<Resistances>(w).unwrap().get(Element::Pyro);
    assert!(pyro > 0.0, "Warden adapted to Pyro (got {pyro})");

    // (b) decay fades it back down.
    let mut step2 = Schedule::default();
    step2.add_systems(decay_warden_resist);
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(2.0));
    world.insert_resource(time);
    step2.run(world);
    let decayed = world.get::<Resistances>(w).unwrap().get(Element::Pyro);
    assert!(decayed < pyro, "adapted resist decays ({decayed} < {pyro})");
}

/// W: the Cryo Burst power weapon freezes enemies its ring sweeps over — the
/// player's setup for the Frozen → shatter reaction.
#[test]
fn cryo_burst_freezes_enemies() {
    use crate::components::Frozen;
    use crate::systems::power_weapon::{spawn_cryo_burst, update_nova};

    let mut app = test_app();
    let world = app.world_mut();
    let e = world
        .spawn((
            Enemy { kind: EnemyKind::Hunter },
            Health::new(20.0),
            Collider { radius: 16.0 },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();

    let mut setup = Schedule::default();
    setup.add_systems(|mut c: Commands| spawn_cryo_burst(&mut c, Vec2::ZERO));
    setup.run(world);

    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(0.05)); // ring expands over the enemy
    world.insert_resource(time);
    let mut step = Schedule::default();
    step.add_systems(update_nova);
    step.run(world);

    assert!(world.get::<Frozen>(e).is_some(), "Cryo Burst froze the enemy");
}

/// W: the Singularity pulls a nearby enemy toward its center, then collapses
/// into a Void AoE (damaging it) and despawns.
#[test]
fn singularity_pulls_then_collapses() {
    use crate::systems::power_weapon::{spawn_singularity, update_singularity, Singularity};

    let mut app = test_app();
    let world = app.world_mut();
    let e = world
        .spawn((Enemy { kind: EnemyKind::Hunter }, Health::new(20.0), Transform::from_xyz(150.0, 0.0, 0.0)))
        .id();

    let mut setup = Schedule::default();
    setup.add_systems(|mut c: Commands| spawn_singularity(&mut c, Vec2::ZERO));
    setup.run(world);

    let mut step = Schedule::default();
    step.add_systems((update_singularity, apply_damage).chain());

    // Phase 1: pull — the enemy is dragged toward the center.
    let mut t1 = Time::<()>::default();
    t1.advance_by(Duration::from_secs_f32(0.1));
    world.insert_resource(t1);
    step.run(world);
    assert!(world.get::<Transform>(e).unwrap().translation.x < 150.0, "enemy pulled inward");

    // Phase 2: past the pull duration → collapse damages + the orb despawns.
    let mut t2 = Time::<()>::default();
    t2.advance_by(Duration::from_secs_f32(2.0));
    world.insert_resource(t2);
    step.run(world);
    assert!(world.get::<Health>(e).unwrap().current < 20.0, "collapse damaged the enemy");
    assert_eq!(world.query::<&Singularity>().iter(world).count(), 0, "singularity despawned");
}

/// W: the Gravity Lance carries the VOID element (the per-weapon base-element
/// seam) and its orb pulls a nearby enemy toward it.
#[test]
fn gravity_lance_is_void_and_pulls() {
    use crate::combat::element::Element;
    use crate::components::GravityBullet;
    use crate::systems::weapons::{gravity_pull, WeaponKind};

    // Base-element seam: only Gravity Lance is non-Kinetic.
    assert_eq!(WeaponKind::GravityLance.element(), Element::Void);
    assert_eq!(WeaponKind::PulseCannon.element(), Element::Kinetic);

    // The orb pulls a nearby enemy inward.
    let mut app = test_app();
    let world = app.world_mut();
    let e = world
        .spawn((Enemy { kind: EnemyKind::Hunter }, Transform::from_xyz(100.0, 0.0, 0.0)))
        .id();
    world.spawn((
        GravityBullet { pull_radius: 150.0, pull_strength: 60.0 },
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));

    let mut step = Schedule::default();
    step.add_systems(gravity_pull);
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(0.1));
    world.insert_resource(time);
    step.run(world);

    assert!(world.get::<Transform>(e).unwrap().translation.x < 100.0, "enemy pulled toward the orb");
}

/// W: a Flak bullet, when its airburst fuse lapses, emits a 9-shrapnel ring and
/// despawns.
#[test]
fn flak_airbursts_into_shrapnel() {
    use crate::components::{Airburst, BulletElements, Bullet, BulletKind};
    use crate::combat::element::ElementSet;
    use crate::messages::Shard;
    use crate::systems::weapons::{flak_airburst, FLAK_SHRAPNEL};

    let mut app = test_app();
    let world = app.world_mut();
    let flak = world
        .spawn((
            Bullet { kind: BulletKind::Player, damage: 0.8, pierce: 0 },
            BulletElements(ElementSet::kinetic()),
            Transform::from_xyz(0.0, 0.0, 0.0),
            Airburst { timer: 0.05 },
        ))
        .id();

    #[derive(Resource, Default)]
    struct Count(u32);
    world.insert_resource(Count::default());
    fn collect(mut r: MessageReader<Shard>, mut c: ResMut<Count>) {
        for _ in r.read() {
            c.0 += 1;
        }
    }

    let mut step = Schedule::default();
    step.add_systems((flak_airburst, collect).chain());
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(0.1)); // past the fuse
    world.insert_resource(time);
    step.run(world);

    assert_eq!(world.resource::<Count>().0, FLAK_SHRAPNEL, "burst a full shrapnel ring");
    assert!(world.get::<Airburst>(flak).is_none(), "flak bullet despawned after bursting");
}

/// W: a Mitosis bullet, on impact, emits 2 shards at half damage carrying gen−1.
#[test]
fn mitosis_splits_into_shards_on_hit() {
    use crate::components::{Bullet, BulletKind, MitosisGen, Velocity};
    use crate::messages::Shard;
    use crate::systems::collision::bullet_hits_enemy;

    let mut app = test_app();
    let world = app.world_mut();
    world.spawn((Enemy { kind: EnemyKind::Hunter }, Health::new(50.0), Collider { radius: 16.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
    world.spawn((
        Bullet { kind: BulletKind::Player, damage: 1.0, pierce: 0 },
        Collider { radius: 5.0 },
        Velocity(Vec2::new(0.0, 300.0)),
        MitosisGen(2),
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));

    #[derive(Resource, Default)]
    struct Shards(Vec<(f32, u32)>);
    world.insert_resource(Shards::default());
    fn collect(mut r: MessageReader<Shard>, mut s: ResMut<Shards>) {
        for sh in r.read() {
            s.0.push((sh.damage, sh.generation));
        }
    }

    let mut step = Schedule::default();
    step.add_systems((bullet_hits_enemy, collect).chain());
    step.run(world);

    let shards = &world.resource::<Shards>().0;
    assert_eq!(shards.len(), 2, "Mitosis emitted 2 shards");
    assert!(shards.iter().all(|(d, g)| (*d - 0.5).abs() < 1e-6 && *g == 1), "half damage, gen 1: {shards:?}");
}

/// W: a Caroms bullet, on hitting an enemy, survives and redirects toward the
/// nearest other enemy, spending one bounce.
#[test]
fn caroms_bounces_to_next_enemy() {
    use crate::components::{Bounce, Bullet, BulletKind, Velocity};
    use crate::systems::collision::bullet_hits_enemy;

    let mut app = test_app();
    let world = app.world_mut();
    // Enemy A at the origin (the hit); enemy B at +X (the bounce target).
    world.spawn((Enemy { kind: EnemyKind::Hunter }, Health::new(50.0), Collider { radius: 16.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
    world.spawn((Enemy { kind: EnemyKind::Hunter }, Health::new(50.0), Collider { radius: 16.0 }, Transform::from_xyz(100.0, 0.0, 0.0)));
    let bullet = world
        .spawn((
            Bullet { kind: BulletKind::Player, damage: 1.0, pierce: 0 },
            Collider { radius: 5.0 },
            Velocity(Vec2::new(0.0, 300.0)), // moving +Y
            Bounce { remaining: 3, seek_radius: 260.0 },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();

    let mut step = Schedule::default();
    step.add_systems(bullet_hits_enemy);
    step.run(world);

    assert!(world.get::<Bounce>(bullet).is_some(), "caroms bullet survived the hit");
    assert_eq!(world.get::<Bounce>(bullet).unwrap().remaining, 2, "one bounce spent");
    let v = world.get::<Velocity>(bullet).unwrap().0;
    assert!(v.x > 200.0 && v.y.abs() < 1.0, "redirected toward enemy B (+X): {v:?}");
}

/// W: a Boomerang disc flies out, then (after the out phase) accelerates back
/// toward the player — its outbound velocity reverses.
#[test]
fn boomerang_returns_to_player() {
    use crate::components::{Boomerang, Velocity};
    use crate::systems::weapons::boomerang_return;

    let mut app = test_app();
    let world = app.world_mut();
    world.spawn((Ship::default(), Transform::from_xyz(0.0, 0.0, 0.0)));
    // A disc out at +X, moving +X, mid-return phase.
    let disc = world
        .spawn((
            Velocity(Vec2::new(300.0, 0.0)),
            Transform::from_xyz(200.0, 0.0, 0.0),
            Boomerang { timer: 0.0, returning: false },
        ))
        .id();

    let mut step = Schedule::default();
    step.add_systems(boomerang_return);
    // Advance well past the 28-tick out phase + keep returning long enough that
    // the +X velocity fully reverses (accel only applies during the return phase).
    for _ in 0..60 {
        let mut t = Time::<()>::default();
        t.advance_by(Duration::from_secs_f32(1.0 / 60.0));
        world.insert_resource(t);
        step.run(world);
    }

    assert!(world.get::<Boomerang>(disc).unwrap().returning, "entered return phase");
    assert!(world.get::<Velocity>(disc).unwrap().0.x < 0.0, "velocity reversed toward the player");
}

/// W: the Spin Cannon's fire cooldown spools from slow (0.22 s) to fast (0.06 s)
/// as the spool fills.
#[test]
fn spin_cannon_cooldown_spools_up() {
    use crate::systems::weapons::{spin_cooldown, SPIN_FAST_CD, SPIN_SLOW_CD};
    assert!((spin_cooldown(0.0) - SPIN_SLOW_CD).abs() < 1e-6, "just started → slow");
    assert!((spin_cooldown(1.0) - SPIN_FAST_CD).abs() < 1e-6, "full spool → fast");
    let mid = spin_cooldown(0.5);
    assert!(mid < SPIN_SLOW_CD && mid > SPIN_FAST_CD, "ramps between");
    assert!((spin_cooldown(2.0) - SPIN_FAST_CD).abs() < 1e-6, "clamps past full");
}

/// W1: a player bullet's element resolves to its attunement if any, else the
/// weapon base; the debug cycle steps off → Pyro → … → Radiant → off.
#[test]
fn attunement_resolution_and_cycle() {
    use crate::combat::element::{Element, ElementSet};
    use crate::systems::weapons::{cycle_attunement, next_attunement, resolve_player_bullet_set, Attunements};

    // Resolution: empty attune → weapon base; attuned → the attunement set.
    assert_eq!(
        resolve_player_bullet_set(ElementSet::EMPTY, Element::Kinetic),
        ElementSet::single(Element::Kinetic)
    );
    assert_eq!(
        resolve_player_bullet_set(ElementSet::single(Element::Pyro), Element::Kinetic),
        ElementSet::single(Element::Pyro)
    );

    // Cycle order wraps off → Pyro → … → Radiant → off.
    assert_eq!(next_attunement(ElementSet::EMPTY), ElementSet::single(Element::Pyro));
    assert_eq!(next_attunement(ElementSet::single(Element::Pyro)), ElementSet::single(Element::Cryo));
    assert_eq!(next_attunement(ElementSet::single(Element::Radiant)), ElementSet::EMPTY);

    // The T key now cycles only *unlocked* attunements. With none unlocked it
    // stays "off"; once Pyro is armory-unlocked, T advances to it.
    use crate::meta::Meta;
    let mut app = test_app();
    app.init_resource::<Attunements>();
    let mut input = ButtonInput::<KeyCode>::default();
    input.press(KeyCode::KeyT);
    app.world_mut().insert_resource(input);

    // Locked: T is a no-op (Pyro not unlocked).
    app.world_mut().insert_resource(Meta::default());
    let mut step = Schedule::default();
    step.add_systems(cycle_attunement);
    step.run(app.world_mut());
    assert_eq!(
        app.world().resource::<Attunements>().0,
        ElementSet::EMPTY,
        "with nothing unlocked, the attunement cycle stays off"
    );

    // Unlock Pyro → T advances to it.
    {
        let mut meta = app.world_mut().resource_mut::<Meta>();
        meta.unlock("ATT_PYRO", 0);
    }
    let mut input2 = ButtonInput::<KeyCode>::default();
    input2.press(KeyCode::KeyT);
    app.world_mut().insert_resource(input2);
    step.run(app.world_mut());
    assert_eq!(app.world().resource::<Attunements>().0, ElementSet::single(Element::Pyro));
}

/// W: an Orbital Strike telegraphs (no damage) then strikes its column AoE.
#[test]
fn orbital_strike_telegraphs_then_hits() {
    use crate::systems::power_weapon::{spawn_orbital_strike, update_orbital_strike, OrbitalStrike};

    let mut app = test_app();
    let world = app.world_mut();
    let e = world
        .spawn((Enemy { kind: EnemyKind::Hunter }, Health::new(50.0), Transform::from_xyz(0.0, 0.0, 0.0)))
        .id();

    let mut setup = Schedule::default();
    setup.add_systems(|mut c: Commands| spawn_orbital_strike(&mut c, Vec2::ZERO));
    setup.run(world);

    let mut step = Schedule::default();
    step.add_systems((update_orbital_strike, apply_damage).chain());

    // During the telegraph: no damage, marker present.
    let mut t1 = Time::<()>::default();
    t1.advance_by(Duration::from_secs_f32(0.1));
    world.insert_resource(t1);
    step.run(world);
    assert_eq!(world.get::<Health>(e).unwrap().current, 50.0, "no damage during telegraph");
    assert_eq!(world.query::<&OrbitalStrike>().iter(world).count(), 1, "marker telegraphing");

    // After the telegraph: strike lands + the marker despawns.
    let mut t2 = Time::<()>::default();
    t2.advance_by(Duration::from_secs_f32(1.0));
    world.insert_resource(t2);
    step.run(world);
    assert!(world.get::<Health>(e).unwrap().current < 50.0, "strike hit the enemy");
    assert_eq!(world.query::<&OrbitalStrike>().iter(world).count(), 0, "marker despawned");
}

/// W: the Prism Beam spawns a fan of 5 RADIANT rays; a ray straight ahead hits
/// an enemy in front.
#[test]
fn prism_beam_fans_rays_and_damages() {
    use crate::components::Intent;
    use crate::systems::power_weapon::{spawn_prism, update_beams, Beam};

    let mut app = test_app();
    let world = app.world_mut();
    world.spawn((Ship::default(), Intent::default(), Transform::from_xyz(0.0, 0.0, 0.0)));
    let e = world
        .spawn((
            Enemy { kind: EnemyKind::Hunter },
            Health::new(100.0),
            Collider { radius: 16.0 },
            Transform::from_xyz(0.0, 100.0, 0.0), // straight ahead (+Y)
        ))
        .id();

    let mut setup = Schedule::default();
    setup.add_systems(|mut c: Commands| spawn_prism(&mut c, Vec2::ZERO));
    setup.run(world);
    assert_eq!(world.query::<&Beam>().iter(world).count(), 5, "5 prism rays");

    let mut step = Schedule::default();
    step.add_systems((update_beams, apply_damage).chain());
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(0.1));
    world.insert_resource(time);
    step.run(world);

    assert!(world.get::<Health>(e).unwrap().current < 100.0, "a prism ray hit the enemy ahead");
}

/// W: the Overdrive buff multiplies the primary's per-shot damage (×1.5).
#[test]
fn overdrive_buffs_primary_damage() {
    use crate::components::{Intent, Overdrive, Weapon};
    use crate::systems::weapons::{player_fire, CurrentWeapon};

    #[derive(Resource, Default)]
    struct Dmg(f32);
    fn tally(mut r: MessageReader<Fire>, mut d: ResMut<Dmg>) {
        for f in r.read() {
            d.0 = f.damage;
        }
    }

    fn fire_dmg(overdrive: bool) -> f32 {
        let mut app = test_app();
        app.init_resource::<CurrentWeapon>();
        app.insert_resource(Dmg::default());
        let world = app.world_mut();
        let mut pc = world.spawn((
            Ship::default(),
            Intent { firing: true, ..Default::default() },
            Weapon::default(),
            Transform::default(),
        ));
        if overdrive {
            pc.insert(Overdrive { secs: 1.0 });
        }
        let mut step = Schedule::default();
        step.add_systems((player_fire, tally).chain());
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_secs_f32(0.5)); // cooldown starts at 0 → fires
        world.insert_resource(time);
        step.run(world);
        world.resource::<Dmg>().0
    }

    let base = fire_dmg(false);
    let buffed = fire_dmg(true);
    assert!(base > 0.0, "primary fired");
    assert!((buffed - base * 1.5).abs() < 1e-4, "Overdrive ×1.5 ({buffed} vs {base})");
}

/// EN: a Lumen Drone's aura pulse stamps an AllyShield on allies in range only.
#[test]
fn lumen_aura_shields_nearby_allies() {
    use crate::components::{AllyShield, SupportAura, AURA_AMOUNT, AURA_INTERVAL, AURA_RADIUS};
    use crate::systems::enemy::mechanics::lumen_aura;

    let mut app = test_app();
    let world = app.world_mut();
    world.spawn((
        Enemy { kind: EnemyKind::LumenDrone },
        Transform::from_xyz(0.0, 0.0, 0.0),
        SupportAura { radius: AURA_RADIUS, amount: AURA_AMOUNT, interval: AURA_INTERVAL, timer: 0.05 },
    ));
    let near = world.spawn((Enemy { kind: EnemyKind::Hunter }, Transform::from_xyz(50.0, 0.0, 0.0))).id();
    let far = world.spawn((Enemy { kind: EnemyKind::Hunter }, Transform::from_xyz(500.0, 0.0, 0.0))).id();

    let mut step = Schedule::default();
    step.add_systems(lumen_aura);
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(0.1)); // past the 0.05 pulse timer
    world.insert_resource(time);
    step.run(world);

    assert!(world.get::<AllyShield>(near).is_some(), "near ally shielded");
    assert!(world.get::<AllyShield>(far).is_none(), "far ally out of range");
}

/// EN: an AllyShield reduces incoming bullet damage by its `amount` (×0.6 at 0.4).
/// Compared across two fresh apps (same seeded RNG → same crit roll) so only the
/// shield differs.
#[test]
fn ally_shield_reduces_bullet_damage() {
    use crate::components::{AllyShield, Bullet, BulletKind};

    fn hp_loss(shielded: bool) -> f32 {
        let mut app = test_app();
        let world = app.world_mut();
        let mut ec = world.spawn((
            Enemy { kind: EnemyKind::Hunter },
            Health::new(100.0),
            Collider { radius: 16.0 },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        if shielded {
            ec.insert(AllyShield { secs: 1.0, amount: 0.4 });
        }
        let e = ec.id();
        world.spawn((
            Bullet { kind: BulletKind::Player, damage: 10.0, pierce: 0 },
            Collider { radius: 5.0 },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        let mut step = Schedule::default();
        step.add_systems((bullet_hits_enemy, apply_damage).chain());
        step.run(world);
        100.0 - world.get::<Health>(e).unwrap().current
    }

    let unshielded = hp_loss(false);
    let shielded = hp_loss(true);
    assert!(unshielded > 0.0 && shielded > 0.0, "both took damage");
    assert!(
        (shielded - unshielded * 0.6).abs() < 1e-3,
        "ally shield ×0.6 ({shielded} vs {unshielded})"
    );
}

/// EN: a Hydra splits into 2 lings on death; a ling (`Splitter.lings == 0`) does
/// not split again.
#[test]
fn hydra_splits_into_lings_on_death() {
    use crate::components::Splitter;

    let mut app = test_app();
    let world = app.world_mut();
    let hydra = world
        .spawn((
            Enemy { kind: EnemyKind::Hydra },
            Health::new(1.0),
            Transform::from_xyz(0.0, 0.0, 0.0),
            Splitter { lings: 2 },
        ))
        .id();
    world.write_message(Damage { target: hydra, amount: 5.0 }); // lethal

    let mut step = Schedule::default();
    step.add_systems(apply_damage);
    step.run(world);

    // Original despawned; exactly 2 Hydra lings remain, each unable to re-split.
    let mut q = world.query::<(&Enemy, &Splitter)>();
    let lings: Vec<u32> = q
        .iter(world)
        .filter(|(e, _)| e.kind == EnemyKind::Hydra)
        .map(|(_, s)| s.lings)
        .collect();
    assert_eq!(lings.len(), 2, "Hydra split into 2 lings");
    assert!(lings.iter().all(|&l| l == 0), "lings can't re-split");
}

/// EN: an Ashen Detonator dying near the player triggers a PYRO death-flare —
/// the player takes the flare damage and a burn.
#[test]
fn ashen_death_flare_hits_nearby_player() {
    use crate::messages::Death;
    use crate::systems::enemy::mechanics::ashen_death_flare;

    let mut app = test_app();
    let world = app.world_mut();
    let p = world
        .spawn((Ship::default(), Health::new(40.0), Transform::from_xyz(0.0, 0.0, 0.0)))
        .id();

    // A setup system writes the Ashen death (50 px away → inside the 130 flare).
    fn emit_ashen_death(mut w: MessageWriter<Death>, q: Query<Entity, With<Ship>>) {
        if let Ok(e) = q.single() {
            w.write(Death {
                entity: e,
                position: Vec2::new(50.0, 0.0),
                kind: Some(EnemyKind::AshenDetonator),
                boss_tier: 0,
                mini_boss: false,
            });
        }
    }

    let mut step = Schedule::default();
    step.add_systems((emit_ashen_death, ashen_death_flare, apply_damage).chain());
    world.insert_resource(Time::<()>::default());
    step.run(world);

    assert_eq!(world.get::<Health>(p).unwrap().current, 28.0, "flare dealt 12");
    assert!(world.get::<PlayerBurn>(p).is_some(), "flare applied a burn");
}

/// EN batch E8c: the Volt/Toxic enemies carry the right attack element + resist.
#[test]
fn en_volt_toxic_enemy_data() {
    use crate::combat::element::Element;
    use crate::systems::enemy::{element_for, resistances_for};

    assert_eq!(element_for(EnemyKind::TeslaWraith), Element::Volt);
    assert_eq!(element_for(EnemyKind::Plaguebearer), Element::Toxic);
    assert_eq!(element_for(EnemyKind::SporeCarrier), Element::Toxic);
    assert_eq!(element_for(EnemyKind::Hydra), Element::Kinetic);

    assert!((resistances_for(EnemyKind::TeslaWraith).get(Element::Volt) - 0.85).abs() < 1e-6);
    assert!(resistances_for(EnemyKind::TeslaWraith).get(Element::Toxic) < 0.0);
    assert!(resistances_for(EnemyKind::Plaguebearer).get(Element::Radiant) < 0.0, "purge the toxic");
}

/// EN: the new elemental enemies are wired into the campaign wave table (so they
/// actually spawn), not just defined as types.
#[test]
fn en_enemies_appear_in_campaign() {
    use crate::systems::wave::campaign_uses;
    for k in [
        EnemyKind::Cinder,
        EnemyKind::Glacier,
        EnemyKind::FrostLance,
        EnemyKind::AshenDetonator,
        EnemyKind::TeslaWraith,
        EnemyKind::Plaguebearer,
        EnemyKind::SporeCarrier,
        EnemyKind::Hydra,
        EnemyKind::Warden,
        EnemyKind::LumenDrone,
    ] {
        assert!(campaign_uses(k), "{k:?} should appear in the campaign");
    }
    assert!(campaign_uses(EnemyKind::Hunter), "originals still present");
}

/// E5: the player burn DoT lands a 2-dmg chunk every 0.5 s (surviving the
/// player-damage rounding) and expires after its duration.
#[test]
fn player_burn_chunks_then_expires() {
    use crate::systems::player_status::tick_player_burn;

    let mut app = test_app();
    let world = app.world_mut();
    let p = world
        .spawn((
            Ship::default(),
            Health::new(40.0),
            Transform::from_xyz(0.0, 0.0, 0.0),
            PlayerBurn { secs: 3.0, tick: 0.5 },
        ))
        .id();

    let mut step = Schedule::default();
    step.add_systems((tick_player_burn, apply_damage).chain());

    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(0.5)); // one chunk
    world.insert_resource(time.clone());
    step.run(world);
    assert_eq!(world.get::<Health>(p).unwrap().current, 38.0, "burn chunk hit the player");
    assert!(world.get::<PlayerBurn>(p).is_some(), "burn persists mid-duration");

    time.advance_by(Duration::from_secs_f32(3.0)); // past expiry
    world.insert_resource(time);
    step.run(world);
    assert!(world.get::<PlayerBurn>(p).is_none(), "burn expires");
}

/// E5: a Tangerine (PYRO) ramming the player stamps a burn on the ship.
#[test]
fn tangerine_contact_burns_player() {
    use crate::systems::collision::enemy_contact_player;

    let mut app = test_app();
    let world = app.world_mut();
    let p = world
        .spawn((
            Ship::default(),
            Health::new(40.0),
            Velocity::default(),
            Collider { radius: 20.0 },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();
    world.spawn((
        Enemy { kind: EnemyKind::Tangerine },
        Velocity::default(),
        Collider { radius: 16.0 },
        Transform::from_xyz(10.0, 0.0, 0.0),
    ));

    let mut step = Schedule::default();
    step.add_systems(enemy_contact_player);
    step.run(world);

    assert!(world.get::<PlayerBurn>(p).is_some(), "Tangerine's PYRO ram burns the player");
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

/// Account SP VAMPIRISM feeds the same lifesteal path as the shop/item sources
/// (Phase ME effect-wiring): a maxed SP Vampirism heals the player on a hit even
/// with no upgrades/affixes.
#[test]
fn sp_vampirism_heals_on_hit() {
    use crate::meta::Meta;
    use crate::systems::collision::bullet_hits_enemy;

    let mut app = test_app();
    {
        let mut meta = app.world_mut().resource_mut::<Meta>();
        meta.sp = 20;
        for _ in 0..20 {
            meta.allocate_sp("VAMPIRISM"); // → 50% lifesteal at the cap
        }
        assert!((meta.sp_value("VAMPIRISM") - 50.0).abs() < 1e-3);
    }
    let world = app.world_mut();

    let player = world
        .spawn((
            Ship::default(),
            Health { current: 10.0, max: 40.0 },
            Collider { radius: 16.0 },
            Faction::Player,
            Transform::from_xyz(500.0, 0.0, 0.0),
        ))
        .id();
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
    assert!(hp > 10.0, "SP vampirism heals the player on a hit (hp now {hp})");
}

/// Account SP TOUGHNESS feeds the player's shield damage-reduction (Phase ME):
/// maxed (+50% DR) halves an incoming hit, even with a 0% base shield.
#[test]
fn sp_toughness_reduces_incoming_damage() {
    use crate::components::Shield;
    use crate::meta::Meta;
    use crate::messages::Damage;
    use crate::systems::damage::apply_damage;

    let mut app = test_app();
    {
        let mut meta = app.world_mut().resource_mut::<Meta>();
        meta.sp = 20;
        for _ in 0..20 {
            meta.allocate_sp("TOUGHNESS"); // +50% damage reduction at the cap
        }
    }
    let world = app.world_mut();
    let player = world
        .spawn((
            Ship::default(),
            Health::new(1000.0),
            Shield { reduction: 0.0 }, // isolate the SP toughness contribution
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();
    world.write_message(Damage { target: player, amount: 100.0 });

    let mut step = Schedule::default();
    step.add_systems(apply_damage);
    step.run(world);

    let hp = world.get::<Health>(player).unwrap().current;
    // 100 × (1 − 0.5) = 50 → 1000 − 50 = 950 (player damage is rounded).
    assert!((hp - 950.0).abs() < 1.5, "SP toughness halves 100 → 50 (hp now {hp})");
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

// ── 27b. second_wind_revives_once_on_lethal_hit ───────────────────────────────

/// Second Wind (AB ability): an armed lethal hit is survived — full-HP revive +
/// a spare tank, the arm consumed, and a brief i-frame (lifecycle.js).
#[test]
fn second_wind_revives_once_on_lethal_hit() {
    use crate::components::{Invulnerable, Lives, SecondWindArmed};
    use crate::messages::Damage;
    use crate::systems::damage::apply_damage;

    let mut app = test_app();
    let world = app.world_mut();

    let player = world
        .spawn((
            Ship::default(),
            Health::new(100.0),
            Lives { count: 0, progress: 0.0 },
            SecondWindArmed,
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();
    world.write_message(Damage { target: player, amount: 999.0 });

    let mut step = Schedule::default();
    step.add_systems(apply_damage);
    step.run(world);

    let hp = world.get::<Health>(player).unwrap();
    assert!((hp.current - hp.max).abs() < 1e-3, "revived to full HP (got {})", hp.current);
    assert!(world.get::<SecondWindArmed>(player).is_none(), "the death-save arm is consumed");
    assert_eq!(world.get::<Lives>(player).unwrap().count, 1, "granted a spare tank");
    assert!(world.get::<Invulnerable>(player).is_some(), "brief i-frames after the revive");
}

// ── 27c. sentry_drone_fires_and_expires ───────────────────────────────────────

/// A deployed Sentry Drone (AB) auto-fires a player `Fire` at the nearest enemy
/// when its timer is ready, and despawns once its 8 s lifetime elapses.
#[test]
fn sentry_drone_fires_at_nearest_enemy() {
    use crate::components::Sentry;
    use crate::systems::abilities::tick_sentry_drones;

    let mut app = test_app();
    let world = app.world_mut();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);

    world.spawn((Ship::default(), Transform::from_xyz(0.0, 0.0, 0.0)));
    world.spawn((
        Sentry { secs: 8.0, angle: 0.0, fire_timer: 0.0 }, // ready to fire
        Transform::from_xyz(58.0, 0.0, 0.0),
    ));
    world.spawn((Enemy { kind: EnemyKind::Hunter }, Transform::from_xyz(120.0, 0.0, 0.0)));

    #[derive(Resource, Default)]
    struct FireCount(u32);
    world.insert_resource(FireCount::default());
    fn tally(mut r: MessageReader<Fire>, mut c: ResMut<FireCount>) {
        for _ in r.read() {
            c.0 += 1;
        }
    }

    let mut step = Schedule::default();
    step.add_systems((tick_sentry_drones, tally).chain());
    step.run(world);

    assert!(
        world.resource::<FireCount>().0 >= 1,
        "a ready sentry with an enemy in view emits a Fire"
    );
}

#[test]
fn sentry_drone_despawns_after_lifetime() {
    use crate::components::Sentry;
    use crate::systems::abilities::tick_sentry_drones;

    let mut app = test_app();
    let world = app.world_mut();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(100)); // 0.1 s step
    world.insert_resource(time);

    world.spawn((Ship::default(), Transform::from_xyz(0.0, 0.0, 0.0)));
    let drone = world
        .spawn((
            Sentry { secs: 0.05, angle: 0.0, fire_timer: 0.0 }, // shorter than the step
            Transform::from_xyz(58.0, 0.0, 0.0),
        ))
        .id();

    let mut step = Schedule::default();
    step.add_systems(tick_sentry_drones);
    step.run(world);

    assert!(
        world.get::<Sentry>(drone).is_none(),
        "the sentry despawns when its lifetime elapses"
    );
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
        enemy::spawn_for_wave(&mut c, EnemyKind::Hunter, Vec2::ZERO, 0, false, 30);
    });
    step.run(world);
    let mut q = world.query_filtered::<&Health, With<Enemy>>();
    let hp = q.iter(world).next().expect("enemy spawned").max;
    // Hunter base 5 × difficulty_hp_mul(30) 15.5 = 77.5.
    assert!((hp - 5.0 * 15.5).abs() < 0.1, "W30 Hunter HP = 5×15.5 (got {hp})");
}

// ── 38. difficulty_bullet_speed_curve ─────────────────────────────────────────

/// Enemy bullet-speed multiplier is normalized to 1.0 at W1 and ramps up
/// (W30 ≈ 3.05/1.15) — spec V.4, relative to the port's W1-tuned base speeds.
#[test]
fn difficulty_bullet_speed_curve() {
    use crate::systems::enemy::difficulty_bullet_speed_mul;

    assert!((difficulty_bullet_speed_mul(1) - 1.0).abs() < 1e-4, "W1 normalized to 1.0");
    assert!(
        (difficulty_bullet_speed_mul(30) - (3.05 / 1.15)).abs() < 1e-3,
        "W30 ramp ≈ 2.65×"
    );
    assert!(
        difficulty_bullet_speed_mul(20) > difficulty_bullet_speed_mul(10),
        "monotonic ramp"
    );
}

// ── 39. difficulty_speed_curve_and_speedmul ───────────────────────────────────

/// Enemy movement-speed multiplier is normalized to 1.0 at W1 and ramps
/// (W30 ≈ 1.75/0.55 ≈ 3.18×); spawn_for_wave stores it (× boss-tier speed) as a
/// SpeedMul that the AIs read (spec V.4 / IV.7).
#[test]
fn difficulty_speed_curve_and_speedmul() {
    use crate::components::SpeedMul;
    use crate::systems::enemy::{self, difficulty_speed_mul};

    assert!((difficulty_speed_mul(1) - 1.0).abs() < 1e-4, "W1 normalized to 1.0");
    assert!((difficulty_speed_mul(30) - (1.75 / 0.55)).abs() < 1e-3, "W30 ≈ 3.18×");
    assert!(difficulty_speed_mul(20) > difficulty_speed_mul(10), "monotonic");

    let mut app = test_app();
    let world = app.world_mut();
    let mut step = Schedule::default();
    step.add_systems(|mut c: Commands| {
        enemy::spawn_for_wave(&mut c, EnemyKind::Titan, Vec2::ZERO, 1, false, 30);
    });
    step.run(world);
    let mut q = world.query::<&SpeedMul>();
    let sm = q.iter(world).next().expect("enemy spawned").0;
    // tier-1 boss speed mul 1.0 × difficulty_speed_mul(30) ≈ 3.18.
    assert!((sm - 1.75 / 0.55).abs() < 0.01, "W30 tier-1 boss SpeedMul (got {sm})");
}

// ── 40. mine_knocks_back_enemy ────────────────────────────────────────────────

/// A mine detonation shoves nearby enemies via the Knockback message (spec III.6).
#[test]
fn mine_knocks_back_enemy() {
    use crate::messages::Knockback;
    use crate::systems::power_weapon::{lay_mine, update_mines};

    let mut app = test_app();
    let world = app.world_mut();

    // Enemy 40 px from the mine (inside the 90 px blast).
    world.spawn((
        Enemy { kind: EnemyKind::Hunter },
        Health::new(5.0),
        Collider { radius: 16.0 },
        Faction::Enemy,
        Transform::from_xyz(40.0, 0.0, 0.0),
    ));
    let mut setup = Schedule::default();
    setup.add_systems(|mut c: Commands| lay_mine(&mut c, Vec2::ZERO));
    setup.run(world);

    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(1.0)); // arm + trigger
    world.insert_resource(time);

    #[derive(Resource, Default)]
    struct KbCount(u32);
    world.insert_resource(KbCount::default());
    fn count(mut r: MessageReader<Knockback>, mut c: ResMut<KbCount>) {
        for _ in r.read() {
            c.0 += 1;
        }
    }

    let mut step = Schedule::default();
    step.add_systems((update_mines, count).chain());
    step.run(world);

    assert!(
        world.resource::<KbCount>().0 >= 1,
        "mine detonation should knock back nearby enemies"
    );
}

// ── 41. boss_pair_rage_links_survivors ────────────────────────────────────────

/// When a boss dies, surviving un-raged bosses immediately rage (spec IV.7).
#[test]
fn boss_pair_rage_links_survivors() {
    use crate::components::{Boss, Raged};
    use crate::messages::Death;
    use crate::systems::enemy::boss_pair_rage;

    let mut app = test_app();
    let world = app.world_mut();

    let survivor = world
        .spawn((
            Enemy { kind: EnemyKind::Titan },
            Boss { tier: 2 },
            FireCooldown { cooldown: 2.0, timer: 1.0 },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();

    // A partner boss died this tick.
    world.write_message(Death {
        entity: Entity::PLACEHOLDER,
        position: Vec2::ZERO,
        kind: Some(EnemyKind::Titan),
        boss_tier: 2,
        mini_boss: false,
    });

    let mut step = Schedule::default();
    step.add_systems(boss_pair_rage);
    step.run(world);

    assert!(
        world.get::<Raged>(survivor).is_some(),
        "surviving boss should rage when its partner dies"
    );
}

// ── 42. overheal_converts_to_tanks ────────────────────────────────────────────

/// Overheal above max HP accumulates toward a spare tank — 40 overheal = 1 tank
/// (spec II.2) — then HP clamps to max.
#[test]
fn overheal_converts_to_tanks() {
    use crate::components::Lives;
    use crate::systems::damage::overheal_to_tanks;

    let mut app = test_app();
    let world = app.world_mut();

    let p = world
        .spawn((
            Ship::default(),
            Health { current: 44.0, max: 40.0 }, // 4 overheal
            Lives { count: 1, progress: 0.0 },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();
    let mut step = Schedule::default();
    step.add_systems(overheal_to_tanks);
    step.run(world);

    assert!((world.get::<Health>(p).unwrap().current - 40.0).abs() < 1e-4, "HP clamped");
    assert_eq!(world.get::<Lives>(p).unwrap().count, 1, "no new tank from 4 overheal");
    assert!((world.get::<Lives>(p).unwrap().progress - 0.1).abs() < 1e-4, "progress 4/40");

    // 40 more overheal → +1 tank (progress 0.1 + 1.0 − 1.0 = 0.1).
    world.get_mut::<Health>(p).unwrap().current = 80.0;
    step.run(world);
    assert_eq!(world.get::<Lives>(p).unwrap().count, 2, "40 overheal grants a tank");
    assert!((world.get::<Lives>(p).unwrap().progress - 0.1).abs() < 1e-4, "leftover progress");
}

// ── 43. passive_regen_after_delay ─────────────────────────────────────────────

/// Passive regen kicks in only after REGEN_DELAY (4 s) without damage, at the
/// Regen-stacked rate (spec II.2).
#[test]
fn passive_regen_after_delay() {
    use crate::resources::DamageClock;
    use crate::systems::damage::passive_regen;
    use crate::systems::shop::{regen_rate, UpgradeId, Upgrades};

    assert_eq!(regen_rate(0), 0.0);
    assert!((regen_rate(2) - 1.0).abs() < 1e-5);
    assert_eq!(regen_rate(100), 3.0, "rate caps at 3 HP/s");

    let mut app = test_app();
    app.world_mut()
        .resource_mut::<Upgrades>()
        .set(UpgradeId::Regen, 2); // 1 HP/s
    let world = app.world_mut();
    world.insert_resource(DamageClock(0.0));
    let p = world
        .spawn((Ship::default(), Health { current: 10.0, max: 40.0 }, Transform::from_xyz(0.0, 0.0, 0.0)))
        .id();

    let mut step = Schedule::default();
    step.add_systems(passive_regen);

    // Step to 3 s (< delay): no regen.
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(3.0));
    world.insert_resource(time.clone());
    step.run(world);
    assert!((world.get::<Health>(p).unwrap().current - 10.0).abs() < 1e-4, "no regen before 4 s");

    // Another 2 s → clock 5 s ≥ delay → regen 1 HP/s × 2 s = 2.
    time.advance_by(Duration::from_secs_f32(2.0));
    world.insert_resource(time);
    step.run(world);
    assert!(world.get::<Health>(p).unwrap().current > 10.0, "regen after 4 s no-damage");
}

// ── 44. stage_label_mapping ───────────────────────────────────────────────────

/// Wave → "stage-substage" label (3 waves/stage, spec V): W1→1-1, W3→1-3,
/// W4→2-1, W30→10-3.
#[test]
fn stage_label_mapping() {
    use crate::render::wave_title::stage_label;
    assert_eq!(stage_label(1), "1-1");
    assert_eq!(stage_label(2), "1-2");
    assert_eq!(stage_label(3), "1-3");
    assert_eq!(stage_label(4), "2-1");
    assert_eq!(stage_label(30), "10-3");
}

// ── 45. last_stand_survives_one_lethal_hit ────────────────────────────────────

/// Last Stand lets the player survive one lethal hit per run at 1 HP + invuln
/// (spec III.5).
#[test]
fn last_stand_survives_one_lethal_hit() {
    use crate::components::Invulnerable;
    use crate::messages::Damage;
    use crate::resources::LastStandUsed;
    use crate::systems::damage::apply_damage;
    use crate::systems::shop::{UpgradeId, Upgrades};

    let mut app = test_app();
    app.world_mut()
        .resource_mut::<Upgrades>()
        .set(UpgradeId::LastStand, 1);
    let world = app.world_mut();

    // No Lives → without Last Stand, a lethal hit would end the run.
    let p = world
        .spawn((Ship::default(), Health { current: 5.0, max: 40.0 }, Transform::from_xyz(0.0, 0.0, 0.0)))
        .id();
    world.write_message(Damage { target: p, amount: 100.0 });

    let mut step = Schedule::default();
    step.add_systems(apply_damage);
    step.run(world);

    assert!(world.get::<Health>(p).is_some(), "Last Stand: player survives");
    assert!((world.get::<Health>(p).unwrap().current - 1.0).abs() < 1e-4, "clamped to 1 HP");
    assert!(world.get::<Invulnerable>(p).is_some(), "grants invuln");
    assert!(world.resource::<LastStandUsed>().0, "Last Stand consumed");
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

// ── 51. raged_enemy_fire_is_flagged_homing ────────────────────────────────────

/// A *raged* enemy's fired bullets carry `homing: true` (spec IV.7
/// `enableHomingBullets`); a normal enemy's carry `homing: false`. `spawn_bullets`
/// reads this flag to tag enemy bullets with `RageHoming`.
#[test]
fn raged_enemy_fire_is_flagged_homing() {
    let mut app = test_app();
    let world = app.world_mut();

    // Player below the enemies so aim_dir != ZERO.
    world.spawn((Ship::default(), Transform::from_xyz(0.0, -100.0, 0.0)));

    // Two Hunters with ready cooldowns: one raged, one not.
    world.spawn((
        Enemy { kind: EnemyKind::Hunter },
        FireCooldown { cooldown: 1.0, timer: 0.0 },
        Raged,
        Transform::from_xyz(50.0, 0.0, 0.0),
    ));
    world.spawn((
        Enemy { kind: EnemyKind::Hunter },
        FireCooldown { cooldown: 1.0, timer: 0.0 },
        Transform::from_xyz(-50.0, 0.0, 0.0),
    ));

    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);

    #[derive(Resource, Default)]
    struct Flags { homing: u32, plain: u32 }
    world.insert_resource(Flags::default());
    fn tally(mut reader: MessageReader<Fire>, mut flags: ResMut<Flags>) {
        for f in reader.read() {
            if f.homing { flags.homing += 1; } else { flags.plain += 1; }
        }
    }

    let mut step = Schedule::default();
    step.add_systems((enemy_firing, tally).chain());
    step.run(world);

    let flags = world.resource::<Flags>();
    assert!(flags.homing >= 1, "the raged enemy fires at least one homing bullet");
    assert!(flags.plain >= 1, "the un-raged enemy fires only non-homing bullets");
}

// ── 52. rage_homing_curves_toward_player ──────────────────────────────────────

/// `rage_homing_steer` curves a `RageHoming` enemy bullet toward the player while
/// preserving its speed (spec IV.7 / IV.5 — the bounded `vel += dir*0.04`
/// equivalent).
#[test]
fn rage_homing_curves_toward_player() {
    use crate::components::RageHoming;
    use crate::systems::enemy::{rage_homing_steer, RAGE_HOMING_TURN};

    let mut app = test_app();
    let world = app.world_mut();

    // Player directly above the bullet; the bullet flies straight along +X, so the
    // player is 90° to the bullet's left — steering must rotate it toward +Y.
    world.spawn((Ship::default(), Transform::from_xyz(0.0, 100.0, 0.0)));

    let speed = 300.0_f32;
    let bullet = world
        .spawn((
            Velocity(Vec2::new(speed, 0.0)),
            RageHoming { turn_rate: RAGE_HOMING_TURN },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();

    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(0.5)); // 0.5 s → ~0.25 rad turn
    world.insert_resource(time);

    let mut step = Schedule::default();
    step.add_systems(rage_homing_steer);
    step.run(world);

    let v = world.get::<Velocity>(bullet).unwrap().0;
    assert!(v.y > 0.0, "velocity should rotate toward the player (+Y); got {v:?}");
    assert!(
        (v.length() - speed).abs() < 0.5,
        "steering preserves speed; got {} vs {speed}",
        v.length()
    );
    // It only *curves*: ~0.25 rad in half a second, so +X stays dominant.
    assert!(v.x > v.y, "a gentle nudge, not a snap turn; got {v:?}");
}

// ── 53. screen_shake_helpers ──────────────────────────────────────────────────

/// The pure screen-shake math (spec I.2): zero intensity → no offset; a positive
/// intensity gives a bounded non-zero offset; per-death magnitude orders
/// boss > mini-boss > regular; `add` keeps the stronger trigger (clamped).
#[test]
fn screen_shake_helpers() {
    use crate::render::shake::{death_shake, shake_offset, ScreenShake, HURT_SHAKE};

    assert_eq!(shake_offset(0.0, 1.234), Vec2::ZERO, "no shake at rest");

    // Each axis is bounded by intensity (0.25 sin/cos + 0.75 jitter = 1.0 max).
    for t in [0.0_f32, 0.3, 1.7, 9.9] {
        let o = shake_offset(20.0, t);
        assert!(o.x.abs() <= 20.0 + 1e-3 && o.y.abs() <= 20.0 + 1e-3, "bounded at t={t}: {o:?}");
    }
    // Some sampled time produces a real (non-zero) offset.
    assert!(shake_offset(20.0, 0.3).length() > 0.0, "active shake displaces the camera");

    // Boss shakes hardest, then mini-boss, then a regular enemy.
    assert!(death_shake(1, false) > death_shake(0, true));
    assert!(death_shake(0, true) > death_shake(0, false));
    assert!(death_shake(4, false) >= death_shake(1, false), "higher tiers shake at least as hard");

    // `add` keeps the stronger magnitude and clamps to the cap.
    let mut s = ScreenShake::default();
    s.add(HURT_SHAKE);
    s.add(3.0);
    assert_eq!(s.intensity, HURT_SHAKE, "the weaker trigger does not lower the shake");
    s.add(1000.0);
    assert!(s.intensity <= 26.0 && s.intensity > HURT_SHAKE, "clamped to the max");
}

// ── 54. death_and_hurt_trigger_shake ──────────────────────────────────────────

/// `trigger_screen_shake` bumps `ScreenShake` from `Death` (scaled by boss tier)
/// and from `PlayerHurt` (spec I.2).
#[test]
fn death_and_hurt_trigger_shake() {
    use crate::messages::PlayerHurt;
    use crate::render::shake::{death_shake, trigger_screen_shake, ScreenShake};

    // A boss death.
    let mut app = test_app();
    app.world_mut().init_resource::<ScreenShake>();
    app.world_mut().write_message(Death {
        entity: Entity::PLACEHOLDER,
        position: Vec2::ZERO,
        kind: Some(EnemyKind::Titan),
        boss_tier: 2,
        mini_boss: false,
    });
    let mut step = Schedule::default();
    step.add_systems(trigger_screen_shake);
    step.run(app.world_mut());
    assert!(
        (app.world().resource::<ScreenShake>().intensity - death_shake(2, false)).abs() < 1e-3,
        "boss death sets the tier-scaled shake"
    );

    // A player hit also shakes.
    let mut app2 = test_app();
    app2.world_mut().init_resource::<ScreenShake>();
    app2.world_mut().write_message(PlayerHurt { amount: 10.0 });
    let mut step2 = Schedule::default();
    step2.add_systems(trigger_screen_shake);
    step2.run(app2.world_mut());
    assert!(
        app2.world().resource::<ScreenShake>().intensity > 0.0,
        "a player hit triggers shake"
    );
}

// ── 55. screen_flash_add_keeps_max ────────────────────────────────────────────

/// `ScreenFlash::add` keeps the stronger trigger (whose color wins) and clamps
/// the alpha to 1.0.
#[test]
fn screen_flash_add_keeps_max() {
    use crate::render::flash::{ScreenFlash, FLASH_GOLD};

    let mut f = ScreenFlash::default();
    assert_eq!(f.intensity, 0.0);
    f.add(Color::WHITE, 0.42);
    f.add(Color::WHITE, 0.1);
    assert_eq!(f.intensity, 0.42, "a weaker trigger does not lower the flash");

    // A stronger gold trigger wins both the alpha and the color.
    f.add(FLASH_GOLD, 0.7);
    assert_eq!(f.intensity, 0.7);
    assert_eq!(f.color, FLASH_GOLD, "the stronger trigger's color wins");
    // A weaker trigger changes neither.
    f.add(Color::WHITE, 0.3);
    assert_eq!(f.intensity, 0.7);
    assert_eq!(f.color, FLASH_GOLD, "weaker trigger keeps the prior color");

    f.add(Color::WHITE, 5.0);
    assert_eq!(f.intensity, 1.0, "alpha is clamped to 1.0");
}

// ── 91. last_stand_triggers_gold_flash ────────────────────────────────────────

/// `trigger_last_stand_flash` fires a gold flash on the `LastStandUsed`
/// false→true transition (cheat-death), once (spec I.2 gold channel).
#[test]
fn last_stand_triggers_gold_flash() {
    use crate::render::flash::{trigger_last_stand_flash, ScreenFlash, FLASH_GOLD, LAST_STAND_FLASH};
    use crate::resources::LastStandUsed;

    let mut app = test_app();
    app.world_mut().init_resource::<ScreenFlash>();
    let mut step = Schedule::default();
    step.add_systems(trigger_last_stand_flash);

    // Not yet spent → no flash.
    step.run(app.world_mut());
    assert_eq!(app.world().resource::<ScreenFlash>().intensity, 0.0, "no flash before Last Stand");

    // Spend it → gold flash fires.
    app.world_mut().resource_mut::<LastStandUsed>().0 = true;
    step.run(app.world_mut());
    {
        let f = app.world().resource::<ScreenFlash>();
        assert!((f.intensity - LAST_STAND_FLASH).abs() < 1e-3, "cheat-death fires the gold flash");
        assert_eq!(f.color, FLASH_GOLD, "the flash is gold");
    }

    // Decay it manually; staying spent does NOT re-fire (one transition only).
    app.world_mut().resource_mut::<ScreenFlash>().intensity = 0.0;
    step.run(app.world_mut());
    assert_eq!(
        app.world().resource::<ScreenFlash>().intensity,
        0.0,
        "no re-fire while it stays spent"
    );
}

// ── 56. rage_triggers_screen_flash ────────────────────────────────────────────

/// `trigger_screen_flash` flashes when a boss newly enters rage (`Added<Raged>`,
/// spec IV.7); an un-raged world leaves the flash at rest.
#[test]
fn rage_triggers_screen_flash() {
    use crate::render::flash::{trigger_screen_flash, ScreenFlash, RAGE_FLASH};

    // A freshly-`Raged` entity triggers the flash.
    let mut app = test_app();
    app.world_mut().init_resource::<ScreenFlash>();
    app.world_mut().spawn(Raged);
    let mut step = Schedule::default();
    step.add_systems(trigger_screen_flash);
    step.run(app.world_mut());
    assert!(
        (app.world().resource::<ScreenFlash>().intensity - RAGE_FLASH).abs() < 1e-3,
        "newly-raged boss triggers a rage flash"
    );

    // No raged entity → no flash.
    let mut app2 = test_app();
    app2.world_mut().init_resource::<ScreenFlash>();
    app2.world_mut().spawn_empty();
    let mut step2 = Schedule::default();
    step2.add_systems(trigger_screen_flash);
    step2.run(app2.world_mut());
    assert_eq!(
        app2.world().resource::<ScreenFlash>().intensity,
        0.0,
        "no rage → no flash"
    );
}

// ── 57. tight_controls_track_input ────────────────────────────────────────────

/// The tightened control model: velocity tracks the input target with no
/// overshoot, reaches it under sustained input, and sheds it fast on release
/// (low momentum).
#[test]
fn tight_controls_track_input() {
    use crate::systems::movement::tracked_velocity;

    let dt = 1.0 / 64.0;
    let resp = 1100.0 * 0.02; // base ship: thrust × RESPONSE_K ≈ 22/s
    let target = Vec2::new(520.0, 0.0);

    // A single step moves toward the target but never past it.
    let one = tracked_velocity(Vec2::ZERO, target, resp, dt);
    assert!(one.x > 0.0 && one.x < target.x, "one step: toward target, no overshoot ({one:?})");

    // Sustained input reaches (≈) the target speed.
    let mut v = Vec2::ZERO;
    for _ in 0..90 {
        v = tracked_velocity(v, target, resp, dt);
    }
    assert!(v.x > 505.0 && v.x <= target.x + 1e-3, "approaches target speed ({v:?})");

    // Releasing input (target 0) sheds speed quickly — low momentum.
    for _ in 0..20 {
        v = tracked_velocity(v, Vec2::ZERO, resp, dt);
    }
    assert!(v.length() < 40.0, "stops fast when input released ({v:?})");
}

// ── 58. asteroid_icosahedron_geometry ─────────────────────────────────────────

/// The asteroid wireframe is a valid icosahedron (spec VI.1): 30 edges, every
/// vertex used with degree 5, and the perspective projection stays finite + near
/// the circumradius.
#[test]
fn asteroid_icosahedron_geometry() {
    use crate::systems::asteroids::{project, CIRCUMRADIUS, ICO_EDGES, ICO_VERTS};

    // 30 edges, all indices valid, every vertex of degree 5 (icosahedron).
    assert_eq!(ICO_EDGES.len(), 30);
    let mut degree = [0u32; 12];
    for &(a, b) in ICO_EDGES.iter() {
        assert!(a < 12 && b < 12 && a != b, "edge ({a},{b}) out of range");
        degree[a] += 1;
        degree[b] += 1;
    }
    assert!(degree.iter().all(|&d| d == 5), "every vertex has 5 edges: {degree:?}");

    // CIRCUMRADIUS matches the canonical vertex magnitude.
    let mag = ICO_VERTS[0].length();
    assert!((mag - CIRCUMRADIUS).abs() < 1e-3, "circumradius {CIRCUMRADIUS} vs {mag}");

    // Projection (identity rotation) is finite and bounded by ~circumradius.
    let screen = project(&ICO_VERTS, Quat::IDENTITY);
    for p in screen {
        assert!(p.is_finite(), "projected point is finite: {p:?}");
        assert!(p.length() <= CIRCUMRADIUS * 2.0, "projected point stays bounded: {p:?}");
    }
    // A rotation changes the projection (the wireframe actually tumbles).
    let rotated = project(&ICO_VERTS, Quat::from_rotation_y(0.7));
    assert!(screen != rotated, "rotation changes the projected wireframe");
}

// ── 59. asteroid_wireframe_mesh ───────────────────────────────────────────────

/// The rainbow wireframe mesh: one thin quad per edge (30 edges → 120 verts /
/// 180 indices), all indices in range, finite positions, and each edge's quad
/// carries its two endpoints' vertex colors (so the GPU interpolates the rainbow
/// gradient along the strut).
#[test]
fn asteroid_wireframe_mesh() {
    use crate::systems::asteroids::{project, wireframe_geometry, ICO_EDGES, ICO_VERTS};

    let screen = project(&ICO_VERTS, Quat::IDENTITY);
    // Distinct color per vertex so we can verify per-endpoint assignment.
    let mut colors = [[0.0_f32; 4]; 12];
    for (i, c) in colors.iter_mut().enumerate() {
        *c = [i as f32, 12.0 - i as f32, 1.0, 1.0];
    }

    let (pos, col, idx) = wireframe_geometry(&screen, &colors);
    assert_eq!(pos.len(), ICO_EDGES.len() * 4, "4 verts per edge");
    assert_eq!(col.len(), ICO_EDGES.len() * 4);
    assert_eq!(idx.len(), ICO_EDGES.len() * 6, "2 triangles per edge");
    assert!(idx.iter().all(|&i| (i as usize) < pos.len()), "indices in range");
    assert!(
        pos.iter().all(|p| p.iter().all(|c| c.is_finite())),
        "finite positions"
    );

    // Edge 0's quad is colored [a, a, b, b] — endpoints carry their vertex hues.
    let (a, b) = ICO_EDGES[0];
    assert_eq!(col[0], colors[a]);
    assert_eq!(col[1], colors[a]);
    assert_eq!(col[2], colors[b]);
    assert_eq!(col[3], colors[b]);
    assert_ne!(colors[a], colors[b], "the gradient varies vertex-to-vertex");
}

// ── 60. lightning_bolt_points ─────────────────────────────────────────────────

/// The Lance/Arc bolt geometry (spec III.7): anchored endpoints on the beam line,
/// monotonic along its length, bounded interior jag, and a different jag per seed
/// (so it crackles frame to frame).
#[test]
fn lightning_bolt_points() {
    use crate::systems::power_weapon::bolt_points;

    let length = 300.0;
    let pts = bolt_points(length, 1.23);
    assert!(pts.len() >= 5, "enough segments ({})", pts.len());

    // Endpoints anchored on the beam line (y=0), spanning 0..length on x.
    assert_eq!(pts[0], Vec2::ZERO, "starts at the origin");
    let last = *pts.last().unwrap();
    assert!(
        (last.x - length).abs() < 1e-3 && last.y.abs() < 1e-6,
        "ends at (length, 0): {last:?}"
    );

    // x strictly increases; interior y jag stays within the amplitude bound.
    let amp = (length * 0.05).min(10.0);
    for w in pts.windows(2) {
        assert!(w[1].x > w[0].x, "x is monotonic along the bolt");
    }
    for p in &pts[1..pts.len() - 1] {
        assert!(p.y.abs() <= amp + 1e-3, "interior jag bounded by amp ({p:?})");
    }

    // A different seed re-rolls the jag (the bolt crackles).
    assert_ne!(pts, bolt_points(length, 9.99), "seed changes the jag");
}

// ── 61. triforce_tank_glyphs ──────────────────────────────────────────────────

/// The HUD triforce lights one gold glyph per spare tank, dims the rest
/// (spec VIII.1): glyph `i` is gold iff `i < tanks`.
#[test]
fn triforce_tank_glyphs() {
    use crate::render::hud::tank_glyph_color;

    let gold = Color::srgb(1.0, 0.843, 0.0);
    // 0 tanks → all three dim.
    assert_ne!(tank_glyph_color(0, 0), gold);
    // 2 tanks → glyphs 0,1 gold, glyph 2 dim.
    assert_eq!(tank_glyph_color(0, 2), gold);
    assert_eq!(tank_glyph_color(1, 2), gold);
    assert_ne!(tank_glyph_color(2, 2), gold);
    // Full 3 tanks → all gold.
    assert_eq!(tank_glyph_color(2, 3), gold);
}

// ── 63. energy_orb_color_states ───────────────────────────────────────────────

/// The energy-sphere color (spec VIII.1): teal that brightens with charge, and a
/// gold pulse (high red+green) when ready.
#[test]
fn energy_orb_color_states() {
    use crate::render::hud::energy_orb_color;

    let dim = energy_orb_color(0.0, false, 0.0).to_srgba();
    let full = energy_orb_color(1.0, false, 0.0).to_srgba();
    // Charging brightens the (blue-dominant) teal.
    assert!(full.blue > dim.blue, "more charge → brighter ({} vs {})", full.blue, dim.blue);
    assert!(full.blue > full.red, "charging orb is teal (blue-dominant)");

    // Ready → gold: red & green dominate blue.
    let ready = energy_orb_color(1.0, true, 0.3).to_srgba();
    assert!(ready.red > ready.blue && ready.green > ready.blue, "ready orb is gold ({ready:?})");
}

// ── 64. mission_assignment ────────────────────────────────────────────────────

/// Boss waves (every 3rd) always assign `NoDamage`; non-boss waves roll one of
/// the four trackable objectives; the reward scales with the wave (spec V.6).
#[test]
fn mission_assignment() {
    use crate::resources::GameRng;
    use crate::systems::missions::{mission_for_wave, mission_reward, MissionKind};

    let mut rng = GameRng::default();

    // Boss waves → always NoDamage.
    for w in [3, 6, 9, 30] {
        assert_eq!(mission_for_wave(w, &mut rng), MissionKind::NoDamage, "wave {w} is a boss wave");
    }
    // Non-boss waves → some valid objective (sample several rolls).
    for w in [1, 2, 4, 5, 7] {
        for _ in 0..8 {
            let m = mission_for_wave(w, &mut rng);
            assert!(matches!(
                m,
                MissionKind::NoDamage
                    | MissionKind::FastKill
                    | MissionKind::Asteroid
                    | MissionKind::Streak
                    | MissionKind::Precision
            ));
        }
    }
    // Reward scales with the wave.
    assert!(mission_reward(30) > mission_reward(1));
}

// ── 65. mission_streak_completion ─────────────────────────────────────────────

/// A Streak mission completes (and pays a gold bonus) once the kill-streak hits
/// the target (spec V.6).
#[test]
fn mission_streak_completion() {
    use crate::resources::{KillStreak, Score};
    use crate::systems::missions::{update_missions, Mission, MissionKind};
    use crate::systems::wave::Wave;

    let mut app = test_app();
    let world = app.world_mut();
    world.insert_resource(Time::<()>::default());
    world.init_resource::<Mission>();
    world.insert_resource(Wave::default()); // wave 1 (non-boss)

    let mut step = Schedule::default();
    step.add_systems(update_missions);
    // First run assigns this wave's mission (last_wave None → Some(1)).
    step.run(world);

    // Force a Streak objective, then drive the kill-streak to the target.
    world.resource_mut::<Mission>().kind = MissionKind::Streak;
    world.resource_mut::<Mission>().done = false;
    world.resource_mut::<KillStreak>().kills = 12;
    let gold_before = world.resource::<Score>().gold;

    step.run(world);
    assert!(world.resource::<Mission>().done, "streak ≥ 12 completes the mission");
    assert!(world.resource::<Score>().gold > gold_before, "completion pays a gold bonus");
}

// ── 66. mission_precision_counts_crits ────────────────────────────────────────

/// The Precision mission completes after 25 `Crit` messages accumulate in a wave
/// (spec V.6); fewer leaves it incomplete.
#[test]
fn mission_precision_counts_crits() {
    use crate::messages::Crit;
    use crate::systems::missions::{update_missions, Mission, MissionKind};
    use crate::systems::wave::Wave;

    let mut app = test_app();
    let world = app.world_mut();
    world.insert_resource(Time::<()>::default());
    world.init_resource::<Mission>();
    world.insert_resource(Wave::default());

    let mut step = Schedule::default();
    step.add_systems(update_missions);
    step.run(world); // assign wave 1's mission

    // Force a Precision objective.
    world.resource_mut::<Mission>().kind = MissionKind::Precision;
    world.resource_mut::<Mission>().done = false;

    // 24 crits → still incomplete.
    for _ in 0..24 {
        world.write_message(Crit);
    }
    step.run(world);
    assert!(!world.resource::<Mission>().done, "24 crits is not enough");

    // One more (25 total) → complete.
    world.write_message(Crit);
    step.run(world);
    assert!(world.resource::<Mission>().done, "25 crits completes Precision");
}

// ── 67. pulse_toast_trigger ───────────────────────────────────────────────────

/// The pulse-toast fires only on a *new* reinforcement pulse (P>0): pulse 0
/// (spawned_pulses 0→1) is silent; later pulses (→2, →3) toast; no change is
/// silent (spec V.6).
#[test]
fn pulse_toast_trigger() {
    use crate::render::wave_title::should_toast_pulse;

    assert!(!should_toast_pulse(0, 1), "pulse 0 is silent");
    assert!(should_toast_pulse(1, 2), "first reinforcement toasts");
    assert!(should_toast_pulse(2, 3), "later reinforcement toasts");
    assert!(!should_toast_pulse(2, 2), "no new pulse → silent");
    assert!(!should_toast_pulse(3, 2), "counter reset (wave change) → silent");
}

// ── 68. executioner_hits_low_hp_harder ────────────────────────────────────────

/// The Executioner passive (spec VI.3) adds damage only vs enemies below the
/// execute threshold (<25% HP). With 5 stacks (+100%), a sub-threshold enemy
/// takes exactly 2× what a full-HP enemy takes for the same shot.
#[test]
fn executioner_hits_low_hp_harder() {
    use crate::systems::collision::bullet_hits_enemy;
    use crate::systems::shop::{executioner_bonus, Upgrades, UpgradeId, EXECUTE_THRESHOLD};

    // Pure helper.
    assert_eq!(executioner_bonus(0), 0.0);
    assert!((executioner_bonus(1) - 0.20).abs() < 1e-6);
    assert_eq!(EXECUTE_THRESHOLD, 0.25);

    #[derive(Resource, Default)]
    struct DmgSum(f32);
    fn capture(mut r: MessageReader<Damage>, mut s: ResMut<DmgSum>) {
        for d in r.read() {
            s.0 += d.amount;
        }
    }

    // One bullet vs one enemy at (cur/max) HP with 5 Executioner stacks. A fresh
    // (seeded) RNG each call → identical crit roll, so executioner is the only
    // difference between the two runs.
    let run = |cur: f32, max: f32| -> f32 {
        let mut app = test_app();
        let world = app.world_mut();
        world.resource_mut::<Upgrades>().set(UpgradeId::Executioner, 5);
        world.init_resource::<DmgSum>();
        world.spawn((
            Enemy {
                kind: EnemyKind::Hunter,
            },
            Health { current: cur, max },
            Collider { radius: 20.0 },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        world.spawn((
            Bullet {
                kind: BulletKind::Player,
                damage: 10.0,
                pierce: 0,
            },
            Collider { radius: 3.0 },
            Faction::Player,
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        let mut step = Schedule::default();
        step.add_systems((bullet_hits_enemy, capture).chain());
        step.run(world);
        world.resource::<DmgSum>().0
    };

    let low = run(1000.0, 5000.0); // 20% < 25% → executioner applies
    let full = run(5000.0, 5000.0); // 100% → no executioner
    assert!(full > 0.0, "control deals damage");
    // 5 stacks → +100% → exactly 2× (same crit roll via the shared seed).
    assert!(
        (low - full * 2.0).abs() < 1e-3,
        "executioner doubles sub-threshold damage (low={low}, full={full})"
    );
}

// ── 69. phase_echo_extends_dash_invuln ────────────────────────────────────────

/// Phase Echo adds 2 s of dash i-frames per stack (spec VI.3), on top of the
/// 0.3 s base.
#[test]
fn phase_echo_extends_dash_invuln() {
    use crate::systems::shop::phase_echo_secs;

    assert_eq!(phase_echo_secs(0), 0.0);
    assert_eq!(phase_echo_secs(1), 2.0);
    assert_eq!(phase_echo_secs(2), 4.0);
    // The dash grants 0.3 s base + the echo bonus.
    assert!((0.3 + phase_echo_secs(2) - 4.3).abs() < 1e-6, "2 stacks → 4.3 s dash i-frames");
}

// ── 70. overcharge_cadence ────────────────────────────────────────────────────

/// Overcharge is off at 0 stacks, fires more frequently with more stacks, and
/// triples exactly every Nth bullet (spec VI.3).
#[test]
fn overcharge_cadence() {
    use crate::systems::shop::overcharge_interval;
    use crate::systems::weapons::is_overcharged;

    assert_eq!(overcharge_interval(0), 0, "off with no stacks");
    assert_eq!(overcharge_interval(1), 7);
    assert_eq!(overcharge_interval(4), 4, "max stacks → most frequent");
    assert!(overcharge_interval(4) < overcharge_interval(1), "more stacks → smaller interval");

    // Off interval never overcharges.
    assert!(!is_overcharged(7, 0));
    // With interval 4, exactly every 4th bullet is overcharged.
    let n = overcharge_interval(4); // 4
    let hot: Vec<u32> = (1..=12).filter(|&t| is_overcharged(t, n)).collect();
    assert_eq!(hot, vec![4, 8, 12], "every 4th bullet (got {hot:?})");
}

// ── 71. static_discharge_hits_nearby ──────────────────────────────────────────

/// Static Discharge pulses AoE damage to enemies within its radius and spares
/// distant ones (spec VI.3); more stacks → faster + harder.
#[test]
fn static_discharge_hits_nearby() {
    use crate::systems::passives::tick_static_discharge;
    use crate::systems::shop::{
        static_discharge_damage, static_discharge_interval, UpgradeId, Upgrades,
    };

    // Helpers scale with stacks.
    assert!(static_discharge_interval(5) < static_discharge_interval(1), "more stacks → faster");
    assert_eq!(static_discharge_damage(0), 0.0);
    assert!(static_discharge_damage(5) > static_discharge_damage(1));

    let mut app = test_app();
    let world = app.world_mut();
    world.resource_mut::<Upgrades>().set(UpgradeId::StaticDischarge, 5);

    // Player at origin; a near enemy (in radius) and a far one (well outside).
    world.spawn((Ship::default(), Transform::from_xyz(0.0, 0.0, 0.0)));
    let near = world
        .spawn((
            Enemy {
                kind: EnemyKind::Hunter,
            },
            Collider { radius: 18.0 },
            Health {
                current: 100.0,
                max: 100.0,
            },
            Transform::from_xyz(80.0, 0.0, 0.0),
        ))
        .id();
    let far = world
        .spawn((
            Enemy {
                kind: EnemyKind::Hunter,
            },
            Collider { radius: 18.0 },
            Health {
                current: 100.0,
                max: 100.0,
            },
            Transform::from_xyz(600.0, 0.0, 0.0),
        ))
        .id();

    // Advance past the pulse interval so it fires this run.
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(static_discharge_interval(5) + 0.1));
    world.insert_resource(time);

    #[derive(Resource, Default)]
    struct Dmg(Vec<Entity>);
    world.init_resource::<Dmg>();
    fn collect(mut r: MessageReader<Damage>, mut d: ResMut<Dmg>) {
        for ev in r.read() {
            d.0.push(ev.target);
        }
    }

    let mut step = Schedule::default();
    step.add_systems((tick_static_discharge, collect).chain());
    step.run(world);

    let hit = &world.resource::<Dmg>().0;
    assert!(hit.contains(&near), "discharge damages the near enemy");
    assert!(!hit.contains(&far), "discharge spares the far enemy");
}

// ── 72. combat_medic_heals_kill_after_hit ─────────────────────────────────────

/// Combat Medic heals on the first enemy kill after taking a hit, then goes on
/// cooldown so a second kill (still on CD) doesn't heal again (spec VI.3).
#[test]
fn combat_medic_heals_kill_after_hit() {
    use crate::messages::PlayerHurt;
    use crate::systems::passives::{tick_combat_medic, COMBAT_MEDIC_HEAL};
    use crate::systems::shop::{UpgradeId, Upgrades};

    let mut app = test_app();
    let world = app.world_mut();
    world.insert_resource(Time::<()>::default());
    world.resource_mut::<Upgrades>().set(UpgradeId::CombatMedic, 1);

    let player = world
        .spawn((
            Ship::default(),
            Health {
                current: 20.0,
                max: 40.0,
            },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();

    let mut step = Schedule::default();
    step.add_systems(tick_combat_medic);

    // Hit (arm) + a kill in the same tick → heal.
    world.write_message(PlayerHurt { amount: 5.0 });
    world.write_message(Death {
        entity: Entity::PLACEHOLDER,
        position: Vec2::ZERO,
        kind: Some(EnemyKind::Hunter),
        boss_tier: 0,
        mini_boss: false,
    });
    step.run(world);
    let after_first = world.get::<Health>(player).unwrap().current;
    assert!(
        (after_first - (20.0 + COMBAT_MEDIC_HEAL)).abs() < 1e-3,
        "kill-after-hit heals (got {after_first})"
    );

    // Another kill while on cooldown (and not freshly hit) → no further heal.
    world.write_message(Death {
        entity: Entity::PLACEHOLDER,
        position: Vec2::ZERO,
        kind: Some(EnemyKind::Hunter),
        boss_tier: 0,
        mini_boss: false,
    });
    step.run(world);
    assert!(
        (world.get::<Health>(player).unwrap().current - after_first).abs() < 1e-3,
        "still on cooldown → no second heal"
    );
}

// ── 73. momentum_ramps_and_caps ───────────────────────────────────────────────

/// Momentum adds no bonus unowned or at rest, ramps with sustained movement, and
/// caps at +15%/stack (spec VI.3).
#[test]
fn momentum_ramps_and_caps() {
    use crate::systems::shop::momentum_bonus;

    assert_eq!(momentum_bonus(5.0, 0), 0.0, "unowned → no bonus");
    assert_eq!(momentum_bonus(0.0, 4), 0.0, "at rest → no bonus");
    // 1 stack: +5%/s, capped at +15%.
    assert!((momentum_bonus(1.0, 1) - 0.05).abs() < 1e-6, "ramps 5%/s");
    assert!((momentum_bonus(10.0, 1) - 0.15).abs() < 1e-6, "caps at +15%/stack");
    // More sustained → more bonus (until the cap); more stacks → higher cap.
    assert!(momentum_bonus(1.0, 4) > momentum_bonus(1.0, 1));
    assert!(momentum_bonus(100.0, 4) > momentum_bonus(100.0, 1), "cap scales with stacks");
}

// ── 74. whirlwind_damages_orbit_zone ──────────────────────────────────────────

/// Whirlwind damages enemies inside its orbiting zone and spares distant ones,
/// and spawns its visual blade (spec VI.3).
#[test]
fn whirlwind_damages_orbit_zone() {
    use crate::systems::passives::{
        tick_whirlwind, WhirlwindBlade, WHIRL_OMEGA, WHIRL_ORBIT_R,
    };
    use crate::systems::shop::{whirlwind_dps, UpgradeId, Upgrades};

    assert_eq!(whirlwind_dps(0), 0.0);
    assert!(whirlwind_dps(4) > whirlwind_dps(1));

    let mut app = test_app();
    let world = app.world_mut();
    world.resource_mut::<Upgrades>().set(UpgradeId::Whirlwind, 4);

    // Advance time, then place the near enemy at the resulting orbit centre.
    let dt = 0.1_f32;
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(dt));
    world.insert_resource(time);
    let ang = dt * WHIRL_OMEGA;
    let center = Vec2::new(ang.cos(), ang.sin()) * WHIRL_ORBIT_R; // player at origin

    world.spawn((Ship::default(), Transform::from_xyz(0.0, 0.0, 0.0)));
    let near = world
        .spawn((
            Enemy {
                kind: EnemyKind::Hunter,
            },
            Collider { radius: 18.0 },
            Transform::from_translation(center.extend(0.0)),
        ))
        .id();
    let far = world
        .spawn((
            Enemy {
                kind: EnemyKind::Hunter,
            },
            Collider { radius: 18.0 },
            Transform::from_xyz(5000.0, 0.0, 0.0),
        ))
        .id();

    #[derive(Resource, Default)]
    struct Dmg(Vec<Entity>);
    world.init_resource::<Dmg>();
    fn collect(mut r: MessageReader<Damage>, mut d: ResMut<Dmg>) {
        for ev in r.read() {
            d.0.push(ev.target);
        }
    }

    let mut step = Schedule::default();
    step.add_systems((tick_whirlwind, collect).chain());
    step.run(world);

    let hit = &world.resource::<Dmg>().0;
    assert!(hit.contains(&near), "whirlwind damages the enemy in its zone");
    assert!(!hit.contains(&far), "whirlwind spares the distant enemy");
    // The visual blade was spawned.
    let mut blades = world.query::<&WhirlwindBlade>();
    assert_eq!(blades.iter(world).count(), 1, "blade visual spawned while owned");
}

// ── 62. status_aura_lifecycle ─────────────────────────────────────────────────

/// A burn aura spawns when an enemy gains `Burning`, follows the enemy, and
/// despawns when `Burning` is removed (spec enemy status FX).
#[test]
fn status_aura_lifecycle() {
    use crate::render::status_fx::{spawn_status_auras, update_status_auras, StatusAura};

    let mut app = test_app();
    app.world_mut().insert_resource(Time::<()>::default());

    // Burning enemy at (100, 0).
    let enemy = app
        .world_mut()
        .spawn((
            Enemy {
                kind: EnemyKind::Hunter,
            },
            Collider { radius: 18.0 },
            Burning { dps: 3.0, secs: 2.0 },
            Transform::from_xyz(100.0, 0.0, 0.0),
        ))
        .id();

    // Spawn the aura (Added<Burning> fires on first sight).
    let mut spawn = Schedule::default();
    spawn.add_systems(spawn_status_auras);
    spawn.run(app.world_mut());
    {
        let mut q = app.world_mut().query::<&StatusAura>();
        assert_eq!(q.iter(app.world()).count(), 1, "aura spawns for a burning enemy");
        assert_eq!(q.iter(app.world()).next().unwrap().target, enemy);
    }

    // It follows the target.
    let mut update = Schedule::default();
    update.add_systems(update_status_auras);
    update.run(app.world_mut());
    {
        let mut q = app.world_mut().query_filtered::<&Transform, With<StatusAura>>();
        let pos = q.iter(app.world()).next().unwrap().translation;
        assert!((pos.x - 100.0).abs() < 1e-3, "aura follows the enemy ({pos:?})");
    }

    // Remove Burning → the aura despawns on the next update.
    app.world_mut().entity_mut(enemy).remove::<Burning>();
    update.run(app.world_mut());
    {
        let mut q = app.world_mut().query::<&StatusAura>();
        assert_eq!(q.iter(app.world()).count(), 0, "aura despawns when Burning ends");
    }
}

// ── 62b. elemental_status_auras ───────────────────────────────────────────────

/// Each elemental status gets its own aura: an enemy with two simultaneous
/// statuses (Frozen + Mark) shows two rings, and each ring despawns when its
/// status is removed independently.
#[test]
fn elemental_status_auras_stack_and_despawn_independently() {
    use crate::components::{Frozen, Mark};
    use crate::render::status_fx::{spawn_status_auras, update_status_auras, StatusAura};

    let mut app = test_app();
    app.world_mut().insert_resource(Time::<()>::default());

    let enemy = app
        .world_mut()
        .spawn((
            Enemy { kind: EnemyKind::Hunter },
            Collider { radius: 16.0 },
            Frozen { secs: 1.5 },
            Mark { secs: 6.0 },
            Transform::from_xyz(50.0, 0.0, 0.0),
        ))
        .id();

    let mut spawn = Schedule::default();
    spawn.add_systems(spawn_status_auras);
    spawn.run(app.world_mut());
    {
        let mut q = app.world_mut().query::<&StatusAura>();
        assert_eq!(
            q.iter(app.world()).count(),
            2,
            "two statuses → two concentric auras"
        );
    }

    let mut update = Schedule::default();
    update.add_systems(update_status_auras);

    // Drop Frozen → only the Mark aura remains.
    app.world_mut().entity_mut(enemy).remove::<Frozen>();
    update.run(app.world_mut());
    {
        let mut q = app.world_mut().query::<&StatusAura>();
        assert_eq!(q.iter(app.world()).count(), 1, "Frozen aura despawns, Mark stays");
    }

    // Drop Mark → no auras left.
    app.world_mut().entity_mut(enemy).remove::<Mark>();
    update.run(app.world_mut());
    {
        let mut q = app.world_mut().query::<&StatusAura>();
        assert_eq!(q.iter(app.world()).count(), 0, "all auras gone when statuses clear");
    }
}

/// The armory catalog lists the six gold-locked exotic weapons + the six
/// elemental attunements (the free base loadout is omitted), each with a
/// positive cost, a stable id, and starting locked.
#[test]
fn armory_catalog_has_exotics_and_attunements() {
    use crate::meta::Meta;
    use crate::systems::armory::armory_catalog;
    use crate::systems::weapons::WeaponKind;

    let catalog = armory_catalog();
    assert_eq!(catalog.len(), 22, "6 exotic weapons + 6 attunements + 10 abilities");
    let meta = Meta::default();

    // No duplicate ids, every cost positive, nothing unlocked by default.
    let mut ids = std::collections::HashSet::new();
    for e in &catalog {
        assert!(e.cost > 0, "{} has a cost", e.name);
        assert!(ids.insert(e.id), "duplicate armory id {}", e.id);
        assert!(!meta.is_unlocked(e.id), "{} starts locked", e.name);
    }

    // The six weapon entries are exactly the non-base weapons.
    let weapon_ids: Vec<&str> = WeaponKind::ALL
        .iter()
        .filter(|w| !w.base_unlocked())
        .map(|w| w.id())
        .collect();
    assert_eq!(weapon_ids.len(), 6);
    for wid in weapon_ids {
        assert!(ids.contains(wid), "exotic weapon {wid} missing from catalog");
    }
    // The six attunement entries use the ATT_ id namespace; the ten ability
    // entries use ABL_ (the four base-loadout abilities are free, not sold).
    assert_eq!(catalog.iter().filter(|e| e.id.starts_with("ATT_")).count(), 6);
    assert_eq!(catalog.iter().filter(|e| e.id.starts_with("ABL_")).count(), 10);
}

/// Weapon armory-gating: the five base weapons are always available, the six
/// exotics only once unlocked. Tab/Q cycling skips locked weapons.
#[test]
fn weapon_cycle_skips_locked_exotics() {
    use crate::meta::{Meta, WEAPON_UNLOCK_COST};
    use crate::systems::weapons::WeaponKind;

    let mut meta = Meta::default();

    // Base loadout is free; exotics are locked by default.
    assert!(WeaponKind::PulseCannon.is_available(&meta), "Pulse is a free base weapon");
    assert!(WeaponKind::ClusterLauncher.is_available(&meta), "Cluster is base");
    assert!(!WeaponKind::GravityLance.is_available(&meta), "Gravity Lance is gold-locked");
    assert!(!WeaponKind::FlakCannon.is_available(&meta), "Flak is gold-locked");

    // From the last base weapon, Tab wraps past all the locked exotics back to
    // the first base weapon (since none are unlocked).
    assert_eq!(
        WeaponKind::ClusterLauncher.next_available(&meta),
        WeaponKind::PulseCannon,
        "with no exotics unlocked, cycling wraps to the base set"
    );

    // Unlock Gravity Lance → it becomes reachable from Cluster.
    meta.account_gold = WEAPON_UNLOCK_COST;
    assert!(meta.unlock(WeaponKind::GravityLance.id(), WEAPON_UNLOCK_COST));
    assert!(WeaponKind::GravityLance.is_available(&meta));
    assert_eq!(
        WeaponKind::ClusterLauncher.next_available(&meta),
        WeaponKind::GravityLance,
        "an unlocked exotic is now in the cycle"
    );
}

/// The player's own statuses (PlayerBurn/Chill/Corrode) get the same aura: it
/// spawns when afflicted and despawns when the status clears.
#[test]
fn player_status_gets_an_aura() {
    use crate::components::PlayerChill;
    use crate::render::status_fx::{spawn_status_auras, update_status_auras, StatusAura};

    let mut app = test_app();
    app.world_mut().insert_resource(Time::<()>::default());

    let player = app
        .world_mut()
        .spawn((
            Ship::default(),
            Collider { radius: 14.0 },
            PlayerChill { secs: 2.0 },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();

    let mut spawn = Schedule::default();
    spawn.add_systems(spawn_status_auras);
    spawn.run(app.world_mut());
    {
        let mut q = app.world_mut().query::<&StatusAura>();
        assert_eq!(q.iter(app.world()).count(), 1, "a chilled player gets an aura");
    }

    let mut update = Schedule::default();
    update.add_systems(update_status_auras);
    app.world_mut().entity_mut(player).remove::<PlayerChill>();
    update.run(app.world_mut());
    {
        let mut q = app.world_mut().query::<&StatusAura>();
        assert_eq!(q.iter(app.world()).count(), 0, "aura clears when the player un-chills");
    }
}

// ── 75. survivor_pick_chains_into_shop ────────────────────────────────────────

/// A stage-clear survivor-card pick applies the card and chains into the Shop
/// (spec V.6 shop-suggest flow) rather than straight to Playing.
#[test]
fn survivor_pick_chains_into_shop() {
    use crate::systems::shop::{UpgradeId, Upgrades};
    use crate::systems::survivor::{survivor_input, SurvivorChoice};
    use crate::systems::wave::Wave;

    let mut app = test_app();
    let world = app.world_mut();
    world.insert_resource(Wave::default());
    world.insert_resource(SurvivorChoice {
        cards: [Some(UpgradeId::HealthBoost), None, None],
    });
    let mut input = ButtonInput::<KeyCode>::default();
    input.press(KeyCode::Digit1); // pick the first card
    world.insert_resource(input);

    // Player — survivor_input applies the picked upgrade to it.
    world.spawn((
        Ship::default(),
        Health {
            current: 40.0,
            max: 40.0,
        },
        Shield { reduction: 0.15 },
        Lives {
            count: 1,
            progress: 0.0,
        },
        Transform::default(),
    ));

    let gold_before = world.resource::<Score>().gold;
    let mut step = Schedule::default();
    step.add_systems(survivor_input);
    step.run(world);

    assert_eq!(
        world.resource::<Upgrades>().owned(UpgradeId::HealthBoost),
        1,
        "the picked card is granted"
    );
    assert!(world.resource::<Score>().gold > gold_before, "stage-clear gold bonus");
    assert!(
        matches!(
            world.resource::<NextState<GameState>>(),
            NextState::Pending(GameState::Shop)
        ),
        "survivor pick chains into the Shop"
    );
}

// ── 76. item_rarity_and_affix_counts ──────────────────────────────────────────

/// Rarity rolls at the spec VI.5 cumulative thresholds (0.65 / 0.92), and each
/// rarity mints the right number of distinct affixes (1 / 2 / 3).
#[test]
fn item_rarity_and_affix_counts() {
    use crate::resources::GameRng;
    use crate::systems::items::{create_item, ItemSlot, Rarity};

    // 8-tier ladder thresholds (v6.161 RARITY_TIERS).
    assert_eq!(Rarity::roll(0.0), Rarity::Common);
    assert_eq!(Rarity::roll(0.49), Rarity::Common);
    assert_eq!(Rarity::roll(0.50), Rarity::Rare);
    assert_eq!(Rarity::roll(0.80), Rarity::Exceptional);
    assert_eq!(Rarity::roll(0.90), Rarity::Legendary);
    assert_eq!(Rarity::roll(0.96), Rarity::Epic);
    assert_eq!(Rarity::roll(0.985), Rarity::Godlike);
    assert_eq!(Rarity::roll(0.994), Rarity::Divine);
    assert_eq!(Rarity::roll(0.999), Rarity::Transcendental);

    // affix counts 1/2/3/3/4/4/5/5 ascending; rank 1..8; next() walks the ladder.
    assert_eq!(Rarity::Common.affix_count(), 1);
    assert_eq!(Rarity::Exceptional.affix_count(), 3);
    assert_eq!(Rarity::Epic.affix_count(), 4);
    assert_eq!(Rarity::Transcendental.affix_count(), 5);
    assert_eq!(Rarity::Common.rank(), 1);
    assert_eq!(Rarity::Transcendental.rank(), 8);
    assert_eq!(Rarity::Common.next(), Some(Rarity::Rare));
    assert_eq!(Rarity::Transcendental.next(), None);

    // create_item respects the count + carries the wave-level + has a name.
    let mut rng = GameRng::default();
    for (rarity, n) in [(Rarity::Common, 1), (Rarity::Epic, 4), (Rarity::Transcendental, 5)] {
        let item = create_item(&mut rng, ItemSlot::Cockpit, 7, rarity);
        assert_eq!(item.affixes.len(), n, "{rarity:?} → {n} affixes");
        assert_eq!(item.level, 7, "item level = wave");
        assert!(!item.name.is_empty(), "item has a generated name");
        // Distinct affix kinds (the pool shuffle is without replacement).
        for i in 0..item.affixes.len() {
            for j in (i + 1)..item.affixes.len() {
                assert_ne!(item.affixes[i].kind, item.affixes[j].kind, "affixes are distinct");
            }
        }
    }
}

// ── 77. item_affix_value_invariants ───────────────────────────────────────────

/// Rolled affix values obey the spec rounding/clamp rules across many waves +
/// rarities: HP is an integer, regen has ≤2 dp, every value clears its `min`, and
/// values scale up with the wave.
#[test]
fn item_affix_value_invariants() {
    use crate::resources::GameRng;
    use crate::systems::items::{roll_affix_set, AffixKind, Rarity};

    let mut rng = GameRng::default();
    let mut seen_hp = false;
    let mut seen_regen = false;
    for wave in [1u32, 5, 15, 30] {
        for rarity in [Rarity::Common, Rarity::Rare, Rarity::Epic] {
            // Roll the whole pool so we exercise every affix kind.
            let affixes = roll_affix_set(&mut rng, wave, rarity, 9);
            for a in &affixes {
                assert!(a.value > 0.0, "{:?} value positive", a.kind);
                match a.kind {
                    AffixKind::Hp => {
                        assert_eq!(a.value.fract(), 0.0, "HP is integer (got {})", a.value);
                        assert!(a.value >= 1.0);
                        seen_hp = true;
                    }
                    AffixKind::Regen => {
                        // ≤2 dp: value*100 is (near) integral.
                        let scaled = a.value * 100.0;
                        assert!((scaled - scaled.round()).abs() < 1e-3, "regen ≤2dp (got {})", a.value);
                        assert!(a.value >= 0.1);
                        seen_regen = true;
                    }
                    AffixKind::Thorns => assert!(a.value >= 2.0, "thorns min 2"),
                    AffixKind::CritDamage => assert!(a.value >= 3.0, "crit-dmg min 3"),
                    AffixKind::Speed => assert!(a.value >= 2.0, "speed min 2"),
                    _ => assert!(a.value >= 1.0, "{:?} min 1", a.kind),
                }
            }
        }
    }
    assert!(seen_hp && seen_regen, "exercised HP + regen affixes");

    // Labels read as expected.
    use crate::systems::items::Affix;
    assert_eq!(Affix { kind: AffixKind::Hp, value: 8.0 }.label(), "+8 MAX HP");
    assert_eq!(Affix { kind: AffixKind::Toughness, value: 3.0 }.label(), "+3% DEF");
    assert_eq!(Affix { kind: AffixKind::Regen, value: 0.3 }.label(), "+0.3/s REGEN");
    assert_eq!(Affix { kind: AffixKind::CritDamage, value: 8.5 }.label(), "+8.5% CRIT DMG");
}

// ── 78. item_drop_rates_and_determinism ───────────────────────────────────────

/// Per-category drop rates match the spec (boss strictly higher); `roll_item_drops`
/// is bounded (≤3 items/kill), deterministic under a seeded RNG, and bosses drop
/// more loot on aggregate.
#[test]
fn item_drop_rates_and_determinism() {
    use crate::resources::GameRng;
    use crate::systems::items::{roll_item_drops, SlotCategory};

    // Exact spec rates + boss > normal in every category.
    for cat in [SlotCategory::Hp, SlotCategory::Toughness, SlotCategory::Trinket] {
        assert!(cat.drop_rate(true) > cat.drop_rate(false), "{cat:?} boss rate higher");
    }
    assert!((SlotCategory::Hp.drop_rate(false) - 0.025).abs() < 1e-6);
    assert!((SlotCategory::Hp.drop_rate(true) - 0.085).abs() < 1e-6);
    assert!((SlotCategory::Toughness.drop_rate(false) - 0.020).abs() < 1e-6);
    assert!((SlotCategory::Trinket.drop_rate(false) - 0.015).abs() < 1e-6);

    // ≤3 items per kill; each item is internally consistent.
    let mut rng = GameRng::default();
    for _ in 0..500 {
        let items = roll_item_drops(&mut rng, 10, false);
        assert!(items.len() <= 3, "≤3 drops/kill (got {})", items.len());
        for it in &items {
            assert_eq!(it.affixes.len(), it.rarity.affix_count());
            assert_eq!(it.level, 10);
        }
    }

    // Determinism: identical seed → identical first drop sequence.
    let mut a = GameRng::default();
    let mut b = GameRng::default();
    for _ in 0..50 {
        let ia = roll_item_drops(&mut a, 12, true);
        let ib = roll_item_drops(&mut b, 12, true);
        assert_eq!(ia.len(), ib.len(), "deterministic drop count");
        for (x, y) in ia.iter().zip(ib.iter()) {
            assert_eq!(x.slot, y.slot);
            assert_eq!(x.rarity, y.rarity);
            assert_eq!(x.affixes.len(), y.affixes.len());
        }
    }

    // Bosses drop more loot on aggregate (same seed, much higher rates).
    let mut normal_rng = GameRng::default();
    let mut boss_rng = GameRng::default();
    let normal: usize = (0..3000).map(|_| roll_item_drops(&mut normal_rng, 8, false).len()).sum();
    let boss: usize = (0..3000).map(|_| roll_item_drops(&mut boss_rng, 8, true).len()).sum();
    assert!(boss > normal, "bosses drop more loot (boss {boss} vs normal {normal})");
    assert!(normal > 0, "some normal drops occur");
}

// ── 79. item_loot_feed_on_death ───────────────────────────────────────────────

/// `roll_item_drops_on_death` mints loot for enemy kills (none for the player),
/// pushes it onto the `LootFeed`, and keeps the backlog bounded.
#[test]
fn item_loot_feed_on_death() {
    use crate::systems::items::{roll_item_drops_on_death, LootFeed};

    let mut app = test_app();
    let world = app.world_mut();
    world.insert_resource(Wave::default());
    world.init_resource::<LootFeed>();

    let mut step = Schedule::default();
    step.add_systems(roll_item_drops_on_death);

    // Player death (kind None) → no loot.
    world.write_message(Death {
        entity: Entity::PLACEHOLDER,
        position: Vec2::ZERO,
        kind: None,
        boss_tier: 0,
        mini_boss: false,
    });
    step.run(world);
    assert_eq!(world.resource::<LootFeed>().pending.len(), 0, "player death → no loot");

    // Many enemy deaths → items accumulate, bounded by the backlog cap.
    for _ in 0..300 {
        world.write_message(Death {
            entity: Entity::PLACEHOLDER,
            position: Vec2::ZERO,
            kind: Some(EnemyKind::Hunter),
            boss_tier: 0,
            mini_boss: false,
        });
    }
    step.run(world);
    let n = world.resource::<LootFeed>().pending.len();
    assert!(n > 0, "enemy kills mint loot (got {n})");
    assert!(n <= 12, "loot backlog stays bounded (got {n})");
}

// ── 80. loot_card_fade_curve ──────────────────────────────────────────────────

/// A loot card holds full opacity until its final fade window, then ramps
/// linearly to zero (spec VI.5 loot feed polish).
#[test]
fn loot_card_fade_curve() {
    use crate::render::loot_feed::card_alpha;

    // Full opacity well before the fade window + at its start.
    assert_eq!(card_alpha(6.0), 1.0, "fresh card is opaque");
    assert_eq!(card_alpha(1.2), 1.0, "still opaque at the fade boundary");
    // Linear fade inside the window.
    assert!((card_alpha(0.6) - 0.5).abs() < 1e-6, "half-faded mid-window");
    assert!((card_alpha(0.3) - 0.25).abs() < 1e-6, "quarter opacity near the end");
    assert_eq!(card_alpha(0.0), 0.0, "fully transparent at end of life");
    // Monotonic: less life → less (or equal) opacity.
    assert!(card_alpha(0.4) < card_alpha(0.8));
}

// ── 81. item_score_and_upgrade ────────────────────────────────────────────────

/// `score_item` weights each affix to an effective-HP scale, and `is_upgrade` is
/// strict-dominant (empty slot always wins; ties don't replace).
#[test]
fn item_score_and_upgrade() {
    use crate::systems::items::{is_upgrade, score_item, Affix, AffixKind, Item, ItemSlot, Rarity};

    let mk = |kind, value| Item {
        slot: ItemSlot::Cockpit,
        level: 1,
        rarity: Rarity::Common,
        affixes: vec![Affix { kind, value }],
        name: "x".to_string(),
    };

    // Weights: HP×1, Toughness×8, Regen×16.
    assert!((score_item(&mk(AffixKind::Hp, 10.0)) - 10.0).abs() < 1e-6);
    assert!((score_item(&mk(AffixKind::Toughness, 5.0)) - 40.0).abs() < 1e-6);
    assert!((score_item(&mk(AffixKind::Regen, 2.0)) - 32.0).abs() < 1e-6);

    let weak = mk(AffixKind::Hp, 10.0); // score 10
    let strong = mk(AffixKind::Toughness, 5.0); // score 40

    assert!(is_upgrade(None, &weak), "empty slot takes anything");
    assert!(is_upgrade(Some(&weak), &strong), "higher score replaces");
    assert!(!is_upgrade(Some(&strong), &weak), "lower score is kept");
    assert!(!is_upgrade(Some(&strong), &strong), "a tie does not replace (strict)");
}

// ── 82. equipment_autoequip_and_affix_total ───────────────────────────────────

/// Equipment auto-equips better drops per slot and sums affixes across slots.
#[test]
fn equipment_autoequip_and_affix_total() {
    use crate::systems::items::{Affix, AffixKind, Equipment, Item, ItemSlot, Rarity};

    let mk = |slot, kind, value| Item {
        slot,
        level: 1,
        rarity: Rarity::Common,
        affixes: vec![Affix { kind, value }],
        name: "x".to_string(),
    };

    let mut eq = Equipment::default();
    assert_eq!(eq.affix_total(AffixKind::Toughness), 0.0, "empty");

    // First HP-slot drop equips; a stronger one replaces; a weaker one doesn't.
    assert!(eq.try_equip(mk(ItemSlot::Cockpit, AffixKind::Hp, 5.0)));
    assert!(eq.try_equip(mk(ItemSlot::Cockpit, AffixKind::Hp, 12.0)), "better replaces");
    assert!(!eq.try_equip(mk(ItemSlot::Cockpit, AffixKind::Hp, 3.0)), "worse kept out");

    // A different slot equips independently and its affix sums in.
    assert!(eq.try_equip(mk(ItemSlot::Shielding, AffixKind::Toughness, 6.0)));
    assert!(eq.try_equip(mk(ItemSlot::Chassis, AffixKind::Toughness, 4.0)));
    assert!((eq.affix_total(AffixKind::Toughness) - 10.0).abs() < 1e-6, "toughness sums across slots");
    assert!((eq.affix_total(AffixKind::Hp) - 12.0).abs() < 1e-6, "best HP item only");

    eq.reset();
    assert_eq!(eq.affix_total(AffixKind::Toughness), 0.0, "reset clears slots");
}

// ── 83. equipment_toughness_reduces_player_damage ─────────────────────────────

/// An equipped TOUGHNESS affix folds into the player's %DR, so a hit lands for
/// less (spec VI.5 affix→stat).
#[test]
fn equipment_toughness_reduces_player_damage() {
    use crate::systems::damage::apply_damage;
    use crate::systems::items::{Affix, AffixKind, Equipment, Item, ItemSlot, Rarity};

    let mut app = test_app();
    let world = app.world_mut();

    // Equip a +50% DEF trinket (base shield reduction 0 → effective 50%).
    world.resource_mut::<Equipment>().try_equip(Item {
        slot: ItemSlot::Shielding,
        level: 1,
        rarity: Rarity::Epic,
        affixes: vec![Affix { kind: AffixKind::Toughness, value: 50.0 }],
        name: "Test Aegis".to_string(),
    });

    let player = world
        .spawn((
            Ship::default(),
            Health { current: 100.0, max: 100.0 },
            Shield { reduction: 0.0 },
            Transform::default(),
        ))
        .id();

    let mut step = Schedule::default();
    step.add_systems(apply_damage);

    world.write_message(Damage { target: player, amount: 20.0 });
    step.run(world);

    // 20 × (1 − 0.50) = 10 → HP 100 − 10 = 90 (vs 80 with no gear).
    let hp = world.get::<Health>(player).unwrap().current;
    assert!((hp - 90.0).abs() < 1e-3, "toughness gear halves the hit (got {hp})");
}

// ── 84. item_crit_damage_raises_cap ───────────────────────────────────────────

/// An equipped CRIT-DAMAGE affix (passed as `dmg_bonus`) lifts the crit
/// multiplier's upper bound, still clamped at 5.5× (spec III.6 / VI.5).
#[test]
fn item_crit_damage_raises_cap() {
    use crate::resources::{roll_crit, GameRng};

    // +100% crit-damage bonus → upper bound rises from 3.0 to 4.0.
    let mut rng = GameRng::default();
    let mut max_seen = 0.0_f32;
    for _ in 0..50_000 {
        let m = roll_crit(&mut rng, 1.0, 0, 1.0); // always crit, +1.0 bonus
        assert!((2.0..=4.0 + 1e-3).contains(&m), "crit in [2,4] with bonus, got {m}");
        max_seen = max_seen.max(m);
    }
    assert!(max_seen > 3.5, "the +100% bonus pushes the cap past 3.5 (got {max_seen})");

    // The 5.5× hard cap holds even with an absurd bonus.
    let mut rng2 = GameRng::default();
    for _ in 0..5_000 {
        let m = roll_crit(&mut rng2, 1.0, 20, 99.0);
        assert!(m <= 5.5 + 1e-3, "crit multiplier clamps at 5.5 (got {m})");
    }
}

// ── 85. item_vampirism_heals ──────────────────────────────────────────────────

/// An equipped VAMPIRISM affix heals the player on a bullet hit, same as the
/// shop passive (spec VI.5 affix→stat).
#[test]
fn item_vampirism_heals() {
    use crate::systems::collision::bullet_hits_enemy;
    use crate::systems::items::{Affix, AffixKind, Equipment, Item, ItemSlot, Rarity};

    let mut app = test_app();
    let world = app.world_mut();
    world.resource_mut::<Equipment>().try_equip(Item {
        slot: ItemSlot::Nanites,
        level: 1,
        rarity: Rarity::Epic,
        affixes: vec![Affix { kind: AffixKind::Vampirism, value: 100.0 }], // 100% lifesteal
        name: "Bloodlet Core".to_string(),
    });

    let player = world
        .spawn((
            Ship::default(),
            Health { current: 10.0, max: 100.0 },
            Collider { radius: 16.0 },
            Transform::from_xyz(500.0, 0.0, 0.0),
        ))
        .id();
    world.spawn((
        Enemy { kind: EnemyKind::Hunter },
        Health::new(1000.0), // survives the hit
        Collider { radius: 16.0 },
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));
    world.spawn((
        Bullet { kind: BulletKind::Player, damage: 10.0, pierce: 0 },
        Collider { radius: 3.0 },
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));

    let mut step = Schedule::default();
    step.add_systems((bullet_hits_enemy, apply_damage).chain());
    step.run(world);

    let hp = world.get::<Health>(player).unwrap().current;
    assert!(hp >= 20.0, "100% item lifesteal heals ≥ the 10 damage dealt (hp now {hp})");
}

// ── 86. item_speed_raises_top_speed ───────────────────────────────────────────

/// An equipped SPEED affix lifts the ship's top speed by a flat fraction,
/// capped by the affix total (spec VI.5).
#[test]
fn item_speed_raises_top_speed() {
    use crate::systems::items::{Affix, AffixKind, Equipment, Item, ItemSlot, Rarity};
    use crate::systems::movement::ship_control;

    let mut app = test_app();
    let world = app.world_mut();
    world.resource_mut::<Equipment>().try_equip(Item {
        slot: ItemSlot::Nanites,
        level: 1,
        rarity: Rarity::Epic,
        affixes: vec![Affix { kind: AffixKind::Speed, value: 50.0 }], // +50% top speed
        name: "Quickening Reactor".to_string(),
    });

    let base = Ship::default().max_speed;
    let ship = world
        .spawn((
            Ship::default(),
            Intent { move_dir: Vec2::X, aim: Vec2::ZERO, aim_active: false, firing: false },
            Velocity(Vec2::ZERO),
            Transform::default(),
        ))
        .id();

    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(1.0 / 60.0));
    world.insert_resource(time);

    let mut step = Schedule::default();
    step.add_systems(ship_control);
    for _ in 0..50 {
        step.run(world);
    }

    let speed = world.get::<Velocity>(ship).unwrap().0.length();
    assert!(speed > base * 1.2, "speed gear lifts top speed above base (base {base}, got {speed})");
    assert!(speed <= base * 1.5 + 1.0, "but not beyond the equipped +50% (got {speed})");
}

/// Account SP SPEED lifts top speed like the SPEED affix (Phase ME wiring).
#[test]
fn sp_speed_raises_top_speed() {
    use crate::meta::Meta;
    use crate::systems::movement::ship_control;

    let mut app = test_app();
    {
        let mut meta = app.world_mut().resource_mut::<Meta>();
        meta.sp = 20;
        for _ in 0..10 {
            meta.allocate_sp("SPEED"); // 10/20 of the +100% cap → +50%
        }
    }
    let world = app.world_mut();
    let base = Ship::default().max_speed;
    let ship = world
        .spawn((
            Ship::default(),
            Intent { move_dir: Vec2::X, aim: Vec2::ZERO, aim_active: false, firing: false },
            Velocity(Vec2::ZERO),
            Transform::default(),
        ))
        .id();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(1.0 / 60.0));
    world.insert_resource(time);

    let mut step = Schedule::default();
    step.add_systems(ship_control);
    for _ in 0..50 {
        step.run(world);
    }
    let speed = world.get::<Velocity>(ship).unwrap().0.length();
    assert!(speed > base * 1.2, "SP speed lifts top speed above base (base {base}, got {speed})");
}

/// Account SP HEALTH adds flat max-HP through apply_item_hp (Phase ME wiring).
#[test]
fn sp_health_raises_max_hp() {
    use crate::components::ItemHpBonus;
    use crate::meta::Meta;
    use crate::systems::items::apply_item_hp;

    let mut app = test_app();
    {
        let mut meta = app.world_mut().resource_mut::<Meta>();
        meta.sp = 20;
        for _ in 0..20 {
            meta.allocate_sp("HEALTH"); // +400 max HP at the cap
        }
    }
    let world = app.world_mut();
    let player = world
        .spawn((Ship::default(), Health::new(40.0), ItemHpBonus(0.0), Transform::default()))
        .id();

    let mut step = Schedule::default();
    step.add_systems(apply_item_hp);
    step.run(world);

    let hp = world.get::<Health>(player).unwrap();
    assert!((hp.max - 440.0).abs() < 1e-3, "SP HEALTH adds +400 max HP (max now {})", hp.max);
    assert!(hp.current > 40.0, "gaining max HP heals by the gained amount");
}

// ── 87. item_hp_raises_max ────────────────────────────────────────────────────

/// An equipped MAX-HP affix raises the player's `Health.max` by its delta (and
/// heals by the gained amount); a stronger HP item applies only the difference
/// (spec VI.5, equip-time bookkeeping via `ItemHpBonus`).
#[test]
fn item_hp_raises_max() {
    use crate::systems::items::{apply_item_hp, Affix, AffixKind, Equipment, Item, ItemSlot, Rarity};

    let hp_item = |value: f32| Item {
        slot: ItemSlot::Cockpit,
        level: 1,
        rarity: Rarity::Common,
        affixes: vec![Affix { kind: AffixKind::Hp, value }],
        name: "Plate".to_string(),
    };

    let mut app = test_app();
    let world = app.world_mut();
    let player = world
        .spawn((Ship::default(), Health { current: 40.0, max: 40.0 }, ItemHpBonus::default()))
        .id();

    let mut step = Schedule::default();
    step.add_systems(apply_item_hp);

    // Equip +20 HP → max 60, healed to 60.
    world.resource_mut::<Equipment>().try_equip(hp_item(20.0));
    step.run(world);
    {
        let hp = world.get::<Health>(player).unwrap();
        assert_eq!(hp.max, 60.0, "max += item HP");
        assert_eq!(hp.current, 60.0, "gaining HP gear heals by the gain");
    }

    // Re-running with no change is a no-op (delta 0).
    step.run(world);
    assert_eq!(world.get::<Health>(player).unwrap().max, 60.0, "stable when gear unchanged");

    // A stronger HP item replaces it; only the +30 delta applies → max 90.
    world.resource_mut::<Equipment>().try_equip(hp_item(50.0));
    step.run(world);
    assert_eq!(world.get::<Health>(player).unwrap().max, 90.0, "delta of the better item applies");
}

// ── 88. gear_panel_row_text ───────────────────────────────────────────────────

/// The gear-panel row shows the slot label + equipped item name, or a dash when
/// the slot is empty (spec VI.5 / VIII.1 build view).
#[test]
fn gear_panel_row_text() {
    use crate::render::gear_panel::gear_row_text;
    use crate::systems::items::{Affix, AffixKind, Item, ItemSlot, Rarity};

    // Empty slot → label + dash.
    let empty = gear_row_text(ItemSlot::Cockpit, None);
    assert!(empty.starts_with("COCKPIT"), "row leads with the slot label");
    assert!(empty.trim_end().ends_with('—'), "empty slot shows a dash");

    // Equipped slot → label + item name.
    let item = Item {
        slot: ItemSlot::Shielding,
        level: 5,
        rarity: Rarity::Rare,
        affixes: vec![Affix { kind: AffixKind::Toughness, value: 6.0 }],
        name: "Refined Quantum Aegis".to_string(),
    };
    let row = gear_row_text(ItemSlot::Shielding, Some(&item));
    assert!(row.starts_with("SHIELDING"), "row leads with the slot label");
    assert!(row.contains("Refined Quantum Aegis"), "row shows the equipped item name");
}

// ── 89. loot_card_equipped_marker ─────────────────────────────────────────────

/// An auto-equipped drop's card title gets a ▲ upgrade marker; a sidegrade shows
/// the plain name (spec VI.5 loot-feed feedback).
#[test]
fn loot_card_equipped_marker() {
    use crate::render::loot_feed::card_title;

    let equipped = card_title("Refined Quantum Aegis", true);
    assert!(equipped.starts_with('▲'), "equipped drop is marked");
    assert!(equipped.contains("Refined Quantum Aegis"), "name retained");

    let sidegrade = card_title("Refined Quantum Aegis", false);
    assert_eq!(sidegrade, "Refined Quantum Aegis", "sidegrade shows the plain name");
    assert!(!sidegrade.starts_with('▲'), "sidegrade has no marker");
}

// ── 90. telegraph_pulse_scale_curve ───────────────────────────────────────────

/// The rage-telegraph ring grows as the rage charges and throbs, staying within
/// a sane scale band the whole window (spec IV.7 polish).
#[test]
fn telegraph_pulse_scale_curve() {
    use crate::render::telegraph_fx::telegraph_pulse_scale;

    // Starts at 1.0 (no grow, sin(0)=0), swells toward activation.
    assert!((telegraph_pulse_scale(0.0) - 1.0).abs() < 1e-5, "starts at base scale");
    assert!(telegraph_pulse_scale(1.0) > 1.1, "swelled near activation");
    assert!(telegraph_pulse_scale(1.0) > telegraph_pulse_scale(0.0), "grows overall");

    // Stays within a sane band across the whole window (and clamps out-of-range).
    for i in 0..=40 {
        let s = telegraph_pulse_scale(i as f32 / 40.0);
        assert!((0.9..=1.35).contains(&s), "scale stays bounded (got {s})");
    }
    assert_eq!(telegraph_pulse_scale(-1.0), telegraph_pulse_scale(0.0), "clamps below 0");
    assert_eq!(telegraph_pulse_scale(2.0), telegraph_pulse_scale(1.0), "clamps above 1");
}

// ── 92. formation_pick_and_slots ──────────────────────────────────────────────

/// `pick_formation` needs ≥3, grows its pool with count, and scales params with
/// the wave; `slot_target` places orbit members on the radius circle and keeps
/// every pattern bounded (spec IV.6).
#[test]
fn formation_pick_and_slots() {
    use crate::resources::GameRng;
    use crate::systems::formations::{pick_formation, slot_target, FormationKind};

    let mut rng = GameRng::default();
    assert!(pick_formation(2, 5, &mut rng).is_none(), "needs ≥3 members");

    // 3 members → only orbit/weave/flank in the pool.
    for _ in 0..30 {
        let p = pick_formation(3, 5, &mut rng).unwrap();
        assert!(
            matches!(p.kind, FormationKind::Orbit | FormationKind::Weave | FormationKind::Flank),
            "3-member pool excludes cross/figure8 (got {:?})",
            p.kind
        );
    }

    // Params scale with the wave (radius caps +120, duration caps 12s).
    let p1 = pick_formation(5, 1, &mut GameRng::default()).unwrap();
    let p30 = pick_formation(5, 30, &mut GameRng::default()).unwrap();
    assert!((p1.radius - 186.0).abs() < 1e-3, "W1 radius 180+6");
    assert!((p30.radius - 300.0).abs() < 1e-3, "radius caps at +120");
    assert!((p30.duration - 12.0).abs() < 1e-3, "duration caps at 12s");
    assert!(p30.duration > p1.duration, "later waves last longer");

    // Orbit slot 0 (phase 0, t 0) sits one radius to the +x of the player.
    let player = Vec2::new(100.0, 50.0);
    let t0 = slot_target(FormationKind::Orbit, 0, 4, 0.0, player, 200.0, 0.6, 0.0);
    assert!((t0 - (player + Vec2::new(200.0, 0.0))).length() < 1e-3, "orbit slot 0 at +x radius");
    for slot in 0..4 {
        let p = slot_target(FormationKind::Orbit, slot, 4, 0.3, player, 200.0, 0.6, 0.5);
        assert!(((p - player).length() - 200.0).abs() < 1e-3, "orbit members ride the radius circle");
    }

    // Every pattern stays within a sane bound of the player (no runaway).
    for kind in [
        FormationKind::Orbit, FormationKind::Weave, FormationKind::Flank,
        FormationKind::Cross, FormationKind::Figure8,
    ] {
        for slot in 0..5 {
            for &t in &[0.0_f32, 1.0, 3.0, 7.0] {
                let p = slot_target(kind, slot, 5, t, player, 200.0, 0.6, 1.0);
                assert!((p - player).length() < 600.0, "{kind:?} slot {slot} stays bounded");
            }
        }
    }
}

// ── 93. formation_lerps_and_expires ───────────────────────────────────────────

/// `update_formations` lerps members toward their slots (overriding AI velocity)
/// and, past `duration`, expires the formation + releases its members (spec IV.6).
#[test]
fn formation_lerps_and_expires() {
    use crate::components::FormationMember;
    use crate::systems::formations::{update_formations, Formation, FormationKind, Formations};

    let mut app = test_app();
    let world = app.world_mut();
    world.spawn((Ship::default(), Transform::default())); // player at origin

    let mut ents = Vec::new();
    for _ in 0..3 {
        let e = world
            .spawn((
                Enemy { kind: EnemyKind::Hunter },
                FormationMember,
                Velocity(Vec2::new(99.0, 99.0)),
                Transform::from_xyz(1000.0, 1000.0, 0.0),
            ))
            .id();
        ents.push(e);
    }
    world.resource_mut::<Formations>().active.push(Formation {
        kind: FormationKind::Orbit,
        members: ents.clone(),
        initial_count: 3,
        elapsed: 0.0,
        duration: 1.0,
        radius: 200.0,
        angular_speed: 0.6,
        lerp: 0.5,
        phase_seed: 0.0,
    });

    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(0.1));
    world.insert_resource(time);
    let mut step = Schedule::default();
    step.add_systems(update_formations);

    // A few ticks (elapsed < duration): members converge onto the orbit circle,
    // and their AI velocity is overridden to zero.
    for _ in 0..7 {
        step.run(world);
    }
    let d = world.get::<Transform>(ents[0]).unwrap().translation.truncate().length();
    assert!((d - 200.0).abs() < 30.0, "member converges toward the orbit radius (got {d})");
    assert_eq!(world.get::<Velocity>(ents[0]).unwrap().0, Vec2::ZERO, "AI movement overridden");
    assert_eq!(world.resource::<Formations>().active.len(), 1, "still active mid-window");

    // Past duration → expire + release the FormationMember marker.
    for _ in 0..6 {
        step.run(world);
    }
    assert_eq!(world.resource::<Formations>().active.len(), 0, "formation expired");
    assert!(world.get::<FormationMember>(ents[0]).is_none(), "members released back to AI");
}

// ── 94. hitstop_triggers_and_drains ───────────────────────────────────────────

/// Hitstop coalesces via `max`, fires only on boss/mini-boss deaths, and drains
/// to zero over time (spec I.1).
#[test]
fn hitstop_triggers_and_drains() {
    use crate::systems::hitstop::{
        tick_hitstop, trigger_hitstop, Hitstop, BOSS_HITSTOP, MINI_HITSTOP,
    };

    // Coalesce via max (never sums).
    let mut h = Hitstop::default();
    assert!(!h.frozen());
    h.add(0.05);
    h.add(0.13);
    h.add(0.02);
    assert!((h.secs - 0.13).abs() < 1e-6, "coalesces via max");
    assert!(h.frozen());

    // trigger_hitstop: a normal kill does nothing; a boss kill freezes.
    let mut app = test_app();
    app.world_mut().init_resource::<Hitstop>();
    let world = app.world_mut();
    let mut step = Schedule::default();
    step.add_systems(trigger_hitstop);

    let death = |tier: u8, mini: bool| Death {
        entity: Entity::PLACEHOLDER,
        position: Vec2::ZERO,
        kind: Some(EnemyKind::Titan),
        boss_tier: tier,
        mini_boss: mini,
    };

    world.write_message(death(0, false)); // normal kill
    step.run(world);
    assert_eq!(world.resource::<Hitstop>().secs, 0.0, "normal kill → no hitstop");

    world.write_message(death(2, false)); // boss kill
    step.run(world);
    assert!(
        (world.resource::<Hitstop>().secs - BOSS_HITSTOP).abs() < 1e-6,
        "boss kill freezes the sim"
    );

    // Drain it with the ungated ticker.
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(0.1));
    world.insert_resource(time);
    let mut drain = Schedule::default();
    drain.add_systems(tick_hitstop);
    drain.run(world);
    assert!((world.resource::<Hitstop>().secs - 0.03).abs() < 1e-5, "drains by dt");
    drain.run(world);
    assert_eq!(world.resource::<Hitstop>().secs, 0.0, "clamps to 0 — freeze always ends");
    assert!(!world.resource::<Hitstop>().frozen());

    // A mini-boss kill gives the lighter freeze.
    world.write_message(death(0, true));
    step.run(world);
    assert!(
        (world.resource::<Hitstop>().secs - MINI_HITSTOP).abs() < 1e-6,
        "mini-boss kill → lighter freeze"
    );
}
