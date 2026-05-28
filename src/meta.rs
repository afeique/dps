//! Meta-progression (Phase ME) — the **persistent account layer** that survives
//! across runs: account-gold (the unlock wallet), account level + XP, and SP.
//! Ported from `player/progression.js` + `core/storage.js`. Serialized as RON in
//! the OS config dir. This is the spine the gold-unlock armory + SP stats build on.
//!
//! In-run leveling was retired in the JS (6.0.0); the account level (gained from
//! kills across runs) only grants SP. Run-gold is banked into `account_gold` at
//! run end. The unlock sets, cores, and stash join this struct as those land.

use crate::components::loadout::Ability;
use crate::messages::Death;
use crate::resources::Score;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Account level cap (`MAX_LEVEL`, sp-stats.js).
pub const MAX_LEVEL: u32 = 100;
/// XP per kill (combat-manager.js): a boss is worth 120, a regular enemy 12.
pub const XP_PER_BOSS: u64 = 120;
pub const XP_PER_KILL: u64 = 12;

/// Account-gold ARMORY unlock costs (`shop/armory.js`). The base loadout is free;
/// these gate the *extra* weapons / abilities / attunements behind banked gold.
pub const WEAPON_UNLOCK_COST: u64 = 8_000;
pub const ABILITY_UNLOCK_COST: u64 = 12_000;
pub const ATTUNEMENT_UNLOCK_COST: u64 = 7_000;

/// Points a single SP stat can hold; 1 SP = 1 point; per-point value = `max/20`
/// (`SP_STAT_MAX_POINTS`, sp-stats.js).
pub const SP_STAT_MAX_POINTS: u32 = 20;

/// One SP-stat definition (`SP_STATS`, sp-stats.js): stable id, display name, the
/// total bonus value at full allocation (20 points), and a UI suffix. Per-point
/// value is `max / SP_STAT_MAX_POINTS`.
pub struct SpStatDef {
    pub id: &'static str,
    pub name: &'static str,
    pub max: f32,
    pub suffix: &'static str,
}

/// The twelve account SP stats (`SP_STATS`, sp-stats.js). Spent with banked SP to
/// permanently buff the run; mirror the item-affix / upgrade effects.
pub const SP_STATS: [SpStatDef; 12] = [
    SpStatDef { id: "HEALTH", name: "Health", max: 400.0, suffix: "max HP" },
    SpStatDef { id: "TOUGHNESS", name: "Toughness", max: 50.0, suffix: "% damage reduction" },
    SpStatDef { id: "VAMPIRISM", name: "Vampirism", max: 50.0, suffix: "% lifesteal" },
    SpStatDef { id: "THORNS", name: "Thorns", max: 100.0, suffix: "% reflected" },
    SpStatDef { id: "CRIT_CHANCE", name: "Crit Chance", max: 50.0, suffix: "% crit chance" },
    SpStatDef { id: "CRIT_DAMAGE", name: "Crit Damage", max: 200.0, suffix: "% crit damage" },
    SpStatDef { id: "DODGE", name: "Evasion", max: 50.0, suffix: "% dodge" },
    SpStatDef { id: "SPEED", name: "Speed", max: 100.0, suffix: "% thrust & top speed" },
    SpStatDef { id: "CAPACITOR", name: "Capacitor", max: 100.0, suffix: "max energy" },
    SpStatDef { id: "REACTOR", name: "Reactor", max: 100.0, suffix: "% energy regen" },
    SpStatDef { id: "EFFICIENCY", name: "Efficiency", max: 50.0, suffix: "% power cost" },
    SpStatDef { id: "REGENERATION", name: "Regeneration", max: 2.0, suffix: "HP/s regen" },
];

/// Look up an SP-stat definition by id.
pub fn sp_stat_def(id: &str) -> Option<&'static SpStatDef> {
    SP_STATS.iter().find(|s| s.id == id)
}

/// XP needed to advance *from* `level` to the next (`xpForLevel`, sp-stats.js):
/// `500 + (level − 1) × 250`.
pub fn xp_for_level(level: u32) -> u64 {
    500 + (level.saturating_sub(1) as u64) * 250
}

/// The persistent account profile (RON in the config dir). `PartialEq` for the
/// round-trip test.
#[derive(Resource, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    /// The persistent unlock wallet (spent in the ARMORY). Run-gold banks here.
    pub account_gold: u64,
    pub level: u32,
    pub xp: u64,
    /// Unspent skill points (1 per level; spent on the SP stats).
    pub sp: u32,
    /// Stable ids of gold-unlocked items (weapons / abilities / attunements),
    /// beyond the always-available base loadout. A sorted set so the RON save is
    /// deterministic. `#[serde(default)]` so older saves (without the field) load.
    #[serde(default)]
    pub unlocked: BTreeSet<String>,
    /// Allocated SP points per stat id (0..=[`SP_STAT_MAX_POINTS`]). Spent from
    /// [`Meta::sp`]; the run reads effective bonuses via [`Meta::sp_value`].
    #[serde(default)]
    pub sp_alloc: BTreeMap<String, u32>,
    /// The saved 4-slot ability loadout as stable ability ids (`""` = empty
    /// slot). Empty vec = "no saved loadout, use the default". Applied to
    /// `EquippedAbilities` at startup; rewritten by the loadout picker.
    #[serde(default)]
    pub abilities: Vec<String>,
    /// Last-used primary weapon id (`WeaponKind::id`); `None` = default. Saved at
    /// run end, restored at startup (gated by unlock availability).
    #[serde(default)]
    pub weapon: Option<String>,
    /// Last-used attunement element id (`Element::id`); `None` = no attunement.
    #[serde(default)]
    pub attunement: Option<String>,
}

