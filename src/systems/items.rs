//! Item-affix drop system — FIRST SLICE (spec VI.5 / `world/item-system.js`).
//!
//! On every enemy kill, three independent rolls (one per slot category) may mint
//! an **item**: a slot, an item level (= the current wave), a rarity, and a set
//! of stat **affixes** rolled from a shared pool. Generated items are pushed into
//! the [`LootFeed`] resource; `render::loot_feed` shows them as left-edge cards.
//!
//! Scope of this slice: the **data model + roll-on-kill + loot-feed display**.
//! DEFERRED (a design call that overlaps the shop/`Upgrades` economy): equipping
//! an item and routing its affixes into player stats. So for now items drop and
//! parade past on the loot feed but do not yet change the run — the acquisition
//! half of VI.5, faithful and verifiable, with the stat half to follow.
//!
//! Anchored to the **v6.55 spec** (3-tier common/rare/epic), not the v6.83 JS
//! (which expanded to an 8-tier ladder + per-element resist affixes + gear
//! passives — all out of scope, see the `rainboids-divergence` memory). The
//! rarity multiplier bands come from the `item-system.js` header comment that
//! documents the pre-expansion 3-tier era (common 0.85–1.05 / rare 1.00–1.40 /
//! epic 1.35–1.85); probabilities + affix counts + the affix pool are the
//! spec VI.5 numbers verbatim.

use crate::components::{Health, ItemHpBonus, Ship};
use crate::messages::Death;
use crate::resources::GameRng;
use crate::systems::wave::Wave;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ─── Rarity ────────────────────────────────────────────────────────────────

/// Item rarity — the v6.161 **8-tier ladder** (`item-names.js` RARITY_TIERS).
/// Drives the affix count, the roll multiplier band, the loot-card glow color,
/// and the name adjective.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Rarity {
    Common,
    Rare,
    Exceptional,
    Legendary,
    Epic,
    Godlike,
    Divine,
    Transcendental,
}

impl Rarity {
    /// All eight tiers, ascending.
    pub const ALL: [Rarity; 8] = [
        Rarity::Common,
        Rarity::Rare,
        Rarity::Exceptional,
        Rarity::Legendary,
        Rarity::Epic,
        Rarity::Godlike,
        Rarity::Divine,
        Rarity::Transcendental,
    ];

    /// Roll a rarity from a uniform `[0, 1)` sample, weighted by the tier drop
    /// weights (common 0.50 … transcendental 0.001; `item-names.js`). Cumulative
    /// thresholds; the top tier catches the tail.
    pub fn roll(r: f32) -> Rarity {
        match r {
            x if x < 0.50 => Rarity::Common,
            x if x < 0.76 => Rarity::Rare,
            x if x < 0.89 => Rarity::Exceptional,
            x if x < 0.95 => Rarity::Legendary,
            x if x < 0.98 => Rarity::Epic,
            x if x < 0.992 => Rarity::Godlike,
            x if x < 0.997 => Rarity::Divine,
            _ => Rarity::Transcendental,
        }
    }

    /// 1-based rank (common = 1 … transcendental = 8); drives Cores salvage value.
    pub fn rank(self) -> u32 {
        match self {
            Rarity::Common => 1,
            Rarity::Rare => 2,
            Rarity::Exceptional => 3,
            Rarity::Legendary => 4,
            Rarity::Epic => 5,
            Rarity::Godlike => 6,
            Rarity::Divine => 7,
            Rarity::Transcendental => 8,
        }
    }

    /// The next tier up (for Cores tier-up crafting); `None` at the cap.
    pub fn next(self) -> Option<Rarity> {
        Rarity::ALL.get(self.rank() as usize).copied()
    }

    /// How many distinct affixes an item of this rarity rolls (`affixCount`).
    pub fn affix_count(self) -> usize {
        match self {
            Rarity::Common => 1,
            Rarity::Rare => 2,
            Rarity::Exceptional | Rarity::Legendary => 3,
            Rarity::Epic | Rarity::Godlike => 4,
            Rarity::Divine | Rarity::Transcendental => 5,
        }
    }

