//! Unit tests for the meta-progression layer (Phase ME): the XP curve, the
//! multi-level rollover (each level → +1 SP), the level cap, gold banking, and
//! the RON round-trip used by the disk save. These are pure-logic tests — no
//! `App`/`World` needed.

use crate::meta::{MAX_LEVEL, Meta, xp_for_level};

#[test]
fn xp_curve_matches_js_formula() {
    // `500 + (level − 1) × 250`.
    assert_eq!(xp_for_level(1), 500);
    assert_eq!(xp_for_level(2), 750);
    assert_eq!(xp_for_level(10), 2750);
}

#[test]
fn add_xp_levels_up_and_grants_sp() {
    let mut m = Meta::default();
    assert_eq!((m.level, m.xp, m.sp), (1, 0, 0));

    // Exactly enough for level 1 → 2.
    m.add_xp(500);
    assert_eq!((m.level, m.xp, m.sp), (2, 0, 1));

    // Partial: 750 reaches level 3 exactly, the +100 carries as leftover xp.
    m.add_xp(850);
    assert_eq!((m.level, m.xp, m.sp), (3, 100, 2));
}

#[test]
fn add_xp_rolls_over_multiple_levels_at_once() {
    let mut m = Meta::default();
    // 500 (→L2) + 750 (→L3) + 100 leftover = 1350 in one shot.
    m.add_xp(1350);
    assert_eq!((m.level, m.xp, m.sp), (3, 100, 2));
}

#[test]
fn add_xp_caps_at_max_level() {
    let mut m = Meta {
        level: MAX_LEVEL,
        xp: 0,
        sp: 99,
        account_gold: 0,
        ..Default::default()
    };
    m.add_xp(1_000_000);
    assert_eq!(m.level, MAX_LEVEL);
    assert_eq!(m.xp, 0);
    // No further SP past the cap.
    assert_eq!(m.sp, 99);
}

#[test]
fn bank_accumulates_run_gold() {
    let mut m = Meta::default();
    m.bank(500);
    m.bank(300);
    assert_eq!(m.account_gold, 800);
}

#[test]
fn ron_round_trips() {
    let mut m = Meta {
        account_gold: 1234,
        level: 5,
        xp: 100,
        sp: 4,
        ..Default::default()
    };
    m.unlock("STORM", 0); // a banked unlock must survive the round-trip
    let restored = Meta::from_ron(&m.to_ron());
    assert_eq!(m, restored);
    assert!(restored.is_unlocked("STORM"));
}

#[test]
fn unlock_spends_gold_once_and_gates_on_affordability() {
    use crate::meta::WEAPON_UNLOCK_COST;
    let mut m = Meta { account_gold: WEAPON_UNLOCK_COST, ..Default::default() };

    // First purchase succeeds and spends the gold.
    assert!(m.unlock("STORM", WEAPON_UNLOCK_COST));
    assert!(m.is_unlocked("STORM"));
    assert_eq!(m.account_gold, 0);

    // Re-unlocking an owned item is a no-op (no spend, returns false).
    m.account_gold = WEAPON_UNLOCK_COST;
    assert!(!m.unlock("STORM", WEAPON_UNLOCK_COST));
    assert_eq!(m.account_gold, WEAPON_UNLOCK_COST, "owning it already → no double charge");

    // Can't afford a different item → no unlock, no spend.
    assert!(!m.unlock("RAIL", WEAPON_UNLOCK_COST + 1));
    assert!(!m.is_unlocked("RAIL"));
    assert_eq!(m.account_gold, WEAPON_UNLOCK_COST);
}

#[test]
fn old_saves_without_unlocked_field_still_load() {
    // A pre-armory RON save (no `unlocked` field) must deserialize via serde
    // default rather than failing to Default.
    let legacy = "(account_gold:500,level:3,xp:50,sp:2)";
    let m = Meta::from_ron(legacy);
    assert_eq!(m.account_gold, 500);
    assert_eq!(m.level, 3);
    assert!(m.unlocked.is_empty());
}

#[test]
fn from_ron_falls_back_to_default_on_garbage() {
    assert_eq!(Meta::from_ron("not valid ron !!!"), Meta::default());
    assert_eq!(Meta::from_ron(""), Meta::default());
}
