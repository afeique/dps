//! Global, run-scoped data (Bevy `Resource`s). Per-entity data lives in
//! `components`. Mirrors the role of `js/modules/core/game-state.js`.

use bevy::prelude::*;

/// World play-area half-extents (entities bounce / wrap / despawn at edges).
/// Phase 2 will derive this from the window/camera viewport.
#[derive(Resource, Debug, Clone, Copy)]
pub struct PlayBounds {
    pub half: Vec2,
}

impl Default for PlayBounds {
    fn default() -> Self {
        Self {
            half: Vec2::new(640.0, 360.0),
        }
    }
}

/// Run-scoped score / economy. Ported from `core/game-state.js` +
/// `hud/status.js` (points, gold, kill count).
#[derive(Resource, Debug, Default)]
pub struct Score {
    pub points: u64,
    pub gold: u64,
    pub kills: u32,
}

/// Seconds the streak buff stays active after a kill (`STREAK_BUFF_DURATION`,
/// refreshed on each kill). The streak *count* persists until the player takes
/// damage; only the *multiplier* deactivates when this window lapses (spec III.6).
pub const STREAK_BUFF_SECS: f32 = 4.0;

/// Kill-streak buff (`STREAK_TIERS`, spec III.6). A multiplier on player bullet
/// damage that climbs every 10 kills, refreshed for `STREAK_BUFF_SECS` on each
/// kill, and reset to zero the instant the player takes damage.
#[derive(Resource, Debug, Default)]
pub struct KillStreak {
    /// Consecutive kills without taking damage (drives the tier).
    pub kills: u32,
    /// Buff-window countdown; the multiplier only applies while > 0.
    pub timer: f32,
}

impl KillStreak {
    /// Active damage multiplier — the tier value while the buff window is open,
    /// else 1.0 (the count is retained but the buff has lapsed).
    pub fn multiplier(&self) -> f32 {
        if self.timer > 0.0 {
            streak_mult(self.kills)
        } else {
            1.0
        }
    }
    /// Register a kill: bump the count and refresh the buff window.
    pub fn on_kill(&mut self) {
        self.kills += 1;
        self.timer = STREAK_BUFF_SECS;
    }
    /// The player took damage — the streak breaks entirely.
    pub fn break_streak(&mut self) {
        self.kills = 0;
        self.timer = 0.0;
    }
}

/// Shared gameplay PRNG (xorshift32). The JS uses unseeded `Math.random`
/// (spec I.3); we use a seeded resource so runs are reproducible — assert on
/// ranges/invariants, not exact sequences. Used by crit rolls, drop rolls, and
/// aim jitter.
#[derive(Resource)]
pub struct GameRng(u32);

impl Default for GameRng {
    fn default() -> Self {
        Self(0x9E37_79B9)
    }
}

impl GameRng {
    /// Next value in `[0, 1)`.
    pub fn next_f32(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }
}

/// Base crit chance (spec III.6: 8%, cap 60% with upgrades — caps are a later
/// increment).
pub const BASE_CRIT_CHANCE: f32 = 0.08;

/// Roll a crit: returns the damage multiplier — `1.0` on a normal hit, or a
/// uniform `2.0..3.0×` on a crit (spec III.6: base crit damage is a random
/// 2.0×–3.0×).
pub fn roll_crit(rng: &mut GameRng) -> f32 {
    if rng.next_f32() < BASE_CRIT_CHANCE {
        2.0 + rng.next_f32() // 2.0 ..= ~3.0
    } else {
        1.0
    }
}

/// `STREAK_TIERS` damage multiplier for a given kill count (spec III.6): a step
/// up every 10 kills, from 1.25× at 10 kills to the 3.00× cap at 200.
pub fn streak_mult(kills: u32) -> f32 {
    const TIERS: &[(u32, f32)] = &[
        (10, 1.25), (20, 1.40), (30, 1.55), (40, 1.70), (50, 1.85),
        (60, 2.00), (70, 2.12), (80, 2.23), (90, 2.33), (100, 2.42),
        (110, 2.50), (120, 2.58), (130, 2.65), (140, 2.72), (150, 2.78),
        (160, 2.84), (170, 2.89), (180, 2.93), (190, 2.97), (200, 3.00),
    ];
    let mut m = 1.0;
    for &(threshold, mult) in TIERS {
        if kills >= threshold {
            m = mult;
        }
    }
    m
}