    /// Roll-multiplier band `(min, max)` for affix values (`RARITY_TIERS`).
    pub fn mult_range(self) -> (f32, f32) {
        match self {
            Rarity::Common => (0.85, 1.05),
            Rarity::Rare => (1.00, 1.40),
            Rarity::Exceptional => (1.30, 1.70),
            Rarity::Legendary => (1.50, 1.90),
            Rarity::Epic => (1.80, 2.20),
            Rarity::Godlike => (2.10, 2.60),
            Rarity::Divine => (2.50, 3.00),
            Rarity::Transcendental => (3.00, 3.60),
        }
    }

    /// Glow color for the loot card (`RARITY_TIERS.color`).
    pub fn color(self) -> Color {
        match self {
            Rarity::Common => Color::srgb_u8(0xb8, 0xc0, 0xcc),
            Rarity::Rare => Color::srgb_u8(0x5c, 0xc6, 0xff),
            Rarity::Exceptional => Color::srgb_u8(0x36, 0xe6, 0xa0),
            Rarity::Legendary => Color::srgb_u8(0xff, 0xb4, 0x3a),
            Rarity::Epic => Color::srgb_u8(0xc0, 0x60, 0xff),
            Rarity::Godlike => Color::srgb_u8(0xff, 0x3d, 0x6e),
            Rarity::Divine => Color::srgb_u8(0xff, 0xf0, 0xa0),
            Rarity::Transcendental => Color::srgb_u8(0xff, 0x66, 0xff),
        }
    }

    /// The rarity adjective prefixed to the item name (common has none).
    pub fn adjective(self) -> &'static str {
        match self {
            Rarity::Common => "",
            Rarity::Rare => "Refined ",
            Rarity::Exceptional => "Calibrated ",
            Rarity::Legendary => "Vanguard ",
            Rarity::Epic => "Prototype ",
            Rarity::Godlike => "Ascendant ",
            Rarity::Divine => "Empyrean ",
            Rarity::Transcendental => "Transcendent ",
        }
    }
}

// ─── Affixes ─────────────────────────────────────────────────────────────────

/// A rollable stat affix (the spec VI.5 pool, which mirrors the passive stat set
/// — the v6.83 per-element resist affixes are out of scope).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum AffixKind {
    Hp,
    Toughness,
    Vampirism,
    Thorns,
    CritChance,
    CritDamage,
    Dodge,
    Speed,
    Regen,
    // Per-element resist affixes (E7) — reduce incoming damage of that element;
    // summed into the player's `Resistances` by `apply_item_resist`.
    ResistPyro,
    ResistCryo,
    ResistVolt,
    ResistToxic,
    ResistVoid,
    ResistRadiant,
}

impl AffixKind {
    /// The element a resist affix protects against (`None` for non-resist kinds).
    pub fn resist_element(self) -> Option<crate::combat::element::Element> {
        use crate::combat::element::Element;
        Some(match self {
            AffixKind::ResistPyro => Element::Pyro,
            AffixKind::ResistCryo => Element::Cryo,
            AffixKind::ResistVolt => Element::Volt,
            AffixKind::ResistToxic => Element::Toxic,
            AffixKind::ResistVoid => Element::Void,
            AffixKind::ResistRadiant => Element::Radiant,
            _ => return None,
        })
    }

    /// Every per-element resist affix kind (drives `apply_item_resist`).
    pub const RESISTS: [AffixKind; 6] = [
        AffixKind::ResistPyro,
        AffixKind::ResistCryo,
        AffixKind::ResistVolt,
        AffixKind::ResistToxic,
        AffixKind::ResistVoid,
        AffixKind::ResistRadiant,
    ];
}

