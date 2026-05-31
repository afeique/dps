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

/// World-space mouse cursor, written by `input::update_aim` every frame. In the
/// tower-defense game there is no ship to carry the aim on its `Intent`, so the
/// cursor lives here — read by tower placement, the build ghost, and the
/// crosshair. `active` is false when the cursor is outside the window.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct Aim {
    pub world: Vec2,
    pub active: bool,
}

/// Run-scoped score / economy. Ported from `core/game-state.js` +
/// `hud/status.js` (points, gold, kill count).
#[derive(Resource, Debug, Default)]
pub struct Score {
    pub points: u64,
    pub gold: u64,
    pub kills: u32,
}

/// Seconds since the player last took damage (spec II.2). Reset to 0 on a
/// `PlayerHurt`, ticked up otherwise; passive regen kicks in after 4 s.
#[derive(Resource, Default)]
pub struct DamageClock(pub f32);

/// Whether the once-per-run Last Stand has been spent this run (spec III.5).
#[derive(Resource, Default)]
pub struct LastStandUsed(pub bool);

/// Power-weapon energy (spec III.3, 6.29.0): built by landing hits (+4 each,
/// `ENERGY_PER_HIT`), capped at `ENERGY_MAX`, spent to fire power weapons,
/// reset to 0 each run.
#[derive(Resource)]
pub struct EnergyMeter {
    pub current: f32,
    /// Usable cap — `ENERGY_MAX` + the SP CAPACITOR bonus, set per run by
    /// `power_weapon::reset_energy`. Defaults to the base cap.
    pub max: f32,
}

pub const ENERGY_MAX: f32 = 100.0;
pub const ENERGY_PER_HIT: f32 = 4.0;

impl Default for EnergyMeter {
    fn default() -> Self {
        Self {
            current: 0.0,
            max: ENERGY_MAX,
        }
    }
}

impl EnergyMeter {
    /// Add energy (e.g. on a landed hit), clamped to the live cap.
    pub fn gain(&mut self, amount: f32) {
        self.current = (self.current + amount).min(self.max);
    }
    /// Spend `cost` if affordable; returns whether the spend happened.
    pub fn try_spend(&mut self, cost: f32) -> bool {
        if self.current >= cost {
            self.current -= cost;
            true
        } else {
            false
        }
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

/// Base crit chance (spec III.6: 8%).
pub const BASE_CRIT_CHANCE: f32 = 0.08;

/// Effective crit chance with `CRIT_CHANCE` upgrade stacks: `min(60%, 8% +
/// 7%×stacks)` (spec III.6).
pub fn crit_chance(stacks: u32) -> f32 {
    (BASE_CRIT_CHANCE + 0.07 * stacks as f32).min(0.60)
}

/// Roll a crit at the given `chance`: returns the damage multiplier — `1.0` on a
/// normal hit, or a uniform `2.0 ..= (3.0 + 0.15×dmg_stacks + dmg_bonus)`×
/// (capped at 5.5×) on a crit (spec III.6). `dmg_bonus` is an extra additive
/// term on the upper bound (equipped CRIT-DAMAGE affixes, as a fraction).
pub fn roll_crit(rng: &mut GameRng, chance: f32, dmg_stacks: u32, dmg_bonus: f32) -> f32 {
    if rng.next_f32() < chance {
        let max = (3.0 + 0.15 * dmg_stacks as f32 + dmg_bonus).min(5.5);
        2.0 + rng.next_f32() * (max - 2.0) // uniform [2.0, max]
    } else {
        1.0
    }
}

