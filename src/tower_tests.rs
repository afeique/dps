//! Tower-defense conversion tests (headless, mirroring `wave_tests.rs`).
//!
//! Covers the new TD mechanics:
//!   • `can_place` placement validation (bounds / core gap / tower spacing)
//!   • `nearest_in_range` turret target acquisition
//!   • `upgrade_cost` / `level_damage_mult` economy curves
//!   • `enemy_contact_core` — an enemy reaching the Core leaks HP + despawns
//!   • `tower_fire` — a ready turret spawns a player projectile at the target

use bevy::prelude::*;
use std::time::Duration;

use crate::combat::element::Element;
use crate::components::{Bullet, BulletKind, Collider, Core, Enemy, EnemyKind, Faction, Health, Velocity};
use crate::render::bullets::BulletAssets;
use crate::systems::collision::{enemy_contact_core, CORE_LEAK_DAMAGE};
use crate::systems::tower::{
    can_place, level_damage_mult, nearest_in_range, tower_fire, upgrade_cost, Tower, TowerKind,
};

// ── pure helpers ──────────────────────────────────────────────────────────────

const BOUNDS: Vec2 = Vec2::new(640.0, 360.0);

#[test]
fn can_place_accepts_open_ground() {
    assert!(
        can_place(Vec2::new(200.0, 0.0), Vec2::ZERO, &[], BOUNDS),
        "open spot clear of the core + edges is buildable"
    );
}

#[test]
fn can_place_rejects_on_top_of_core() {
    // Inside MIN_CORE_GAP of the centre.
    assert!(!can_place(Vec2::new(40.0, 0.0), Vec2::ZERO, &[], BOUNDS));
}

#[test]
fn can_place_rejects_out_of_bounds() {
    assert!(!can_place(Vec2::new(700.0, 0.0), Vec2::ZERO, &[], BOUNDS));
    assert!(!can_place(Vec2::new(0.0, 400.0), Vec2::ZERO, &[], BOUNDS));
}

#[test]
fn can_place_rejects_stacking_on_another_tower() {
    let existing = [Vec2::new(210.0, 0.0)];
    assert!(
        !can_place(Vec2::new(200.0, 0.0), Vec2::ZERO, &existing, BOUNDS),
        "10px from an existing tower violates spacing"
    );
    assert!(
        can_place(Vec2::new(300.0, 0.0), Vec2::ZERO, &existing, BOUNDS),
        "100px away is fine"
    );
}

#[test]
fn nearest_in_range_picks_closest_within_range() {
    let candidates = [Vec2::new(90.0, 0.0), Vec2::new(50.0, 0.0), Vec2::new(220.0, 0.0)];
    assert_eq!(
        nearest_in_range(Vec2::ZERO, 100.0, &candidates),
        Some(Vec2::new(50.0, 0.0)),
        "nearest in-range target wins; the 220px one is out of range"
    );
}

#[test]
fn nearest_in_range_none_when_all_out_of_range_or_empty() {
    assert_eq!(nearest_in_range(Vec2::ZERO, 100.0, &[Vec2::new(150.0, 0.0)]), None);
    assert_eq!(nearest_in_range(Vec2::ZERO, 100.0, &[]), None);
}

#[test]
fn economy_curves_increase_with_level() {
    assert!(upgrade_cost(TowerKind::Gun, 1) > upgrade_cost(TowerKind::Gun, 0));
    assert!((level_damage_mult(0) - 1.0).abs() < 1e-6);
    assert!(level_damage_mult(2) > level_damage_mult(1));
}

#[test]
fn tower_kinds_carry_their_elements() {
    assert_eq!(TowerKind::Frost.element(), Element::Cryo);
    assert_eq!(TowerKind::Inferno.element(), Element::Pyro);
    assert_eq!(TowerKind::Gun.element(), Element::Kinetic);
}

// ── integration: enemy reaching the Core ──────────────────────────────────────

#[test]
fn enemy_reaching_core_leaks_and_despawns() {
    let mut app = App::new();
    let world = app.world_mut();

    let core = world
        .spawn((
            Core,
            Health::new(1000.0),
            Collider { radius: 54.0 },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();
    // An enemy overlapping the Core (reach = 54 + 12 = 66 > 20).
    let enemy = world
        .spawn((
            Enemy { kind: EnemyKind::Drifter },
            Collider { radius: 12.0 },
            Transform::from_xyz(20.0, 0.0, 0.0),
        ))
        .id();

    let mut step = Schedule::default();
    step.add_systems(enemy_contact_core);
    step.run(world);

    let hp = world.get::<Health>(core).expect("core survives one leak");
    assert!(
        (hp.current - (1000.0 - CORE_LEAK_DAMAGE)).abs() < 0.01,
        "core lost one leak's worth of HP (got {})",
        hp.current
    );
    assert!(
        world.get::<Enemy>(enemy).is_none(),
        "the leaked enemy is consumed (despawned)"
    );
}

// ── integration: turret fire ──────────────────────────────────────────────────

/// Dummy bullet assets (default handles) so `tower_fire` can spawn projectiles
/// in a headless world with no rendering plugins.
fn dummy_bullet_assets() -> BulletAssets {
    BulletAssets {
        circle: Handle::default(),
        player_body: Handle::default(),
        player_core: Handle::default(),
        enemy_body: Handle::default(),
        player_trail: Handle::default(),
    }
}

#[test]
fn ready_tower_fires_player_bullet_at_target() {
    let mut app = App::new();
    let world = app.world_mut();
    world.insert_resource(dummy_bullet_assets());

    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(0.1));
    world.insert_resource(time);

    // A fresh Gun (timer 0 → ready) at origin, enemy to its right within range.
    world.spawn((Tower::new(TowerKind::Gun), Transform::from_xyz(0.0, 0.0, 0.0)));
    world.spawn((
        Enemy { kind: EnemyKind::Drifter },
        Collider { radius: 12.0 },
        Transform::from_xyz(100.0, 0.0, 0.0),
    ));

    let mut step = Schedule::default();
    step.add_systems(tower_fire);
    step.run(world);

    let mut q = world.query::<(&Bullet, &Velocity, &Faction)>();
    let shot = q.iter(world).next().expect("the ready turret fires a bullet");
    let (bullet, vel, faction) = shot;
    assert!(matches!(bullet.kind, BulletKind::Player), "tower bullets are player-faction");
    assert!(matches!(faction, Faction::Player));
    assert!(vel.0.x > 0.0, "bullet flies toward the enemy on the +X side (vel {:?})", vel.0);
    assert!(vel.0.y.abs() < 1.0, "and roughly straight along X");
}
