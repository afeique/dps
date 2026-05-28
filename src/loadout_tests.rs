//! Tests for the 4-slot ability loadout (Phase AB): the cooldown bookkeeping,
//! the ability catalog, the default loadout, and the `activate_loadout` dispatch
//! (slot fires, empty/on-cooldown gating, Blink teleport).

use crate::components::loadout::*;
use crate::components::{
    Boss, Bulwark, Enemy, EnemyKind, Frozen, Invulnerable, Mark, SecondWindArmed, Ship,
};
use crate::systems::abilities::{activate_loadout, tick_ability_fields};
use bevy::prelude::*;
use std::time::Duration;

#[test]
fn cooldowns_tick_trigger_and_fraction() {
    let mut cds = AbilityCooldowns::default();
    assert!(cds.is_ready(0), "fresh slot is ready");
    assert_eq!(cds.fraction(0), 0.0);

    cds.trigger(0, 20.0);
    assert!(!cds.is_ready(0), "triggered slot is on cooldown");
    assert!((cds.fraction(0) - 1.0).abs() < 1e-6, "just-triggered → full");

    cds.tick(5.0);
    assert!((cds.remaining[0] - 15.0).abs() < 1e-6);
    assert!((cds.fraction(0) - 0.75).abs() < 1e-6);

    cds.tick(100.0);
    assert!(cds.is_ready(0), "cooldown elapsed → ready");
    assert_eq!(cds.fraction(0), 0.0);
}

#[test]
fn out_of_range_slot_is_never_ready() {
    let cds = AbilityCooldowns::default();
    assert!(!cds.is_ready(4));
    assert!(!cds.is_ready(99));
}

#[test]
fn default_loadout_is_the_four_implemented_abilities() {
    let eq = EquippedAbilities::default();
    assert_eq!(
        eq.0,
        [
            Some(Ability::Bulwark),
            Some(Ability::FieldMedic),
            Some(Ability::DeflectorOrbs),
            Some(Ability::EmpPulse),
        ]
    );
    for a in eq.0.into_iter().flatten() {
        assert!(a.implemented(), "{a:?} is default-equipped but not wired");
    }
}

#[test]
fn ability_catalog_is_complete_and_well_formed() {
    assert_eq!(Ability::ALL.len(), 14);
    for a in Ability::ALL {
        assert!(a.cooldown() > 0.0, "{a:?} needs a positive cooldown");
        assert!(!a.name().is_empty());
    }
    let wired = Ability::ALL.iter().filter(|a| a.implemented()).count();
    assert_eq!(wired, 13, "all but Sentry Drone are wired");
}

#[test]
fn infusion_cycle_wraps_and_skips_kinetic() {
    use crate::combat::element::Element;
    use crate::systems::weapons::next_infusion_element;
    assert_eq!(next_infusion_element(None), Element::Pyro);
    assert_eq!(next_infusion_element(Some(Element::Pyro)), Element::Cryo);
    assert_eq!(next_infusion_element(Some(Element::Radiant)), Element::Pyro);
    // Kinetic isn't in the cycle → falls back to the first element.
    assert_eq!(next_infusion_element(Some(Element::Kinetic)), Element::Pyro);
}

#[test]
fn field_params_match_weapon_data() {
    assert_eq!(
        Ability::CryoField.field_params(),
        Some((FieldStatus::Freeze, 180.0, 0.25))
    );
    assert_eq!(
        Ability::StasisField.field_params(),
        Some((FieldStatus::Chill, 210.0, 0.25))
    );
    assert_eq!(
        Ability::StormCell.field_params(),
        Some((FieldStatus::Conduct, 200.0, 0.30))
    );
    assert_eq!(
        Ability::PyreAura.field_params(),
        Some((FieldStatus::Burn, 190.0, 0.40))
    );
    assert_eq!(Ability::Bulwark.field_params(), None);
}

/// A bare world with the loadout resources, a 16 ms clock, and a ship at origin.
fn world_with_ship() -> (World, Entity) {
    let mut world = World::new();
    world.insert_resource(EquippedAbilities::default());
    world.insert_resource(AbilityCooldowns::default());
    world.insert_resource(crate::systems::weapons::ElementInfusion::default());
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);
    let ship = world
        .spawn((Ship::default(), Transform::from_xyz(0.0, 0.0, 0.0)))
        .id();
    (world, ship)
}

