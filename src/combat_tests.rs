//! Unit tests for the `combat` module (Phase E onward). Ported alongside the
//! JS `tests/unit/combat/*` invariants. Element/resistance math first.

use crate::combat::element::{
    elemental_multiplier, resolve_bullet_elements, Element, ElementSet, Resistances, ELEMENT_COUNT,
};
use crate::systems::collision::{enemy_defense_damage, frontal_blocked};
use bevy::math::Vec2;

// ─── Element taxonomy ────────────────────────────────────────────────────────

#[test]
fn element_all_has_seven_and_distinct_indices() {
    assert_eq!(Element::ALL.len(), ELEMENT_COUNT);
    let mut idxs: Vec<usize> = Element::ALL.iter().map(|e| e.idx()).collect();
    idxs.sort_unstable();
    assert_eq!(idxs, (0..ELEMENT_COUNT).collect::<Vec<_>>());
}

#[test]
fn element_id_roundtrips() {
    for e in Element::ALL {
        assert_eq!(Element::from_id(e.id()), Some(e));
    }
    assert_eq!(Element::from_id("NONSENSE"), None);
}

#[test]
fn only_kinetic_has_no_signature_status() {
    assert_eq!(Element::Kinetic.status_id(), None);
    for e in Element::ALL.iter().filter(|e| **e != Element::Kinetic) {
        assert!(e.status_id().is_some(), "{} should carry a status", e.name());
    }
    // Spot-check the exact JS status ids.
    assert_eq!(Element::Pyro.status_id(), Some("BRN"));
    assert_eq!(Element::Radiant.status_id(), Some("PURGE"));
}

// ─── elemental_multiplier (resist → damage mult) ─────────────────────────────

#[test]
fn elemental_multiplier_neutral_resist_weak_immune() {
    assert_eq!(elemental_multiplier(0.0), 1.0); // neutral
    assert_eq!(elemental_multiplier(0.5), 0.5); // resistant
    assert_eq!(elemental_multiplier(1.0), 0.0); // immune
    assert_eq!(elemental_multiplier(-0.5), 1.5); // weak → bonus
}

#[test]
fn elemental_multiplier_clamps_to_0_2() {
    assert_eq!(elemental_multiplier(-3.0), 2.0); // a huge weakness caps at +100%
    assert_eq!(elemental_multiplier(5.0), 0.0); // over-resist floors at 0
}

// ─── Resistances ─────────────────────────────────────────────────────────────

#[test]
fn default_resistances_are_all_neutral() {
    let r = Resistances::new();
    for e in Element::ALL {
        assert_eq!(r.multiplier(e), 1.0);
    }
}

#[test]
fn with_builder_sets_per_element_resist() {
    let r = Resistances::new()
        .with(Element::Pyro, 0.6)
        .with(Element::Cryo, -0.4);
    assert!((r.multiplier(Element::Pyro) - 0.4).abs() < 1e-6); // 1 - 0.6
    assert!((r.multiplier(Element::Cryo) - 1.4).abs() < 1e-6); // 1 - (-0.4)
    assert_eq!(r.multiplier(Element::Volt), 1.0); // untouched → neutral
}

#[test]
fn multi_multiplier_averages_per_element() {
    let r = Resistances::new()
        .with(Element::Pyro, 0.5) // mult 0.5
        .with(Element::Cryo, -0.5); // mult 1.5
    assert_eq!(r.multi_multiplier(&[]), 1.0); // none → neutral
    assert_eq!(r.multi_multiplier(&[Element::Pyro]), 0.5); // single = elemental_multiplier
    // average of 0.5 and 1.5 = 1.0 (coverage cancels focus)
    assert!((r.multi_multiplier(&[Element::Pyro, Element::Cryo]) - 1.0).abs() < 1e-6);
}