/// Static roll parameters for one affix kind (spec VI.5: `value = (base +
/// (wave−1)*perWave) × rarityMult`, clamped to `min`, with `pct` / regen
/// rounding). `prefix` selects the name-adjective family (hp / toughness / regen).
struct AffixDef {
    kind: AffixKind,
    base: f32,
    per_wave: f32,
    /// Percentage stat → round to 1 dp (vs flat HP int / regen 2 dp).
    pct: bool,
    min: f32,
    prefix: PrefixGroup,
}

#[derive(Clone, Copy)]
enum PrefixGroup {
    Hp,
    Toughness,
    Regen,
}

/// The spec VI.5 affix pool (base / perWave / pct / min), verbatim, plus the six
/// per-element resist affixes (E7).
const AFFIX_POOL: [AffixDef; 15] = [
    AffixDef { kind: AffixKind::Hp,         base: 8.0, per_wave: 2.0,  pct: false, min: 1.0, prefix: PrefixGroup::Hp },
    AffixDef { kind: AffixKind::Toughness,  base: 3.0, per_wave: 0.3,  pct: true,  min: 1.0, prefix: PrefixGroup::Toughness },
    AffixDef { kind: AffixKind::Vampirism,  base: 2.0, per_wave: 0.1,  pct: true,  min: 1.0, prefix: PrefixGroup::Regen },
    AffixDef { kind: AffixKind::Thorns,     base: 6.0, per_wave: 0.4,  pct: true,  min: 2.0, prefix: PrefixGroup::Toughness },
    AffixDef { kind: AffixKind::CritChance, base: 3.0, per_wave: 0.2,  pct: true,  min: 1.0, prefix: PrefixGroup::Hp },
    AffixDef { kind: AffixKind::CritDamage, base: 8.0, per_wave: 0.5,  pct: true,  min: 3.0, prefix: PrefixGroup::Hp },
    AffixDef { kind: AffixKind::Dodge,      base: 2.0, per_wave: 0.1,  pct: true,  min: 1.0, prefix: PrefixGroup::Toughness },
    AffixDef { kind: AffixKind::Speed,      base: 6.0, per_wave: 0.3,  pct: true,  min: 2.0, prefix: PrefixGroup::Regen },
    AffixDef { kind: AffixKind::Regen,      base: 0.3, per_wave: 0.05, pct: false, min: 0.1, prefix: PrefixGroup::Regen },
    AffixDef { kind: AffixKind::ResistPyro,    base: 4.0, per_wave: 0.2, pct: true, min: 2.0, prefix: PrefixGroup::Toughness },
    AffixDef { kind: AffixKind::ResistCryo,    base: 4.0, per_wave: 0.2, pct: true, min: 2.0, prefix: PrefixGroup::Toughness },
    AffixDef { kind: AffixKind::ResistVolt,    base: 4.0, per_wave: 0.2, pct: true, min: 2.0, prefix: PrefixGroup::Toughness },
    AffixDef { kind: AffixKind::ResistToxic,   base: 4.0, per_wave: 0.2, pct: true, min: 2.0, prefix: PrefixGroup::Toughness },
    AffixDef { kind: AffixKind::ResistVoid,    base: 4.0, per_wave: 0.2, pct: true, min: 2.0, prefix: PrefixGroup::Toughness },
    AffixDef { kind: AffixKind::ResistRadiant, base: 4.0, per_wave: 0.2, pct: true, min: 2.0, prefix: PrefixGroup::Toughness },
];