fn run_with_key(world: &mut World, key: KeyCode) {
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(key);
    world.insert_resource(keys);
    let mut step = Schedule::default();
    step.add_systems(activate_loadout);
    step.run(world);
}

#[test]
fn numpad1_fires_bulwark_and_sets_cooldown() {
    let (mut world, ship) = world_with_ship();
    run_with_key(&mut world, KeyCode::Numpad1);

    assert!(world.get::<Bulwark>(ship).is_some(), "Numpad1 fires Bulwark");
    let cds = world.resource::<AbilityCooldowns>();
    assert!(!cds.is_ready(0), "slot 0 now on cooldown");
    assert!(cds.is_ready(1), "other slots unaffected");
}

#[test]
fn slot_on_cooldown_does_not_refire() {
    let (mut world, ship) = world_with_ship();
    world.resource_mut::<AbilityCooldowns>().trigger(0, 20.0);

    run_with_key(&mut world, KeyCode::Numpad1);
    assert!(
        world.get::<Bulwark>(ship).is_none(),
        "an on-cooldown slot is a no-op"
    );
}

#[test]
fn empty_slot_is_a_no_op() {
    let (mut world, ship) = world_with_ship();
    world.insert_resource(EquippedAbilities([None, None, None, None]));

    run_with_key(&mut world, KeyCode::Numpad1);
    assert!(world.get::<Bulwark>(ship).is_none());
    assert!(
        world.resource::<AbilityCooldowns>().is_ready(0),
        "no ability → no cooldown spent"
    );
}

#[test]
fn blink_teleports_forward_with_iframe() {
    let (mut world, ship) = world_with_ship();
    // Slot 0 → Blink; default ship rotation faces +Y.
    world.insert_resource(EquippedAbilities([Some(Ability::Blink), None, None, None]));

    run_with_key(&mut world, KeyCode::Numpad1);
    let tf = world.get::<Transform>(ship).unwrap();
    assert!(
        (tf.translation.y - 220.0).abs() < 1e-3,
        "blinks 220px along +Y facing (y = {})",
        tf.translation.y
    );
    assert!(
        world.get::<Invulnerable>(ship).is_some(),
        "blink grants i-frames"
    );
}

#[test]
fn cryo_field_ability_drops_a_field_zone() {
    let (mut world, _ship) = world_with_ship();
    world.insert_resource(EquippedAbilities([Some(Ability::CryoField), None, None, None]));

    run_with_key(&mut world, KeyCode::Numpad1);
    let mut q = world.query::<&AbilityField>();
    assert_eq!(q.iter(&world).count(), 1, "CryoField drops one field zone");
    assert!(
        !world.resource::<AbilityCooldowns>().is_ready(0),
        "slot 0 on cooldown after dropping the field"
    );
}

/// A bare world with a 16 ms clock (no ship needed — fields tick on their own).
fn world_with_clock(ms: u64) -> World {
    let mut world = World::new();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(ms));
    world.insert_resource(time);
    world
}

#[test]
fn cryo_field_freezes_enemies_inside_radius_only() {
    let mut world = world_with_clock(16);
    world.spawn((
        AbilityField {
            status: FieldStatus::Freeze,
            radius: 180.0,
            secs: 5.0,
            tick: 0.25,
            timer: 0.0,
        },
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));
    let inside = world
        .spawn((Enemy { kind: EnemyKind::Hunter }, Transform::from_xyz(100.0, 0.0, 0.0)))
        .id();
    let outside = world
        .spawn((Enemy { kind: EnemyKind::Hunter }, Transform::from_xyz(300.0, 0.0, 0.0)))
        .id();

    let mut step = Schedule::default();
    step.add_systems(tick_ability_fields);
    step.run(&mut world);

    assert!(world.get::<Frozen>(inside).is_some(), "enemy inside the field is frozen");
    assert!(world.get::<Frozen>(outside).is_none(), "enemy outside the radius is not");
}

