//! Hitstop / impact freeze (spec I.1). When `Hitstop` is active the FixedUpdate
//! **simulation** is gated off for a few frames while presentation (explosions,
//! camera shake/flash, damage numbers — all in `Update`) keeps animating, so a
//! big kill lands with a punchy freeze.
//!
//! Port scope (documented divergence): the JS triggers brief budget-limited
//! hitstop on *all* kills while keeping the player moving during the freeze. Here
//! it's a **full-sim freeze scoped to boss / mini-boss deaths** — the rare,
//! dramatic moments where freezing everything (player included) for ~0.1 s reads
//! as impact rather than input lag. The per-trash-kill freeze + player-moves-
//! during-hitstop refinements (which need feel tuning) are deferred. Coalesced
//! via `max`; the tick-down runs **ungated** so the freeze always ends.

use crate::messages::Death;
use bevy::prelude::*;

/// Remaining freeze time (seconds). While > 0, the sim chain is gated off.
#[derive(Resource, Default)]
pub struct Hitstop {
    pub secs: f32,
}

impl Hitstop {
    /// Is the simulation currently frozen?
    pub fn frozen(&self) -> bool {
        self.secs > 0.0
    }
    /// Add a freeze, coalescing via `max` (a bigger hit during a freeze extends
    /// it; it never stacks/sums).
    pub fn add(&mut self, secs: f32) {
        self.secs = self.secs.max(secs);
    }
}

/// Freeze on a boss kill (~8 frames @60 Hz) — a beat to register the kill.
pub const BOSS_HITSTOP: f32 = 0.13;
/// Freeze on a mini-boss kill (~5 frames) — a lighter punch.
pub const MINI_HITSTOP: f32 = 0.08;

/// Run condition: the sim runs only when NOT frozen. Combined with
/// `in_state(Playing)` on the FixedUpdate sim chain.
pub fn sim_active(hs: Res<Hitstop>) -> bool {
    !hs.frozen()
}

/// Trigger hitstop on a boss / mini-boss `Death`. Runs **inside** the sim chain
/// (so it fires on the kill tick, after `apply_damage`); subsequent ticks are
/// then gated off until `tick_hitstop` drains the freeze.
pub fn trigger_hitstop(mut deaths: MessageReader<Death>, mut hs: ResMut<Hitstop>) {
    for d in deaths.read() {
        if d.boss_tier > 0 {
            hs.add(BOSS_HITSTOP);
        } else if d.mini_boss {
            hs.add(MINI_HITSTOP);
        }
    }
}

/// Drain the freeze each FixedUpdate tick. Registered **ungated** by hitstop (it
/// only runs while `Playing`, but NOT behind `sim_active`) so the freeze always
/// ends — gating it behind `sim_active` would deadlock the freeze forever.
pub fn tick_hitstop(time: Res<Time>, mut hs: ResMut<Hitstop>) {
    if hs.secs > 0.0 {
        hs.secs = (hs.secs - time.delta_secs()).max(0.0);
    }
}