impl AffixKind {
    /// Cross-affix comparison weight (`AFFIX_SCORE_WEIGHT`), normalizing each
    /// stat to an "effective HP" scale so `score_item` can rank items across
    /// slots + rarities for the auto-equip-if-better check.
    pub fn score_weight(self) -> f32 {
        match self {
            AffixKind::Hp => 1.0,
            AffixKind::Toughness => 8.0,
            AffixKind::Vampirism => 8.0,
            AffixKind::Thorns => 4.0,
            AffixKind::CritChance => 6.0,
            AffixKind::CritDamage => 3.0,
            AffixKind::Dodge => 8.0,
            AffixKind::Speed => 3.0,
            AffixKind::Regen => 16.0,
            // Per-element resist — defensive, weighted like Toughness.
            AffixKind::ResistPyro
            | AffixKind::ResistCryo
            | AffixKind::ResistVolt
            | AffixKind::ResistToxic
            | AffixKind::ResistVoid
            | AffixKind::ResistRadiant => 7.0,
        }
    }

    /// Human-readable label for a rolled value (mirrors the JS `label(v)`).
    pub fn label(self, value: f32) -> String {
        let n = trim_num(value);
        match self {
            AffixKind::Hp => format!("+{n} MAX HP"),
            AffixKind::Toughness => format!("+{n}% DEF"),
            AffixKind::Vampirism => format!("+{n}% LIFESTEAL"),
            AffixKind::Thorns => format!("+{n}% THORNS"),
            AffixKind::CritChance => format!("+{n}% CRIT"),
            AffixKind::CritDamage => format!("+{n}% CRIT DMG"),
            AffixKind::Dodge => format!("+{n}% DODGE"),
            AffixKind::Speed => format!("+{n}% SPEED"),
            AffixKind::Regen => format!("+{n}/s REGEN"),
            AffixKind::ResistPyro => format!("+{n}% PYRO RES"),
            AffixKind::ResistCryo => format!("+{n}% CRYO RES"),
            AffixKind::ResistVolt => format!("+{n}% VOLT RES"),
            AffixKind::ResistToxic => format!("+{n}% TOXIC RES"),
            AffixKind::ResistVoid => format!("+{n}% VOID RES"),
            AffixKind::ResistRadiant => format!("+{n}% RADIANT RES"),
        }
    }
}

/// One rolled affix on a finished item.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Affix {
    pub kind: AffixKind,
    pub value: f32,
}

impl Affix {
    pub fn label(self) -> String {
        self.kind.label(self.value)
    }
}

/// Affix value formula (spec VI.5): `(base + (L−1)*perWave) × mult`, rounded by
/// type (HP → integer, regen → 2 dp, other pct → 1 dp) and clamped to `min`.
fn affix_value(def: &AffixDef, level: u32, mult: f32) -> f32 {
    let l = level.max(1) as f32;
    let raw = (def.base + (l - 1.0) * def.per_wave) * mult;
    let rounded = if def.pct {
        (raw * 10.0).round() / 10.0
    } else if matches!(def.kind, AffixKind::Regen) {
        (raw * 100.0).round() / 100.0
    } else {
        raw.round()
    };
    rounded.max(def.min)
}

// ─── Slots ─────────────────────────────────────────────────────────────────

/// A concrete equipment slot (`item-names.js` keys). Five slots grouped into the
/// three drop categories the spec rolls over.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ItemSlot {
    Cockpit,
    Hull,
    Shielding,
    Chassis,
    Nanites,
}

impl ItemSlot {
    /// All five slots, in inventory order (`SLOT_ORDER`).
    pub const ALL: [ItemSlot; 5] = [
        ItemSlot::Cockpit,
        ItemSlot::Hull,
        ItemSlot::Shielding,
        ItemSlot::Chassis,
        ItemSlot::Nanites,
    ];

