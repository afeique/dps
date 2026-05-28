//! Meta-progression (Phase ME) — the **persistent account layer** that survives
//! across runs: account-gold (the unlock wallet), account level + XP, and SP.
//! Ported from `player/progression.js` + `core/storage.js`. Serialized as RON in
//! the OS config dir. This is the spine the gold-unlock armory + SP stats build on.
//!
//! In-run leveling was retired in the JS (6.0.0); the account level (gained from
//! kills across runs) only grants SP. Run-gold is banked into `account_gold` at
//! run end. The unlock sets, cores, and stash join this struct as those land.

use crate::messages::Death;
use crate::resources::Score;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Account level cap (`MAX_LEVEL`, sp-stats.js).
pub const MAX_LEVEL: u32 = 100;
/// XP per kill (combat-manager.js): a boss is worth 120, a regular enemy 12.
pub const XP_PER_BOSS: u64 = 120;
pub const XP_PER_KILL: u64 = 12;

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
}

impl Default for Meta {
    fn default() -> Self {
        Self {
            account_gold: 0,
            level: 1,
            xp: 0,
            sp: 0,
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
pub fn award_xp(mut deaths: MessageReader<Death>, mut meta: ResMut<Meta>) {
    for d in deaths.read() {
        if d.kind.is_some() {
            meta.add_xp(if d.boss_tier > 0 { XP_PER_BOSS } else { XP_PER_KILL });
        }
    }
}

/// At run end (`OnEnter(GameOver)` / `OnEnter(GameComplete)`), bank the run's
/// gold into the account wallet and persist the profile.
pub fn bank_run(mut meta: ResMut<Meta>, score: Res<Score>) {
    meta.bank(score.gold);
    save_meta(&meta);
}
