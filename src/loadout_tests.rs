//! Tests for the 4-slot ability loadout (Phase AB): the cooldown bookkeeping,
//! the ability catalog, the default loadout, and the `activate_loadout` dispatch
//! (slot fires, empty/on-cooldown gating, Blink teleport).

use crate::components::loadout::*;
use crate::components::{Bulwark, Invulnerable, Ship};
use crate::systems::abilities::activate_loadout;
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
    assert_eq!(wired, 5, "exactly five abilities are wired this increment");
}

/// A bare world with the loadout resources, a 16 ms clock, and a ship at origin.
fn world_with_ship() -> (World, Entity) {
    let mut world = World::new();
    world.insert_resource(EquippedAbilities::default());
    world.insert_resource(AbilityCooldowns::default());
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
