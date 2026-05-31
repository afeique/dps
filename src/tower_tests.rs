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

// ── integration: click-to-fire the equipped weapon ───────────────────────────

/// A collector so the test can inspect emitted `Fire` messages.
#[derive(Resource, Default)]
struct FiredShots(Vec<crate::messages::Fire>);

fn collect_fire(
    mut r: MessageReader<crate::messages::Fire>,
    mut out: ResMut<FiredShots>,
) {
    for f in r.read() {
        out.0.push(*f);
    }
}

/// Clicking on an enemy (cursor over it, left-button just pressed, no tower armed)
/// emits one player `Fire` from the Core toward the enemy. Clicking while a tower
/// is armed (placement mode) emits nothing.
#[test]
fn clicking_an_enemy_fires_the_equipped_weapon() {
    use crate::messages::Fire;
    use crate::resources::Aim;
    use crate::systems::shop::Upgrades;
    use crate::systems::tower::SelectedTower;
    use crate::systems::weapons::{manual_fire, CurrentWeapon};

    fn run(armed: Option<TowerKind>) -> Vec<Fire> {
        let mut app = App::new();
        app.add_message::<Fire>()
            .init_resource::<CurrentWeapon>()
            .init_resource::<Upgrades>()
            .init_resource::<FiredShots>();

        let mut mouse = ButtonInput::<MouseButton>::default();
        mouse.press(MouseButton::Left);

        let world = app.world_mut();
        world.insert_resource(mouse);
        world.insert_resource(Time::<()>::default());
        // Cursor sits on the enemy at (100, 0).
        world.insert_resource(Aim { world: Vec2::new(100.0, 0.0), active: true });
        world.insert_resource(SelectedTower { kind: armed });
        world.spawn((Core, Transform::from_xyz(0.0, 0.0, 0.0)));
        world.spawn((
            Enemy { kind: EnemyKind::Drifter },
            Collider { radius: 16.0 },
            Transform::from_xyz(100.0, 0.0, 0.0),
        ));

        let mut step = Schedule::default();
        step.add_systems((manual_fire, collect_fire).chain());
        step.run(world);
        world.resource::<FiredShots>().0.clone()
    }

    // Not placing → one shot from the Core (origin ~0,0) aimed at the enemy (+X).
    let shots = run(None);
    assert_eq!(shots.len(), 1, "a click on an enemy fires once");
    let f = shots[0];
    assert!(matches!(f.faction, Faction::Player), "the shot is player-faction");
    assert!(f.origin.length() < 0.01, "it originates at the Core");
    assert!(f.dir.x > 0.9 && f.dir.y.abs() < 0.1, "aimed at the enemy on the +X side (dir {:?})", f.dir);

    // Placement mode (a tower armed) → the click builds, not shoots.
    assert!(run(Some(TowerKind::Gun)).is_empty(), "no manual fire while placing a tower");
}

/// End-to-end: a click on an enemy runs `manual_fire` → `spawn_bullets`, yielding
/// a real player bullet flying toward the enemy with the equipped weapon's build.
#[test]
fn manual_fire_chains_to_a_player_bullet() {
    use crate::messages::Fire;
    use crate::resources::Aim;
    use crate::systems::shop::Upgrades;
    use crate::systems::tower::SelectedTower;
    use crate::systems::weapons::{
        manual_fire, spawn_bullets, Attunements, CurrentWeapon, ElementInfusion,
    };
    use crate::systems::wave::Wave;

    let mut app = App::new();
    app.add_message::<Fire>()
        .init_resource::<CurrentWeapon>()
        .init_resource::<Attunements>()
        .init_resource::<ElementInfusion>()
        .init_resource::<Upgrades>()
        .init_resource::<SelectedTower>()
        .init_resource::<Wave>()
        .insert_resource(dummy_bullet_assets());

    let mut mouse = ButtonInput::<MouseButton>::default();
    mouse.press(MouseButton::Left);

    let world = app.world_mut();
    world.insert_resource(mouse);
    world.insert_resource(Time::<()>::default());
    world.insert_resource(Aim { world: Vec2::new(120.0, 0.0), active: true });
    world.spawn((Core, Transform::from_xyz(0.0, 0.0, 0.0)));
    world.spawn((
        Enemy { kind: EnemyKind::Drifter },
        Collider { radius: 16.0 },
        Transform::from_xyz(120.0, 0.0, 0.0),
    ));

    // Same-schedule chain so spawn_bullets reads the Fire manual_fire just wrote.
    let mut step = Schedule::default();
    step.add_systems((manual_fire, spawn_bullets).chain());
    step.run(world);

    let mut q = world.query::<(&Bullet, &Velocity, &Faction)>();
    let (bullet, vel, faction) = q
        .iter(world)
        .next()
        .expect("clicking an enemy spawns a player bullet");
    assert!(matches!(bullet.kind, BulletKind::Player));
    assert!(matches!(faction, Faction::Player));
    assert!(vel.0.x > 0.0, "the bullet flies toward the clicked enemy (+X) (vel {:?})", vel.0);
}
