//! Data-driven wave spawner — ports the full 30-wave / 10-stage campaign from
//! `js/modules/wave/wave-data.js` and the live progression logic from
//! `wave-manager.js` (`updateWaveSystem` / `tryAdvanceSubWave`), per port-spec
//! Part V.
//!
//! Faithful behaviors implemented here:
//! - The **exact 30-wave table** (spec V.2), each wave a list of *pulses*, each
//!   pulse a list of `(kind, count)` groups. Pulse 0 spawns immediately at wave
//!   start; later pulses spawn when **≤2 enemies remain OR 12 s** elapsed since
//!   the last pulse (`tryAdvanceSubWave`).
//! - **Kill-gated advance:** a wave completes only when every pulse has spawned
//!   *and* no enemies remain (asteroids never block). A short between-wave
//!   breather then starts the next wave.
//! - **Boss tiers:** designated TITAN groups carry a tier (1–4) → HP/size
//!   overlay via `enemy::spawn_tiered` (spec IV.7). After wave 30 clears the
//!   campaign is complete (no endless loop).
//!
//! Deferred (separate increments, noted in the spec): on-screen edge spawn
//! positioning (V.5), mini-boss promotion, formations, boss rage, the mission
//! system, per-wave asteroid counts (kept as data in `WaveDef.asteroids` but
//! left to the standalone `AsteroidSpawner`), survivor-card/shop flow, and
//! boss speed-scaling.

use crate::components::{Enemy, EnemyKind};
use crate::resources::PlayBounds;
use crate::systems::enemy;
use bevy::prelude::*;

// Bring the variants into scope so the wave table reads compactly.
use crate::components::EnemyKind::*;

// ---------------------------------------------------------------------------
// Wave table data model
// ---------------------------------------------------------------------------

/// A spawn group within a pulse: `count` enemies of `kind`, optionally promoted
/// to a boss of `tier` (0 = normal).
#[derive(Clone, Copy)]
struct Group {
    kind: EnemyKind,
    count: u32,
    tier: u8,
}

const fn g(kind: EnemyKind, count: u32) -> Group {
    Group { kind, count, tier: 0 }
}
const fn boss(kind: EnemyKind, count: u32, tier: u8) -> Group {
    Group { kind, count, tier }
}

