//! Elemental synergy reactions (Phase E4b) — ported from `_triggerStatusReactions`
//! (`combat/collision-system.js:2623-2685`).
//!
//! - **SHATTER** — a *frozen* enemy hit for ≥ [`SHATTER_THRESHOLD`] cracks: a
//!   [`SHATTER_DAMAGE`] CRYO burst to every neighbor within [`SHATTER_RADIUS`],
//!   re-freezing them, chaining up to [`MAX_SHATTER_DEPTH`] hops. Fires even on a
//!   killing blow.
//! - **OIL FLARE** — an *oiled* enemy struck by PYRO ignites: a burn of
//!   [`flare_damage`] to every neighbor within [`FLARE_RADIUS`].
//!
//! `bullet_hits_enemy` detects the trigger and pushes a [`ReactionSeed`] onto
//! [`PendingReactions`]; `systems::reactions::resolve_reactions` drains the queue
//! and resolves the AoE (+ the shatter chain) after the hit. Inert until CRYO /
//! PYRO sources apply `Frozen` / `Oil` (W attunements, Cryo Burst, the oil verb).

use bevy::prelude::*;

/// Minimum single-hit damage to crack a frozen enemy (`SHATTER_THRESHOLD`).
pub const SHATTER_THRESHOLD: f32 = 6.0;
/// Shatter AoE radius in px (`SHATTER_RADIUS`).
pub const SHATTER_RADIUS: f32 = 110.0;
/// CRYO damage dealt to each neighbor by a shatter (`SHATTER_DAMAGE`).
pub const SHATTER_DAMAGE: f32 = 8.0;
/// Hard chain-depth cap (`MAX_SHATTER_DEPTH`).
pub const MAX_SHATTER_DEPTH: u8 = 2;
/// Re-freeze duration applied to shattered neighbors, seconds (`applyFreeze(…, 800)`).
pub const SHATTER_REFREEZE_SECS: f32 = 0.8;
/// Oil-flare AoE radius in px (`FLARE_RADIUS`).
pub const FLARE_RADIUS: f32 = 100.0;
/// Burn duration applied by an oil flare, seconds (BURN 3000 ms).
pub const FLARE_BURN_SECS: f32 = 3.0;

/// Whether a hit cracks a frozen target (`enemy.freezeUntil > now && dealt >=
/// SHATTER_THRESHOLD && depth < MAX_SHATTER_DEPTH`). The hit's element does not
/// matter — any heavy enough hit shatters a frozen enemy.
pub fn shatter_triggers(frozen: bool, dealt: f32, depth: u8) -> bool {
    frozen && dealt >= SHATTER_THRESHOLD && depth < MAX_SHATTER_DEPTH
}

/// Oil-flare burn amount for a hit that dealt `dealt` (`max(2, dealt × 0.4)`).
pub fn flare_damage(dealt: f32) -> f32 {
    (dealt * 0.4).max(2.0)
}

/// A queued reaction, produced by `bullet_hits_enemy` and resolved by
/// `systems::reactions::resolve_reactions`.
#[derive(Debug, Clone, Copy)]
pub enum ReactionSeed {
    /// A frozen `source` at `center` cracked; `depth` is the chain hop (0 = the
    /// initial crack from a bullet). The `source` is excluded from its own AoE.
    Shatter {
        source: Entity,
        center: Vec2,
        depth: u8,
    },
    /// An oiled enemy at `center` ignited; spread `damage` burn to neighbors
    /// (the source included, matching the JS flare loop).
    Flare { center: Vec2, damage: f32 },
}

/// Reactions awaiting resolution this tick. `bullet_hits_enemy` pushes; the
/// `resolve_reactions` system drains. A resource (not a message) so it needs no
/// per-harness registration and the shatter chain can re-queue into the same
/// drain.
#[derive(Resource, Default)]
pub struct PendingReactions(pub Vec<ReactionSeed>);