    /// Short uppercase slot label (`SLOT_LABEL`) for the gear panel.
    pub fn label(self) -> &'static str {
        match self {
            ItemSlot::Cockpit => "COCKPIT",
            ItemSlot::Hull => "HULL",
            ItemSlot::Shielding => "SHIELDING",
            ItemSlot::Chassis => "CHASSIS",
            ItemSlot::Nanites => "NANITES",
        }
    }

    /// Index into the `Equipment` slot array.
    pub fn idx(self) -> usize {
        match self {
            ItemSlot::Cockpit => 0,
            ItemSlot::Hull => 1,
            ItemSlot::Shielding => 2,
            ItemSlot::Chassis => 3,
            ItemSlot::Nanites => 4,
        }
    }

    /// Pickup accent color (`SLOT_ACCENT`): HP slots cyan, Toughness amber,
    /// regen green.
    pub fn accent(self) -> Color {
        match self {
            ItemSlot::Cockpit | ItemSlot::Hull => Color::srgb_u8(0x33, 0xdd, 0xff),
            ItemSlot::Shielding | ItemSlot::Chassis => Color::srgb_u8(0xff, 0xae, 0x3a),
            ItemSlot::Nanites => Color::srgb_u8(0x66, 0xff, 0xaa),
        }
    }

    /// Name bases (`ITEM_BASES`) — the noun half of the item name.
    fn bases(self) -> &'static [&'static str] {
        match self {
            ItemSlot::Cockpit => &["Cockpit", "Bridge", "Prow", "Canopy", "Visor", "Nose-Cone"],
            ItemSlot::Hull => &["Hull", "Carapace", "Shell", "Plating", "Mantle", "Skin"],
            ItemSlot::Shielding => &["Shielding", "Barrier", "Field", "Buffer", "Aegis", "Ward"],
            ItemSlot::Chassis => &["Chassis", "Frame", "Brace", "Lattice", "Truss", "Strut"],
            ItemSlot::Nanites => &["Nanites", "Reactor", "Core", "Matrix", "Module", "Capacitor"],
        }
    }
}

/// The three slot categories the spec rolls over, with per-category drop rates.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotCategory {
    Hp,
    Toughness,
    Trinket,
}

impl SlotCategory {
    const ALL: [SlotCategory; 3] = [SlotCategory::Hp, SlotCategory::Toughness, SlotCategory::Trinket];

    /// Per-kill drop chance for this category (spec VI.5: HP 0.025/0.085,
    /// Toughness 0.020/0.075, Trinket 0.015/0.060 — the higher rate vs a boss).
    pub fn drop_rate(self, boss: bool) -> f32 {
        match (self, boss) {
            (SlotCategory::Hp, false) => 0.025,
            (SlotCategory::Hp, true) => 0.085,
            (SlotCategory::Toughness, false) => 0.020,
            (SlotCategory::Toughness, true) => 0.075,
            (SlotCategory::Trinket, false) => 0.015,
            (SlotCategory::Trinket, true) => 0.060,
        }
    }

    /// The concrete slots in this category (HP = cockpit/hull, Toughness =
    /// shielding/chassis, Trinket = nanites).
    fn slots(self) -> &'static [ItemSlot] {
        match self {
            SlotCategory::Hp => &[ItemSlot::Cockpit, ItemSlot::Hull],
            SlotCategory::Toughness => &[ItemSlot::Shielding, ItemSlot::Chassis],
            SlotCategory::Trinket => &[ItemSlot::Nanites],
        }
    }
}

// ─── Item ──────────────────────────────────────────────────────────────────

/// A generated item (acquisition-only in this slice — not yet equippable).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub slot: ItemSlot,
    pub level: u32,
    pub rarity: Rarity,
    pub affixes: Vec<Affix>,
    /// Pre-built display name, `[Adjective] Prefix Base`.
    pub name: String,
}