/// One in-wave pulse (a batch of groups spawned together).
struct Pulse(&'static [Group]);

/// One wave: an asteroid count (data only for now) + an ordered pulse list.
struct WaveDef {
    #[allow(dead_code)]
    asteroids: u32,
    pulses: &'static [Pulse],
}

// ---------------------------------------------------------------------------
// The exact 30-wave campaign (port spec V.2). Abbreviation key:
//   Hunter, Wasp, Guardian, Stalker, Drifter, Tangerine, Weaver, Sentinel,
//   Prowler, Titan.
// ---------------------------------------------------------------------------
static WAVES: &[WaveDef] = &[
    // ── Stage 1 — HUNTER + WASP ──────────────────────────────────────────
    WaveDef { asteroids: 5, pulses: &[ // W1 (1-1)
        Pulse(&[g(Hunter, 3)]),
        Pulse(&[g(Hunter, 2), g(Wasp, 2)]),
        Pulse(&[g(Hunter, 3), g(Wasp, 2)]),
    ]},
    WaveDef { asteroids: 5, pulses: &[ // W2 (1-2)
        Pulse(&[g(Hunter, 3), g(Wasp, 2)]),
        Pulse(&[g(Wasp, 4)]),
        Pulse(&[g(Hunter, 3), g(Wasp, 3)]),
    ]},
    WaveDef { asteroids: 3, pulses: &[ // W3 (1-3) BOSS T1
        Pulse(&[g(Hunter, 3), g(Wasp, 2)]),
        Pulse(&[boss(Titan, 1, 1), g(Hunter, 2), g(Wasp, 2)]),
    ]},

    // ── Stage 2 — +GUARDIAN ──────────────────────────────────────────────
    WaveDef { asteroids: 5, pulses: &[ // W4 (2-1)
        Pulse(&[g(Guardian, 2)]),
        Pulse(&[g(Guardian, 2), g(Hunter, 3)]),
        Pulse(&[g(Guardian, 2), g(Wasp, 3)]),
    ]},
    WaveDef { asteroids: 5, pulses: &[ // W5 (2-2)
        Pulse(&[g(Guardian, 3), g(Hunter, 2)]),
        Pulse(&[g(Wasp, 4), g(Guardian, 1)]),
        Pulse(&[g(Guardian, 2), g(Hunter, 3), g(Wasp, 2)]),
    ]},
    WaveDef { asteroids: 3, pulses: &[ // W6 (2-3) BOSS T1
        Pulse(&[g(Guardian, 3), g(Wasp, 2)]),
        Pulse(&[boss(Titan, 1, 1), g(Guardian, 3), g(Hunter, 2)]),
    ]},

    // ── Stage 3 — +STALKER ───────────────────────────────────────────────
    WaveDef { asteroids: 5, pulses: &[ // W7 (3-1)
        Pulse(&[g(Stalker, 2)]),
        Pulse(&[g(Stalker, 2), g(Hunter, 3)]),
        Pulse(&[g(Stalker, 2), g(Guardian, 2), g(Wasp, 2)]),
    ]},
    WaveDef { asteroids: 5, pulses: &[ // W8 (3-2)
        Pulse(&[g(Stalker, 3), g(Hunter, 2)]),
        Pulse(&[g(Guardian, 3), g(Stalker, 1)]),
        Pulse(&[g(Stalker, 2), g(Guardian, 2), g(Hunter, 3)]),
    ]},
    WaveDef { asteroids: 3, pulses: &[ // W9 (3-3) BOSS T2
        Pulse(&[g(Stalker, 2), g(Guardian, 2)]),
        Pulse(&[boss(Titan, 1, 2), g(Stalker, 2), g(Hunter, 2)]),
    ]},

    // ── Stage 4 — +DRIFTER +TANGERINE ────────────────────────────────────
    WaveDef { asteroids: 4, pulses: &[ // W10 (4-1)
        Pulse(&[g(Drifter, 2), g(Hunter, 2)]),
        Pulse(&[g(Tangerine, 2), g(Wasp, 2)]),
        Pulse(&[g(Drifter, 2), g(Tangerine, 2), g(Hunter, 2)]),
    ]},
    WaveDef { asteroids: 4, pulses: &[ // W11 (4-2)
        Pulse(&[g(Stalker, 2), g(Drifter, 2)]),
        Pulse(&[g(Tangerine, 2), g(Guardian, 2)]),
        Pulse(&[g(Stalker, 2), g(Drifter, 2), g(Tangerine, 1)]),
    ]},
    WaveDef { asteroids: 3, pulses: &[ // W12 (4-3) BOSS T2 ×2
        Pulse(&[g(Guardian, 3), g(Stalker, 2), g(Wasp, 2)]),
        Pulse(&[boss(Titan, 2, 2), g(Stalker, 2), g(Tangerine, 1)]),
    ]},

    // ── Stage 5 — +WEAVER +SENTINEL ──────────────────────────────────────
    WaveDef { asteroids: 4, pulses: &[ // W13 (5-1)
        Pulse(&[g(Weaver, 2), g(Wasp, 3)]),
        Pulse(&[g(Weaver, 2), g(Hunter, 3)]),
        Pulse(&[g(Weaver, 2), g(Guardian, 2), g(Stalker, 1)]),
    ]},
    WaveDef { asteroids: 4, pulses: &[ // W14 (5-2)
        Pulse(&[g(Sentinel, 2), g(Wasp, 2)]),
        Pulse(&[g(Sentinel, 2), g(Guardian, 2), g(Weaver, 1)]),
        Pulse(&[g(Sentinel, 2), g(Stalker, 2), g(Weaver, 2)]),
    ]},
    WaveDef { asteroids: 2, pulses: &[ // W15 (5-3) BOSS T3 ×3
        Pulse(&[g(Guardian, 3), g(Sentinel, 2), g(Weaver, 1)]),
        Pulse(&[boss(Titan, 3, 3), g(Sentinel, 2), g(Stalker, 1)]),
    ]},

    // ── Stage 6 — +PROWLER (full roster) ─────────────────────────────────
    WaveDef { asteroids: 4, pulses: &[ // W16 (6-1)
        Pulse(&[g(Prowler, 2), g(Hunter, 3)]),
        Pulse(&[g(Prowler, 2), g(Stalker, 2), g(Wasp, 2)]),
        Pulse(&[g(Prowler, 2), g(Guardian, 2), g(Weaver, 2)]),
    ]},
    WaveDef { asteroids: 4, pulses: &[ // W17 (6-2)
        Pulse(&[g(Tangerine, 2), g(Drifter, 2), g(Hunter, 2)]),
        Pulse(&[g(Sentinel, 2), g(Weaver, 2), g(Stalker, 2)]),
        Pulse(&[g(Prowler, 2), g(Guardian, 2), g(Wasp, 3)]),
    ]},
    WaveDef { asteroids: 2, pulses: &[ // W18 (6-3) BOSS T3 ×3
        Pulse(&[g(Prowler, 3), g(Sentinel, 2), g(Wasp, 2)]),
        Pulse(&[boss(Titan, 3, 3), g(Prowler, 2), g(Guardian, 2)]),
    ]},

    // ── Stage 7 — combined arms ──────────────────────────────────────────
    WaveDef { asteroids: 4, pulses: &[ // W19 (7-1)
        Pulse(&[g(Hunter, 4), g(Guardian, 2), g(Wasp, 2)]),
        Pulse(&[g(Stalker, 2), g(Weaver, 2), g(Drifter, 2)]),
        Pulse(&[g(Prowler, 2), g(Sentinel, 2), g(Tangerine, 2)]),
    ]},
    WaveDef { asteroids: 4, pulses: &[ // W20 (7-2)
        Pulse(&[g(Stalker, 3), g(Prowler, 2), g(Wasp, 2)]),
        Pulse(&[g(Sentinel, 3), g(Guardian, 2), g(Hunter, 2)]),
        Pulse(&[g(Weaver, 2), g(Tangerine, 2), g(Drifter, 2)]),
    ]},
    WaveDef { asteroids: 2, pulses: &[ // W21 (7-3) BOSS T4 ×4
        Pulse(&[g(Stalker, 3), g(Guardian, 3), g(Weaver, 1)]),
        Pulse(&[boss(Titan, 4, 4), g(Stalker, 2), g(Sentinel, 2)]),
    ]},

    // ── Stage 8 ──────────────────────────────────────────────────────────
    WaveDef { asteroids: 4, pulses: &[ // W22 (8-1)
        Pulse(&[g(Tangerine, 2), g(Guardian, 2), g(Hunter, 2)]),
        Pulse(&[g(Weaver, 2), g(Drifter, 2), g(Stalker, 2)]),
        Pulse(&[g(Prowler, 2), g(Sentinel, 2), g(Wasp, 3)]),
    ]},
    WaveDef { asteroids: 4, pulses: &[ // W23 (8-2)
        Pulse(&[g(Hunter, 5), g(Stalker, 2)]),
        Pulse(&[g(Sentinel, 3), g(Prowler, 2), g(Weaver, 2)]),
        Pulse(&[g(Guardian, 3), g(Tangerine, 2), g(Drifter, 1)]),
    ]},
    WaveDef { asteroids: 2, pulses: &[ // W24 (8-3) BOSS T4 ×4
        Pulse(&[g(Tangerine, 3), g(Guardian, 3), g(Stalker, 2)]),
        Pulse(&[boss(Titan, 4, 4), g(Tangerine, 2), g(Prowler, 2)]),
    ]},

    // ── Stage 9 — peak density ───────────────────────────────────────────
    WaveDef { asteroids: 4, pulses: &[ // W25 (9-1)
        Pulse(&[g(Stalker, 3), g(Guardian, 3), g(Wasp, 2)]),
        Pulse(&[g(Sentinel, 3), g(Prowler, 2), g(Weaver, 2)]),
        Pulse(&[g(Tangerine, 3), g(Drifter, 2), g(Hunter, 3)]),
    ]},
    WaveDef { asteroids: 4, pulses: &[ // W26 (9-2)
        Pulse(&[g(Prowler, 3), g(Sentinel, 2), g(Tangerine, 2)]),
        Pulse(&[g(Weaver, 3), g(Stalker, 2), g(Guardian, 2)]),
        Pulse(&[g(Hunter, 4), g(Wasp, 3), g(Drifter, 2)]),
    ]},
    WaveDef { asteroids: 2, pulses: &[ // W27 (9-3) BOSS T4 ×5
        Pulse(&[g(Weaver, 3), g(Guardian, 2), g(Sentinel, 2)]),
        Pulse(&[boss(Titan, 5, 4), g(Weaver, 2), g(Stalker, 2)]),
    ]},

    // ── Stage 10 — finale ────────────────────────────────────────────────
    WaveDef { asteroids: 4, pulses: &[ // W28 (10-1)
        Pulse(&[g(Stalker, 3), g(Guardian, 3), g(Wasp, 3)]),
        Pulse(&[g(Tangerine, 3), g(Prowler, 2), g(Hunter, 3)]),
        Pulse(&[g(Sentinel, 3), g(Weaver, 3), g(Drifter, 2)]),
    ]},
    WaveDef { asteroids: 4, pulses: &[ // W29 (10-2) — final TITAN is NORMAL, not a boss
        Pulse(&[g(Hunter, 4), g(Guardian, 3), g(Wasp, 3)]),
        Pulse(&[g(Stalker, 3), g(Weaver, 3), g(Prowler, 2)]),
        Pulse(&[g(Titan, 1), g(Sentinel, 2), g(Tangerine, 2), g(Drifter, 2)]),
    ]},
    WaveDef { asteroids: 2, pulses: &[ // W30 (10-3) FINAL BOSS T4 ×5
        Pulse(&[g(Guardian, 3), g(Sentinel, 2), g(Stalker, 2)]),
        Pulse(&[g(Prowler, 2), g(Weaver, 2), g(Tangerine, 2)]),
        Pulse(&[boss(Titan, 5, 4), g(Guardian, 2), g(Sentinel, 2), g(Stalker, 2), g(Prowler, 1)]),
    ]},
];

// ---------------------------------------------------------------------------
// Tuning constants (port spec V.3)
// ---------------------------------------------------------------------------

/// Spawn the next pulse once living enemies drop to this many or fewer.
const PULSE_ADVANCE_ENEMY_THRESHOLD: usize = 2;
/// …or this many seconds since the last pulse, whichever comes first.
const PULSE_STALE_SECS: f32 = 12.0;
/// Breather between a wave clearing and the next wave's pulse 0.
const BETWEEN_WAVE_SECS: f32 = 2.0;
/// Initial delay before the very first pulse of the run.
const INTRO_SECS: f32 = 1.0;

// ---------------------------------------------------------------------------
// Wave resource
// ---------------------------------------------------------------------------

/// Live campaign progression state. Mirrors `wave-manager.js`'s `_waveState`.
#[derive(Resource)]
pub struct Wave {
    /// Index into `WAVES` (0-based; 0 = wave 1, … 29 = wave 30).
    idx: usize,
    /// How many pulses of the current wave have been spawned.
    spawned_pulses: usize,
    /// Seconds since the last pulse (drives the 12 s stale fallback).
    pulse_timer: f32,
    /// Countdown before the current wave's pulse 0 spawns (intro / breather).
    start_timer: f32,
    /// Has pulse 0 of the current wave been spawned yet?
    started: bool,
    /// Campaign cleared (wave 30 done) — spawning halts.
    pub completed: bool,
    /// Deterministic spread counter for spawn x-positions.
    spawn_seq: u32,
    /// A (non-final) wave just cleared and is waiting on the survivor-card pick
    /// before advancing — `flow`/`survivor` drive the `Survivor` state off this.
    pub awaiting_reward: bool,
}

impl Default for Wave {
    fn default() -> Self {
        Self {
            idx: 0,
            spawned_pulses: 0,
            pulse_timer: 0.0,
            start_timer: INTRO_SECS,
            started: false,
            completed: false,
            spawn_seq: 0,
            awaiting_reward: false,
        }
    }
}

impl Wave {
    /// Current wave number, 1-based (for HUD/logging).
    pub fn number(&self) -> usize {
        self.idx + 1
    }

    /// Advance to the next wave after the survivor-card reward is taken: clear
    /// the gate, bump the index, and start the between-wave breather.
    pub fn advance_after_reward(&mut self) {
        self.awaiting_reward = false;
        self.idx += 1;
        self.spawned_pulses = 0;
        self.started = false;
        self.start_timer = BETWEEN_WAVE_SECS;
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Distance enemies spawn just *outside* the nearest edge (spec V.5 source
/// margin, simplified — short enough to stay inside the offscreen-cull margin so
/// the player-seeking AI flies them in instead of a warp-in animation).
const EDGE_MARGIN: f32 = 120.0;

/// Spawn position just outside one of the four edges (spec V.5 — "source just
/// outside the nearest edge"). The edge cycles with `seq` so a pulse enters from
/// all sides; the along-edge coordinate is a deterministic `sin`/`cos` spread.
pub fn spawn_pos(seq: u32, bounds: &PlayBounds) -> Vec2 {
    let half = bounds.half;
    let a = (seq as f32 * 97.13_f32).sin();
    let b = (seq as f32 * 43.71_f32).cos();
    let t = (a * 0.6 + b * 0.4) * 0.85; // along-edge spread in [-0.85, 0.85]
    match seq % 4 {
        0 => Vec2::new(t * half.x, half.y + EDGE_MARGIN),  // top
        1 => Vec2::new(half.x + EDGE_MARGIN, t * half.y),  // right
        2 => Vec2::new(t * half.x, -half.y - EDGE_MARGIN), // bottom
        _ => Vec2::new(-half.x - EDGE_MARGIN, t * half.y), // left
    }
}

/// Spawn every group in pulse `pulse_idx` of wave `wave_idx`.
fn spawn_pulse(commands: &mut Commands, bounds: &PlayBounds, wave: &mut Wave, wave_idx: usize, pulse_idx: usize) {
    // The wave's asteroid budget spawns with its opening pulse (spec V).
    if pulse_idx == 0 {
        for _ in 0..WAVES[wave_idx].asteroids {
            wave.spawn_seq += 1;
            crate::systems::asteroids::spawn_one_asteroid(commands, bounds, wave.spawn_seq);
        }
    }

    let pulse = &WAVES[wave_idx].pulses[pulse_idx];
    for group in pulse.0 {
        for _ in 0..group.count {
            wave.spawn_seq += 1;
            let pos = spawn_pos(wave.spawn_seq, bounds);
            enemy::spawn_tiered(commands, group.kind, pos, group.tier);
        }
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Reset wave state — `OnEnter(Playing)`, so each new game starts at wave 1.
pub fn reset(mut wave: ResMut<Wave>) {
    *wave = Wave::default();
}

/// Per-tick campaign driver (FixedUpdate). Pulse pacing + kill-gated advance.
pub fn spawn_waves(
    time: Res<Time>,
    bounds: Res<PlayBounds>,
    enemies: Query<(), With<Enemy>>,
    mut wave: ResMut<Wave>,
    mut commands: Commands,
) {
    if wave.completed {
        return;
    }
    let dt = time.delta_secs();
    let enemy_count = enemies.iter().count();

    // ── Wave start (intro / between-wave breather) ───────────────────────
    if !wave.started {
        wave.start_timer -= dt;
        if wave.start_timer > 0.0 {
            return;
        }
        let idx = wave.idx;
        spawn_pulse(&mut commands, &bounds, &mut wave, idx, 0);
        wave.spawned_pulses = 1;
        wave.pulse_timer = 0.0;
        wave.started = true;
        return;
    }

    let total_pulses = WAVES[wave.idx].pulses.len();

    // ── More pulses to spawn? (≤2 enemies OR 12 s) ───────────────────────
    if wave.spawned_pulses < total_pulses {
        wave.pulse_timer += dt;
        if enemy_count <= PULSE_ADVANCE_ENEMY_THRESHOLD || wave.pulse_timer >= PULSE_STALE_SECS {
            let (idx, next) = (wave.idx, wave.spawned_pulses);
            spawn_pulse(&mut commands, &bounds, &mut wave, idx, next);
            wave.spawned_pulses += 1;
            wave.pulse_timer = 0.0;
        }
        return;
    }

    // ── All pulses spawned — wait for the field to clear (kill-gated) ────
    if enemy_count == 0 {
        if wave.idx + 1 >= WAVES.len() {
            wave.completed = true;
            info!("CAMPAIGN COMPLETE — all 30 waves cleared");
            return;
        }
        // Gate the advance on the survivor-card pick; `survivor::check_survivor`
        // sees this flag and opens the pick, which calls `advance_after_reward`.
        if !wave.awaiting_reward {
            wave.awaiting_reward = true;
            info!("WAVE {} CLEAR — survivor card", wave.idx + 1);
        }
    }
}
