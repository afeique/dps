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
fn cores_and_stash_survive_ron_round_trip() {
    use crate::systems::items::{Affix, AffixKind, Item, ItemSlot, Rarity};

    let mut m = Meta { cores: 77, ..Default::default() };
    m.stash.push(Item {
        slot: ItemSlot::Hull,
        level: 7,
        rarity: Rarity::Epic,
        affixes: vec![Affix { kind: AffixKind::Hp, value: 12.5 }],
        name: "Test Hull".to_string(),
    });

    let restored = Meta::from_ron(&m.to_ron());
    assert_eq!(m, restored, "cores + stash round-trip exactly");
    assert_eq!(restored.cores, 77);
    assert_eq!(restored.stash.len(), 1);
    assert_eq!(restored.stash[0].rarity, Rarity::Epic);
}

#[test]
fn salvage_converts_stash_items_to_cores() {
    use crate::systems::cores::salvage_value;
    use crate::systems::items::{Affix, AffixKind, Item, ItemSlot, Rarity};

    let item = |rarity: Rarity, level: u32| Item {
        slot: ItemSlot::Hull,
        level,
        rarity,
        affixes: vec![Affix { kind: AffixKind::Hp, value: 1.0 }],
        name: String::new(),
    };

    let mut m = Meta::default();
    let a = item(Rarity::Common, 1);
    let b = item(Rarity::Epic, 10);
    let (va, vb) = (salvage_value(&a), salvage_value(&b));
    m.stash = vec![a, b];

    // Salvage one item: bank its value, drop it from the stash.
    assert_eq!(m.salvage_item(0), Some(va), "gains the item's salvage value");
    assert_eq!(m.cores, va);
    assert_eq!(m.stash.len(), 1, "salvaged item removed");
    assert_eq!(m.salvage_item(5), None, "out-of-range index → no-op");

    // Salvage the rest: bank the total, clear the stash.
    assert_eq!(m.salvage_all(), vb);
    assert!(m.stash.is_empty(), "stash cleared");
    assert_eq!(m.cores, va + vb, "all salvage cores banked");
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
fn sp_catalog_is_well_formed() {
    use crate::meta::{sp_stat_def, SP_STATS};
    use std::collections::HashSet;
    assert_eq!(SP_STATS.len(), 12, "twelve SP stats");
    let mut ids = HashSet::new();
    for s in &SP_STATS {
        assert!(s.max > 0.0, "{} has a positive cap", s.name);
        assert!(ids.insert(s.id), "duplicate SP stat id {}", s.id);
        assert!(sp_stat_def(s.id).is_some(), "{} is looked up by id", s.id);
    }
    assert!(sp_stat_def("NOPE").is_none());
}

#[test]
fn allocate_sp_spends_points_and_caps() {
    use crate::meta::SP_STAT_MAX_POINTS;
    let mut m = Meta { sp: 5, ..Default::default() };

    assert!(m.allocate_sp("HEALTH"));
    assert_eq!(m.sp, 4);
    assert_eq!(m.sp_points("HEALTH"), 1);
    // Health max 400 over 20 points → +20 per point.
    assert!((m.sp_value("HEALTH") - 20.0).abs() < 1e-3);

    // Unknown stat → no spend.
    assert!(!m.allocate_sp("BOGUS"));
    assert_eq!(m.sp, 4);

    // Out of SP → no spend.
    let mut broke = Meta { sp: 0, ..Default::default() };
    assert!(!broke.allocate_sp("HEALTH"));

    // Caps at SP_STAT_MAX_POINTS even with plenty of SP.
    let mut rich = Meta { sp: 100, ..Default::default() };
    for _ in 0..SP_STAT_MAX_POINTS {
        assert!(rich.allocate_sp("DODGE"));
    }
    assert_eq!(rich.sp_points("DODGE"), SP_STAT_MAX_POINTS);
    assert!(!rich.allocate_sp("DODGE"), "a maxed stat takes no more points");
    // Evasion (Dodge) max 50% at 20 points.
    assert!((rich.sp_value("DODGE") - 50.0).abs() < 1e-3);
}

#[test]
fn sp_alloc_survives_ron_round_trip() {
    let mut m = Meta { sp: 3, ..Default::default() };
    m.allocate_sp("CRIT_CHANCE");
    m.allocate_sp("CRIT_CHANCE");
    let restored = Meta::from_ron(&m.to_ron());
    assert_eq!(m, restored);
    assert_eq!(restored.sp_points("CRIT_CHANCE"), 2);
}

#[test]
fn ability_loadout_persists_round_trip() {
    use crate::components::loadout::Ability;

    let mut m = Meta::default();
    assert!(m.ability_loadout().is_none(), "no saved loadout by default");

    let slots = [
        Some(Ability::Blink),
        None,
        Some(Ability::SentryDrone),
        Some(Ability::Bulwark),
    ];
    m.set_ability_loadout(&slots);
    assert_eq!(m.ability_loadout(), Some(slots), "saved loadout reads back");

    // Survives a RON save/load.
    let restored = Meta::from_ron(&m.to_ron());
    assert_eq!(restored.ability_loadout(), Some(slots));
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
