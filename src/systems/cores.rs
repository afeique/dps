//! Cores — the item-crafting currency earned by salvaging gear (Phase IT,
//! faithful port of rainboids `world/cores.js`). Pure value helpers (no ECS, no
//! UI) so the salvage economy is unit-testable in isolation; the persistent
//! stash + crafting UI that spend Cores (`Meta::spend_cores`) land in later
//! slices. Cores are banked into [`crate::meta::Meta::cores`] via `add_cores`.

use crate::meta::Meta;
use crate::resources::GameRng;
use crate::systems::items::{roll_affix_set, Item};

/// Cores granted by salvaging one item: `rarity_rank × affix_count ×
/// (1 + level × 0.1)`, rounded and floored at 1 (so even a common L1 is worth
/// something). Mirrors `cores.js::salvageValue` (the v6.161 base, without the
/// later trait/passive flat riders — dps items carry neither yet).
pub fn salvage_value(item: &Item) -> u64 {
    let rank = item.rarity.rank() as f32;
    let affixes = (item.affixes.len().max(1)) as f32;
    let lvl = item.level.max(1) as f32;
    let base = rank * affixes * (1.0 + lvl * 0.1);
    (base.round() as u64).max(1)
}

/// Total Cores from salvaging a batch of items (`cores.js::totalSalvage`).
pub fn total_salvage(items: &[Item]) -> u64 {
    items.iter().map(salvage_value).sum()
}

/// Cores to reroll an item's affixes within its tier — `max(2, rank × 3)`, so a
/// transcendental reroll is a real commitment (`cores.js::rerollCost`).
pub fn reroll_cost(item: &Item) -> u64 {
    (item.rarity.rank() * 3).max(2) as u64
}

/// Cores to tier an item up one rung — `(rank + 1) × 12`, steeper each step.
/// `None` at rank 8 (transcendental — no higher tier). (`cores.js::tierUpCost`).
pub fn tier_up_cost(item: &Item) -> Option<u64> {
    let rank = item.rarity.rank();
    if rank >= 8 {
        None
    } else {
        Some(((rank + 1) * 12) as u64)
    }
}

// ── Crafting: spend Cores to improve a stashed item (R8.6 / R8.8) ───────────

/// Reroll the affixes of `stash[index]` within its tier, spending [`reroll_cost`]
/// Cores. Keeps the item's rarity, level, and affix *count*; re-rolls the values
/// (and which affixes). Returns `true` if the reroll happened (affordable + valid
/// index), `false` otherwise (Cores unspent).
pub fn reroll_stash_item(meta: &mut Meta, rng: &mut GameRng, index: usize) -> bool {
    let Some(item) = meta.stash.get(index) else {
        return false;
    };
    let cost = reroll_cost(item);
    let (level, rarity, count) = (item.level, item.rarity, item.affixes.len().max(1));
    if !meta.spend_cores(cost) {
        return false;
    }
    meta.stash[index].affixes = roll_affix_set(rng, level, rarity, count);
    true
}

/// Tier `stash[index]` up one rarity rung, spending [`tier_up_cost`] Cores and
/// re-rolling its affixes for the new (wider) tier. Returns `false` (Cores
/// unspent) at max tier, an invalid index, or when unaffordable.
pub fn tier_up_stash_item(meta: &mut Meta, rng: &mut GameRng, index: usize) -> bool {
    let Some(item) = meta.stash.get(index) else {
        return false;
    };
    let (Some(cost), Some(next)) = (tier_up_cost(item), item.rarity.next()) else {
        return false; // already at the top of the ladder
    };
    let (level, count) = (item.level, next.affix_count());
    if !meta.spend_cores(cost) {
        return false;
    }
    let it = &mut meta.stash[index];
    it.rarity = next;
    it.affixes = roll_affix_set(rng, level, next, count);
    true
}