#[test]
fn weakness_finds_most_negative_beyond_threshold() {
    let r = Resistances::new()
        .with(Element::Cryo, -0.4)
        .with(Element::Toxic, -0.6); // the bigger weakness
    assert_eq!(r.weakest(), Some(Element::Toxic));
    // No weakness past the −0.3 default threshold → None.
    let r2 = Resistances::new().with(Element::Pyro, 0.5).with(Element::Volt, -0.2);
    assert_eq!(r2.weakest(), None);
}

#[test]
fn adapt_bumps_toward_cap_and_clamps() {
    let mut r = Resistances::new();
    r.adapt_default(Element::Pyro); // +0.12
    assert!((r.get(Element::Pyro) - 0.12).abs() < 1e-6);
    for _ in 0..20 {
        r.adapt_default(Element::Pyro);
    }
    assert!((r.get(Element::Pyro) - 0.75).abs() < 1e-6); // capped at 0.75
}

#[test]
fn decay_scales_adapted_resist_toward_zero_and_snaps() {
    let mut r = Resistances::new();
    r.adapt(Element::Volt, 0.5, 0.75); // adapted to 0.5
    r.decay_default(); // ×0.8 → 0.40
    assert!((r.get(Element::Volt) - 0.4).abs() < 1e-6);
    // Decay until it snaps to 0 once at/under the 0.02 floor.
    for _ in 0..20 {
        r.decay_default();
    }
    assert_eq!(r.get(Element::Volt), 0.0);
}

#[test]
fn decay_leaves_base_weakness_untouched() {
    let mut r = Resistances::new().with(Element::Cryo, -0.4);
    r.decay_default();
    assert!((r.get(Element::Cryo) - (-0.4)).abs() < 1e-6); // negatives are not decayed
}

// ─── resolve_bullet_elements ─────────────────────────────────────────────────

#[test]
fn resolve_override_replaces_all() {
    let out = resolve_bullet_elements(
        Some(Element::Radiant),
        &[Element::Pyro, Element::Cryo],
        Element::Kinetic,
    );
    assert_eq!(out, vec![Element::Radiant]);
}

#[test]
fn resolve_dedups_attunements_preserving_order() {
    let out = resolve_bullet_elements(
        None,
        &[Element::Pyro, Element::Volt, Element::Pyro],
        Element::Kinetic,
    );
    assert_eq!(out, vec![Element::Pyro, Element::Volt]);
}

#[test]
fn resolve_falls_back_to_base_element() {
    let out = resolve_bullet_elements(None, &[], Element::Void);
    assert_eq!(out, vec![Element::Void]);
}

// ─── E4 enemy-side defense pipeline ──────────────────────────────────────────

#[test]
fn defense_no_modifiers_is_identity() {
    assert_eq!(
        enemy_defense_damage(10.0, 0, false, false, false, 0.0, 0.0, false),
        10.0
    );
}

#[test]
fn defense_corrode_and_conduct_amplify() {
    // corrode 2 stacks → ×(1 + 0.15·2) = ×1.30
    assert!((enemy_defense_damage(10.0, 2, false, false, false, 0.0, 0.0, false) - 13.0).abs() < 1e-4);
    // conducting + a VOLT hit → ×1.5
    assert!((enemy_defense_damage(10.0, 0, true, true, false, 0.0, 0.0, false) - 15.0).abs() < 1e-4);
    // conducting but a non-VOLT hit → unchanged
    assert_eq!(
        enemy_defense_damage(10.0, 0, true, false, false, 0.0, 0.0, false),
        10.0
    );
}

#[test]
fn defense_armor_floors_at_25_percent() {
    // small hit vs armor 1.0 → max(1.2·0.25, 1.2−1.0) = 0.3 (chip floored, not nullified)
    assert!((enemy_defense_damage(1.2, 0, false, false, false, 1.0, 0.0, false) - 0.3).abs() < 1e-4);
    // big hit punches through → max(10·0.25, 10−1) = 9.0
    assert!((enemy_defense_damage(10.0, 0, false, false, false, 1.0, 0.0, false) - 9.0).abs() < 1e-4);
}

