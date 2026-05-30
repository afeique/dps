//! Phase 1 exit-gate scenario tests (headless, per `docs/port-plan.md` §9).
//!
//! Each test spins a minimal `World` with only the sim systems under test,
//! drives a few fixed steps via an ad-hoc `Schedule`, and asserts on the
//! resulting `World` — no plugins, no rendering, no runner. This is exactly the
//! property §1 buys us: simulation never touches rendering, so it's testable
//! headless. These lock in the gate's "take damage / die" loop before Phase 2
//! builds on it.

use bevy::prelude::*;
use std::time::Duration;

use crate::components::*;
use crate::messages::{Damage, Death};
use crate::resources::Score;
use crate::states::GameState;
use crate::systems::collision::{bullet_hits_enemy, enemy_contact_player};
use crate::systems::damage::{apply_damage, tick_invulnerability};

/// A bare `App` carrying just the message buffers + resources the damage
/// pipeline needs. We never call `app.update()`; instead we run our own
/// `Schedule` against `app.world_mut()`, so nothing consumes `NextState` and
/// the assertions stay deterministic.
fn test_app() -> App {
    let mut app = App::new();
    app.add_message::<Damage>()
        .add_message::<Death>()
        .add_message::<crate::messages::Knockback>()
        .add_message::<crate::messages::PlayerHurt>()
        .add_message::<crate::messages::Crit>()
        .add_message::<crate::messages::Dodged>()
        .add_message::<crate::messages::Shard>()
        .add_message::<crate::messages::Reaction>()
        .init_resource::<crate::meta::Meta>()
        .init_resource::<Score>()
        .init_resource::<crate::resources::KillStreak>()
        .init_resource::<crate::resources::GameRng>()
        .init_resource::<crate::resources::EnergyMeter>()
        .init_resource::<crate::systems::shop::Upgrades>()
        .init_resource::<crate::systems::items::Equipment>()
        .init_resource::<crate::combat::reaction::PendingReactions>()
        .init_resource::<crate::resources::LastStandUsed>()
        .insert_resource(NextState::<GameState>::Unchanged);
    app
}

fn spawn_player(world: &mut World, hp: f32) -> Entity {
    world
        .spawn((
            Ship::default(),
            Health::new(hp),
            Velocity::default(),
            Collider { radius: 16.0 },
            Faction::Player,
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id()
}

/// A drifter sitting at `pos` (use `Vec2::ZERO` to overlap the player).
fn spawn_enemy_at(world: &mut World, pos: Vec2) -> Entity {
    world
        .spawn((
            Enemy {
                kind: EnemyKind::Drifter,
            },
            Health::new(30.0),
            Velocity::default(),
            Collider { radius: 18.0 },
            Faction::Enemy,
            Transform::from_xyz(pos.x, pos.y, 0.0),
        ))
        .id()
}

#[test]
fn enemy_contact_damages_player_then_separation_blocks_the_next_hit() {
    let mut app = test_app();
    let world = app.world_mut();
    let player = spawn_player(world, 100.0); // no Shield in this fixture → full dmg
    spawn_enemy_at(world, Vec2::ZERO); // overlapping the player

    let mut step = Schedule::default();
    step.add_systems((enemy_contact_player, apply_damage).chain());

    // Tick 1: one contact lands 25 dmg and pushes the two bodies apart — and,
    // per spec II.2 (i-frames removed), grants NO post-hit invulnerability.
    step.run(world);
    assert_eq!(world.get::<Health>(player).unwrap().current, 75.0);
    assert!(
        world.get::<Invulnerable>(player).is_none(),
        "no post-hit i-frames are granted (spec 5.88.0)"
    );

    // Tick 2: the separation (not i-frames) has ended the overlap, so no second
    // contact lands this tick. (No `integrate` runs here, so the push persists.)
    step.run(world);
    assert_eq!(
        world.get::<Health>(player).unwrap().current,
        75.0,
        "physical separation, not i-frames, prevents the immediate re-hit"
    );
}

#[test]
fn iframes_expire_after_their_window() {
    let mut app = test_app();
    let world = app.world_mut();
    let player = spawn_player(world, 100.0);
    world
        .entity_mut(player)
        .insert(Invulnerable { seconds: 0.6 });

    // One fixed tick of 1.0 s elapses — past the 0.6 s window.
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(1.0));
    world.insert_resource(time);

    let mut step = Schedule::default();
    step.add_systems(tick_invulnerability);
    step.run(world);

    assert!(
        world.get::<Invulnerable>(player).is_none(),
        "i-frames should be removed once their window elapses"
    );
}

#[test]
fn player_lethal_hit_revives_commander_not_gameover() {
    // Tower-defense conversion: the commander ship is no longer the lose
    // condition. A lethal hit revives it at full HP with a brief invuln window
    // instead of despawning + flipping to GameOver — only the Core dying ends
    // the run (`tower::core_lose_check`).
    let mut app = test_app();
    let world = app.world_mut();
    let player = spawn_player(world, 10.0); // would die to one 25-dmg contact
    spawn_enemy_at(world, Vec2::ZERO);

    let mut step = Schedule::default();
    step.add_systems((enemy_contact_player, apply_damage).chain());
    step.run(world);

    let hp = world
        .get::<Health>(player)
        .expect("the commander is revived, not despawned");
    assert!(
        (hp.current - hp.max).abs() < 0.01,
        "revive restores full HP (got {}/{})",
        hp.current,
        hp.max
    );
    assert!(
        world.get::<Invulnerable>(player).is_some(),
        "revive grants a brief invuln window"
    );
    assert_eq!(
        world.resource::<Score>().kills,
        0,
        "the player's own near-death is not a kill"
    );
    assert!(
        matches!(
            world.resource::<NextState<GameState>>(),
            NextState::Unchanged
        ),
        "a lethal hit on the commander does NOT queue GameOver"
    );
}

#[test]
fn player_bullet_kills_enemy_and_scores_one() {
    let mut app = test_app();
    let world = app.world_mut();
    let enemy = world
        .spawn((
            Enemy {
                kind: EnemyKind::Drifter,
            },
            Health::new(10.0),
            Collider { radius: 18.0 },
            Faction::Enemy,
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();
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
        "the enemy should be despawned by a lethal hit"
    );
    assert!(
        world.get::<Bullet>(bullet).is_none(),
        "the bullet should be despawned on impact"
    );
    assert_eq!(world.resource::<Score>().kills, 1);
}