impl Item {
    /// The affix lines joined for a compact loot-card subtitle.
    pub fn affix_summary(&self) -> String {
        self.affixes
            .iter()
            .map(|a| a.label())
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

/// Unified item score for cross-slot / cross-rarity comparison (JS `scoreItem`):
/// each affix's value weighted to an "effective HP" scale (`AFFIX_SCORE_WEIGHT`).
pub fn score_item(item: &Item) -> f32 {
    item.affixes
        .iter()
        .map(|a| a.value * a.kind.score_weight())
        .sum()
}

/// Strict-dominant upgrade check (JS `isUpgrade`): a candidate replaces the
/// current slot item only if it strictly out-scores it; an empty slot always
/// takes the candidate.
pub fn is_upgrade(current: Option<&Item>, candidate: &Item) -> bool {
    match current {
        None => true,
        Some(cur) => score_item(candidate) > score_item(cur),
    }
}

// ─── Equipment ───────────────────────────────────────────────────────────────

/// The player's equipped gear — one slot per `ItemSlot`. A dropped item
/// **auto-equips** into its slot if it out-scores the current occupant (JS
/// auto-equip-if-better). Combat reads `affix_total(kind)` to fold equipped
/// affixes into the matching stat (additive with the shop `Upgrades`). Run-scoped.
#[derive(Resource, Default)]
pub struct Equipment {
    slots: [Option<Item>; 5],
}

impl Equipment {
    /// The item currently in `slot`, if any.
    pub fn get(&self, slot: ItemSlot) -> Option<&Item> {
        self.slots[slot.idx()].as_ref()
    }

    /// Auto-equip `item` into its slot if it out-scores the current occupant.
    /// Returns whether it was equipped.
    pub fn try_equip(&mut self, item: Item) -> bool {
        let i = item.slot.idx();
        if is_upgrade(self.slots[i].as_ref(), &item) {
            self.slots[i] = Some(item);
            true
        } else {
            false
        }
    }

    /// Sum a given affix's value across all equipped items (its contribution to
    /// the matching player stat). pct affixes are summed as raw percentages
    /// (callers divide by 100); HP/regen are summed in their native units.
    pub fn affix_total(&self, kind: AffixKind) -> f32 {
        self.slots
            .iter()
            .flatten()
            .flat_map(|it| it.affixes.iter())
            .filter(|a| a.kind == kind)
            .map(|a| a.value)
            .sum()
    }

    /// Clear all slots (called at the start of a fresh run).
    pub fn reset(&mut self) {
        self.slots = Default::default();
    }
}

// ─── Roll helpers ────────────────────────────────────────────────────────────

/// Pick a uniformly-random element of `arr` using the shared RNG.
fn pick<'a, T>(rng: &mut GameRng, arr: &'a [T]) -> &'a T {
    let i = ((rng.next_f32() * arr.len() as f32) as usize).min(arr.len() - 1);
    &arr[i]
}

/// One rarity-mult roll: `multMin + rand × (multMax − multMin)`.
fn roll_mult(rng: &mut GameRng, rarity: Rarity) -> f32 {
    let (lo, hi) = rarity.mult_range();
    lo + rng.next_f32() * (hi - lo)
}

/// Roll `count` distinct affixes for an item (spec VI.5: shuffle the pool,
/// take `count`, each value wave-scaled × an independent rarity-mult roll).
pub fn roll_affix_set(rng: &mut GameRng, level: u32, rarity: Rarity, count: usize) -> Vec<Affix> {
    // Fisher–Yates over pool indices (mirrors the JS shuffle).
    let mut idx: Vec<usize> = (0..AFFIX_POOL.len()).collect();
    for i in (1..idx.len()).rev() {
        let j = ((rng.next_f32() * (i as f32 + 1.0)) as usize).min(i);
        idx.swap(i, j);
    }
    idx.into_iter()
        .take(count.min(AFFIX_POOL.len()))
        .map(|i| {
            let def = &AFFIX_POOL[i];
            let mult = roll_mult(rng, rarity);
            Affix {
                kind: def.kind,
                value: affix_value(def, level, mult),
            }
        })
        .collect()
}