#[test]
fn defense_radiant_purge_bypasses_armor_and_frontal() {
    // RADIANT skips both flat armor and the frontal-shield block
    assert!((enemy_defense_damage(1.2, 0, false, false, true, 1.0, 0.8, true) - 1.2).abs() < 1e-4);
}

#[test]
fn defense_frontal_shield_reduces_blocked_hits() {
    // blocked → ×(1 − 0.8) = ×0.2
    assert!((enemy_defense_damage(10.0, 0, false, false, false, 0.0, 0.8, true) - 2.0).abs() < 1e-4);
    // not blocked (flank/bounce) → unchanged
    assert_eq!(
        enemy_defense_damage(10.0, 0, false, false, false, 0.0, 0.8, false),
        10.0
    );
}

#[test]
fn frontal_blocked_geometry() {
    let enemy = Vec2::ZERO;
    let player = Vec2::new(0.0, 100.0);
    // a shot from the player's bearing is blocked
    assert!(frontal_blocked(enemy, player, Vec2::new(0.0, 50.0), 2.4));
    // a 90° flank shot gets through (1.57 rad > arc/2 = 1.2)
    assert!(!frontal_blocked(enemy, player, Vec2::new(50.0, 0.0), 2.4));
    // a shot from directly behind gets through
    assert!(!frontal_blocked(enemy, player, Vec2::new(0.0, -50.0), 2.4));
    // coincident points never block
    assert!(!frontal_blocked(enemy, player, enemy, 2.4));
}

// ─── ElementSet (the on-bullet bitset, E2) ───────────────────────────────────

#[test]
fn element_set_insert_contains_len_iter() {
    let mut s = ElementSet::EMPTY;
    assert!(s.is_empty());
    s.insert(Element::Pyro);
    s.insert(Element::Pyro); // idempotent (a set)
    s.insert(Element::Volt);
    assert_eq!(s.len(), 2);
    assert!(s.contains(Element::Pyro));
    assert!(!s.contains(Element::Cryo));
    // iter yields in Element::ALL order regardless of insert order
    assert_eq!(s.iter().collect::<Vec<_>>(), vec![Element::Pyro, Element::Volt]);
}

#[test]
fn element_set_kinetic_and_from_slice_dedup() {
    assert_eq!(ElementSet::kinetic(), ElementSet::single(Element::Kinetic));
    let s = ElementSet::from_slice(&[Element::Pyro, Element::Pyro, Element::Cryo]);
    assert_eq!(s.len(), 2);
}

#[test]
fn multi_multiplier_set_matches_slice_form() {
    let r = Resistances::new()
        .with(Element::Pyro, 0.5) // mult 0.5
        .with(Element::Cryo, -0.5); // mult 1.5
    assert_eq!(r.multi_multiplier_set(ElementSet::EMPTY), 1.0);
    assert!((r.multi_multiplier_set(ElementSet::single(Element::Pyro)) - 0.5).abs() < 1e-6);
    let both = ElementSet::from_slice(&[Element::Pyro, Element::Cryo]);
    assert!((r.multi_multiplier_set(both) - 1.0).abs() < 1e-6); // average
}

/// E2 sanity: a Kinetic shot vs the real per-enemy resist directions.
/// Guardian resists Kinetic (0.30 → ×0.7); Sentinel is weak to Kinetic
/// (−0.30 → ×1.3); Hunter is neutral (×1.0).
#[test]
fn kinetic_shot_respects_guardian_sentinel_hunter_resist() {
    let kinetic = ElementSet::kinetic();
    let guardian = Resistances::new().with(Element::Kinetic, 0.30).with(Element::Volt, -0.40);
    let sentinel = Resistances::new().with(Element::Radiant, 0.50).with(Element::Kinetic, -0.30);
    let hunter = Resistances::new();
    assert!((guardian.multi_multiplier_set(kinetic) - 0.70).abs() < 1e-6);
    assert!((sentinel.multi_multiplier_set(kinetic) - 1.30).abs() < 1e-6);
    assert_eq!(hunter.multi_multiplier_set(kinetic), 1.0);
}