impl Default for Meta {
    fn default() -> Self {
        Self {
            account_gold: 0,
            level: 1,
            xp: 0,
            sp: 0,
            unlocked: BTreeSet::new(),
            sp_alloc: BTreeMap::new(),
            abilities: Vec::new(),
            weapon: None,
            attunement: None,
        }
    }
}

impl Meta {
    /// Add XP, rolling over as many levels as it covers (each grants +1 SP),
    /// capped at [`MAX_LEVEL`] (XP stops accumulating past the cap).
    pub fn add_xp(&mut self, amount: u64) {
        if self.level >= MAX_LEVEL {
            return;
        }
        self.xp += amount;
        while self.level < MAX_LEVEL && self.xp >= xp_for_level(self.level) {
            self.xp -= xp_for_level(self.level);
            self.level += 1;
            self.sp += 1;
        }
        if self.level >= MAX_LEVEL {
            self.xp = 0;
        }
    }

    /// Bank a finished run's gold into the persistent wallet.
    pub fn bank(&mut self, run_gold: u64) {
        self.account_gold = self.account_gold.saturating_add(run_gold);
    }

    /// Whether `id` has been unlocked in the armory.
    pub fn is_unlocked(&self, id: &str) -> bool {
        self.unlocked.contains(id)
    }

    /// Try to unlock `id` for `cost` account-gold. Returns `true` only on a
    /// *new* purchase — the player had the gold and didn't already own it; the
    /// gold is then spent. Already-owned or unaffordable → `false`, no spend.
    pub fn unlock(&mut self, id: &str, cost: u64) -> bool {
        if self.unlocked.contains(id) || self.account_gold < cost {
            return false;
        }
        self.account_gold -= cost;
        self.unlocked.insert(id.to_string());
        true
    }

    /// Points currently allocated to SP stat `id`.
    pub fn sp_points(&self, id: &str) -> u32 {
        self.sp_alloc.get(id).copied().unwrap_or(0)
    }

    /// Spend 1 banked SP on stat `id`. Returns `true` only on success — there was
    /// unspent SP and the stat wasn't already at [`SP_STAT_MAX_POINTS`].
    pub fn allocate_sp(&mut self, id: &str) -> bool {
        if self.sp == 0 || sp_stat_def(id).is_none() || self.sp_points(id) >= SP_STAT_MAX_POINTS {
            return false;
        }
        *self.sp_alloc.entry(id.to_string()).or_insert(0) += 1;
        self.sp -= 1;
        true
    }

    /// The current effective bonus value of SP stat `id` = `points × (max/20)`.
    pub fn sp_value(&self, id: &str) -> f32 {
        sp_stat_def(id).map_or(0.0, |d| {
            self.sp_points(id) as f32 * (d.max / SP_STAT_MAX_POINTS as f32)
        })
    }

    /// Persist the 4-slot ability loadout (empty slot → `""`) for cross-run memory.
    pub fn set_ability_loadout(&mut self, slots: &[Option<Ability>; 4]) {
        self.abilities = slots
            .iter()
            .map(|s| s.map_or(String::new(), |a| a.id().to_string()))
            .collect();
    }

    /// The saved loadout parsed back to abilities, or `None` if nothing's saved
    /// (the vec isn't exactly 4 long). Empty/unknown ids → an empty slot.
    pub fn ability_loadout(&self) -> Option<[Option<Ability>; 4]> {
        if self.abilities.len() != 4 {
            return None;
        }
        let mut out = [None; 4];
        for (i, id) in self.abilities.iter().enumerate() {
            out[i] = if id.is_empty() { None } else { Ability::from_id(id) };
        }
        Some(out)
    }

    /// Serialize to RON (pure — used by the disk save + the round-trip test).
    pub fn to_ron(&self) -> String {
        ron::to_string(self).unwrap_or_default()
    }

    /// Parse from RON, falling back to `Default` on any error (corrupt/missing).
    pub fn from_ron(s: &str) -> Self {
        ron::from_str(s).unwrap_or_default()
    }
}

/// The on-disk save path: `<config dir>/dps/meta.ron`.
fn meta_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("dps").join("meta.ron"))
}

/// Load the account profile from disk (or `Default` if absent/corrupt). Called
/// once at app build to seed the `Meta` resource.
pub fn load_meta() -> Meta {
    meta_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| Meta::from_ron(&s))
        .unwrap_or_default()
}

/// Persist the account profile to disk (best-effort; creates the dir).
pub fn save_meta(meta: &Meta) {
    if let Some(p) = meta_path() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&p, meta.to_ron());
    }
}

/// Award account XP per enemy kill (boss 120 / regular 12) — reads `Death` in the
/// FixedUpdate sim. Player deaths (`kind: None`) award nothing.
pub fn award_xp(
    mut deaths: MessageReader<Death>,
    mut meta: ResMut<Meta>,
    upgrades: Res<crate::systems::shop::Upgrades>,
) {
    use crate::systems::shop::{xp_boost_mult, UpgradeId};
    let mult = xp_boost_mult(upgrades.owned(UpgradeId::XpBoost));
    for d in deaths.read() {
        if d.kind.is_some() {
            let base = if d.boss_tier > 0 { XP_PER_BOSS } else { XP_PER_KILL };
            meta.add_xp((base as f32 * mult).round() as u64);
        }
    }
}

/// At run end (`OnEnter(GameOver)` / `OnEnter(GameComplete)`), bank the run's
/// gold into the account wallet and persist the profile.
pub fn bank_run(mut meta: ResMut<Meta>, score: Res<Score>) {
    meta.bank(score.gold);
    save_meta(&meta);
}