/// Build the item name `[Adjective] Prefix Base` from the slot + primary affix
/// (the prefix family follows the primary affix, the base follows the slot).
fn build_name(rng: &mut GameRng, slot: ItemSlot, rarity: Rarity, primary: AffixKind) -> String {
    let group = AFFIX_POOL
        .iter()
        .find(|d| d.kind == primary)
        .map(|d| d.prefix)
        .unwrap_or(PrefixGroup::Hp);
    let prefixes: &[&str] = match group {
        PrefixGroup::Hp => &[
            "Ablative", "Composite", "Reinforced", "Ironclad", "Insectoid",
            "Tempered", "Layered", "Bulkhead-grade", "Heavy-Spec", "Plated",
        ],
        PrefixGroup::Toughness => &[
            "Deflector", "Quantum", "Phased", "Polarized", "Crystalline",
            "Refractive", "Mirrored", "Inertial", "Stalwart", "Vigilant",
        ],
        PrefixGroup::Regen => &[
            "Adaptive", "Regenerative", "Mending", "Quickening", "Symbiotic",
            "Self-Sealing", "Phoenix", "Vital", "Restorative", "Organic",
        ],
    };
    let prefix = pick(rng, prefixes);
    let base = pick(rng, slot.bases());
    format!("{}{} {}", rarity.adjective(), prefix, base)
}

/// Generate a fresh item for a slot at a wave-level + rarity (spec VI.5). RNG
/// order mirrors the JS: affixes → name (prefix then base).
pub fn create_item(rng: &mut GameRng, slot: ItemSlot, level: u32, rarity: Rarity) -> Item {
    let affixes = roll_affix_set(rng, level, rarity, rarity.affix_count());
    let primary = affixes.first().map(|a| a.kind).unwrap_or(AffixKind::Hp);
    let name = build_name(rng, slot, rarity, primary);
    Item {
        slot,
        level: level.max(1),
        rarity,
        affixes,
        name,
    }
}

/// Roll the per-kill item drops (spec VI.5): three independent rolls, one per
/// slot category. Each success mints an item for a random slot in that category
/// at the current wave-level + a freshly-rolled rarity. `boss` selects the higher
/// drop rates.
pub fn roll_item_drops(rng: &mut GameRng, level: u32, boss: bool) -> Vec<Item> {
    let mut out = Vec::new();
    for cat in SlotCategory::ALL {
        if rng.next_f32() < cat.drop_rate(boss) {
            let slot = *pick(rng, cat.slots());
            let rarity = Rarity::roll(rng.next_f32());
            out.push(create_item(rng, slot, level, rarity));
        }
    }
    out
}

// ─── Loot-feed resource + roll system ────────────────────────────────────────

/// One pending loot-feed entry: a dropped item + whether it **auto-equipped**
/// (out-scored the slot occupant), so the card can mark upgrades vs sidegrades.
pub struct LootEntry {
    pub item: Item,
    pub equipped: bool,
}

/// Items minted this run that the loot feed has not yet shown. The render layer
/// (`render::loot_feed`) drains this each frame into left-edge cards.
#[derive(Resource, Default)]
pub struct LootFeed {
    pub pending: Vec<LootEntry>,
}

impl LootFeed {
    /// Bound on the backlog so a long sim/render desync can't grow it without
    /// limit — drop the oldest if we somehow pile up faster than the feed drains.
    const MAX_PENDING: usize = 12;

    pub fn push(&mut self, item: Item, equipped: bool) {
        self.pending.push(LootEntry { item, equipped });
        if self.pending.len() > Self::MAX_PENDING {
            let overflow = self.pending.len() - Self::MAX_PENDING;
            self.pending.drain(0..overflow);
        }
    }
}