#[test]
fn ability_field_despawns_when_lifetime_elapses() {
    let mut world = world_with_clock(100); // 0.1 s step
    let field = world
        .spawn((
            AbilityField {
                status: FieldStatus::Burn,
                radius: 100.0,
                secs: 0.05, // shorter than the step → expires this tick
                tick: 0.4,
                timer: 0.0,
            },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();

    let mut step = Schedule::default();
    step.add_systems(tick_ability_fields);
    step.run(&mut world);

    assert!(
        world.get::<AbilityField>(field).is_none(),
        "the field despawns once its lifetime elapses"
    );
}

#[test]
fn gravity_snare_yanks_nonboss_enemies_inward() {
    let (mut world, _ship) = world_with_ship(); // ship at origin
    world.insert_resource(EquippedAbilities([Some(Ability::GravitySnare), None, None, None]));
    // Within r320, beyond minDist70: pull = min(200-70, 200*0.6) = 120 → x: 200→80.
    let near = world
        .spawn((Enemy { kind: EnemyKind::Hunter }, Transform::from_xyz(200.0, 0.0, 0.0)))
        .id();
    // Outside r320: unmoved.
    let far = world
        .spawn((Enemy { kind: EnemyKind::Hunter }, Transform::from_xyz(500.0, 0.0, 0.0)))
        .id();

    run_with_key(&mut world, KeyCode::Numpad1);

    let nx = world.get::<Transform>(near).unwrap().translation.x;
    let fx = world.get::<Transform>(far).unwrap().translation.x;
    assert!((nx - 80.0).abs() < 1e-3, "snared enemy pulled to x=80 (got {nx})");
    assert!((fx - 500.0).abs() < 1e-3, "out-of-range enemy unmoved (got {fx})");
}

#[test]
fn gravity_snare_ignores_bosses() {
    let (mut world, _ship) = world_with_ship();
    world.insert_resource(EquippedAbilities([Some(Ability::GravitySnare), None, None, None]));
    let boss = world
        .spawn((
            Enemy { kind: EnemyKind::Hunter },
            Boss { tier: 1 },
            Transform::from_xyz(200.0, 0.0, 0.0),
        ))
        .id();

    run_with_key(&mut world, KeyCode::Numpad1);

    let bx = world.get::<Transform>(boss).unwrap().translation.x;
    assert!((bx - 200.0).abs() < 1e-3, "a boss is immune to the snare (got {bx})");
}

#[test]
fn designator_marks_enemies_in_radius_only() {
    let (mut world, _ship) = world_with_ship();
    world.insert_resource(EquippedAbilities([Some(Ability::Designator), None, None, None]));
    let near = world
        .spawn((Enemy { kind: EnemyKind::Hunter }, Transform::from_xyz(300.0, 0.0, 0.0)))
        .id(); // within r360
    let far = world
        .spawn((Enemy { kind: EnemyKind::Hunter }, Transform::from_xyz(400.0, 0.0, 0.0)))
        .id(); // outside r360

    run_with_key(&mut world, KeyCode::Numpad1);

    assert!(world.get::<Mark>(near).is_some(), "enemy within designator radius is marked");
    assert!(world.get::<Mark>(far).is_none(), "enemy outside the radius is not");
}

#[test]
fn second_wind_arms_a_death_save() {
    let (mut world, ship) = world_with_ship();
    world.insert_resource(EquippedAbilities([Some(Ability::SecondWind), None, None, None]));

    run_with_key(&mut world, KeyCode::Numpad1);
    assert!(
        world.get::<SecondWindArmed>(ship).is_some(),
        "casting Second Wind arms the death save"
    );
    assert!(!world.resource::<AbilityCooldowns>().is_ready(0), "slot 0 on cooldown");
}

#[test]
fn elemental_infusion_sets_the_override() {
    use crate::combat::element::Element;
    use crate::systems::weapons::ElementInfusion;

    let (mut world, _ship) = world_with_ship();
    world.insert_resource(EquippedAbilities([
        Some(Ability::ElementalInfusion),
        None,
        None,
        None,
    ]));

    run_with_key(&mut world, KeyCode::Numpad1);
    let inf = world.resource::<ElementInfusion>();
    assert_eq!(inf.element, Some(Element::Pyro), "first cast infuses Pyro");
    assert!((inf.secs - 8.0).abs() < 1e-3, "8s infusion duration");
}