/// On each enemy `Death`, roll item drops (spec VI.5) and queue them on the loot
/// feed. Player deaths drop nothing. Runs in the FixedUpdate drops group; reads
/// `Death` with its own cursor (independent of `spawn_drops`).
pub fn roll_item_drops_on_death(
    mut deaths: MessageReader<Death>,
    wave: Res<Wave>,
    mut rng: ResMut<GameRng>,
    mut feed: ResMut<LootFeed>,
    mut equipment: ResMut<Equipment>,
    mut meta: ResMut<crate::meta::Meta>,
) {
    let level = wave.number() as u32;
    for death in deaths.read() {
        if death.kind.is_none() {
            continue; // player death — no loot
        }
        let boss = death.boss_tier > 0;
        for item in roll_item_drops(&mut rng, level, boss) {
            // Auto-equip if it out-scores the current slot item (JS model); the
            // feed shows every drop, marking which ones equipped.
            let equipped = equipment.try_equip(item.clone());
            // Drops that weren't an upgrade bank into the persistent stash (the
            // salvage/crafting pool, rainboids meta.stash), oldest pruned at the cap.
            if !equipped {
                meta.stash.push(item.clone());
                if meta.stash.len() > STASH_CAP {
                    meta.stash.remove(0);
                }
            }
            feed.push(item, equipped);
        }
    }
}

/// Soft cap on the persistent stash so the RON save can't grow without bound;
/// the oldest entries are pruned first (the player salvages to manage it).
pub const STASH_CAP: usize = 300;

/// Reconcile the player's `Health.max` against the equipped MAX-HP affix total
/// (spec VI.5). Unlike the read-live affixes, HP is a *capacity* baked into
/// `Health.max`, so this applies only the **delta** since the last reconcile
/// (tracked per-player by `ItemHpBonus`, which resets with each new ship).
/// Gaining HP gear heals by the gained amount; losing it clamps `current`.
pub fn apply_item_hp(
    equipment: Res<Equipment>,
    meta: Res<crate::meta::Meta>,
    upgrades: Res<crate::systems::shop::Upgrades>,
    mut player: Query<(&mut Health, &mut ItemHpBonus), With<Ship>>,
) {
    // Equipped HP affixes + account SP HEALTH (flat max-HP on top of base), minus
    // the Glass Cannon keystone's HP cost; reconciled against ItemHpBonus.
    let target = equipment.affix_total(AffixKind::Hp) + meta.sp_value("HEALTH")
        - crate::systems::shop::glass_cannon_hp(
            upgrades.owned(crate::systems::shop::UpgradeId::GlassCannon),
        );
    let Ok((mut hp, mut bonus)) = player.single_mut() else {
        return; // no player (between runs) — reconcile once it spawns
    };
    let delta = target - bonus.0;
    if delta == 0.0 {
        return;
    }
    hp.max = (hp.max + delta).max(1.0);
    if delta > 0.0 {
        hp.current += delta; // equipping HP gear heals by the gained amount
    } else {
        hp.current = hp.current.min(hp.max);
    }
    bonus.0 = target;
}

/// Reconcile the player's per-element `Resistances` against equipped resist
/// affixes (E7) each tick — `resist = Σ ResistX affixes / 100`. Overwrites
/// (the only resist source), so unequipping clears it.
pub fn apply_item_resist(
    equipment: Res<Equipment>,
    upgrades: Res<crate::systems::shop::Upgrades>,
    mut q: Query<&mut crate::combat::element::Resistances, With<Ship>>,
) {
    let Ok(mut res) = q.single_mut() else {
        return;
    };
    // Warding passive adds flat resist to EVERY element, on top of per-element
    // resist affixes.
    let warding = crate::systems::shop::warding_bonus(
        upgrades.owned(crate::systems::shop::UpgradeId::Warding),
    );
    for kind in AffixKind::RESISTS {
        if let Some(e) = kind.resist_element() {
            res.set(e, equipment.affix_total(kind) / 100.0 + warding);
        }
    }
}

// ─── Small formatting helper ─────────────────────────────────────────────────

/// Format a number for an affix label: up to 2 decimals, trailing zeros and a
/// trailing dot trimmed (so `3.0 → "3"`, `3.30 → "3.3"`, `0.30 → "0.3"`).
fn trim_num(v: f32) -> String {
    let mut s = format!("{v:.2}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}
