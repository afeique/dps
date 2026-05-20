//! Data-driven wave spawner — ports the 10-stage × 3-wave structure from
//! `js/modules/wave/wave-data.js` and the sub-wave progression logic from
//! `js/modules/wave/wave-manager.js`.
//!
//! Simplifications vs. the JS source:
//! - 30 JS waves collapsed to 12 representative waves (one normal + one normal
//!   + one boss per stage for stages 1-4; combined arms for stages 5-6 as
//!   single waves). Endless loop restarts from wave 0 with +1 count bump.
//! - Sub-waves are flattened into a single ordered list of `SpawnEntry`s with
//!   a stagger timer (0.5 s), no kill-count gating (JS gates on ≤2 remaining).
//! - No boss-tier stat scaling, no mini-boss promotions, no asteroids, no
//!   mission system — those live in later phases.
//! - Spawn positions use a deterministic `sin`-based spread, no `rand` crate.

use crate::components::EnemyKind;
use crate::resources::PlayBounds;
use crate::systems::enemy;
use bevy::prelude::*;

// ---------------------------------------------------------------------------
// Static wave table
// ---------------------------------------------------------------------------

/// A single entry in a wave: spawn `count` enemies of `kind`.
#[derive(Clone, Copy)]
struct SpawnEntry {
    kind:  EnemyKind,
    count: u32,
}

const fn e(kind: EnemyKind, count: u32) -> SpawnEntry {
    SpawnEntry { kind, count }
}

/// One wave definition: a flat list of spawn entries executed sequentially
/// with `SPAWN_STAGGER` seconds between each *individual* enemy.
struct WaveDef {
    entries: &'static [SpawnEntry],
}

// ---------------------------------------------------------------------------
// 12-wave table (ported from wave-data.js, simplified).
//
// Stage 1 (JS waves 1-3)  : HUNTER + WASP; boss = TITAN
// Stage 2 (JS waves 4-6)  : adds GUARDIAN; boss = TITAN
// Stage 3 (JS waves 7-9)  : adds STALKER;  boss = TITAN
// Stage 4 (JS waves 10-12): adds DRIFTER + TANGERINE; boss = 2× TITAN
// Stage 5 (JS wave 13-14) : adds WEAVER + SENTINEL (merged to 2 waves)
// Stage 6 (JS wave 15-16) : full roster incl. PROWLER (merged to 2 waves)
// ---------------------------------------------------------------------------
static WAVES: &[WaveDef] = &[
    // ── Wave 0 — Stage 1-1: First Contact ──────────────────────────────
    WaveDef { entries: &[
        e(EnemyKind::Hunter, 3),
        e(EnemyKind::Wasp,   2),
        e(EnemyKind::Hunter, 2),
        e(EnemyKind::Wasp,   2),
    ]},

    // ── Wave 1 — Stage 1-2 ─────────────────────────────────────────────
    WaveDef { entries: &[
        e(EnemyKind::Hunter, 3),
        e(EnemyKind::Wasp,   3),
        e(EnemyKind::Hunter, 3),
        e(EnemyKind::Wasp,   3),
    ]},

    // ── Wave 2 — Stage 1-3: Boss ───────────────────────────────────────
    WaveDef { entries: &[
        e(EnemyKind::Hunter, 3),
        e(EnemyKind::Wasp,   2),
        e(EnemyKind::Titan,  1),
        e(EnemyKind::Hunter, 2),
        e(EnemyKind::Wasp,   2),
    ]},

    // ── Wave 3 — Stage 2-1: Iron Sentinel (adds GUARDIAN) ──────────────
    WaveDef { entries: &[
        e(EnemyKind::Guardian, 2),
        e(EnemyKind::Hunter,   3),
        e(EnemyKind::Guardian, 2),
        e(EnemyKind::Wasp,     3),
    ]},

    // ── Wave 4 — Stage 2-2 ─────────────────────────────────────────────
    WaveDef { entries: &[
        e(EnemyKind::Guardian, 3),
        e(EnemyKind::Hunter,   2),
        e(EnemyKind::Wasp,     4),
        e(EnemyKind::Guardian, 2),
        e(EnemyKind::Hunter,   3),
        e(EnemyKind::Wasp,     2),
    ]},

    // ── Wave 5 — Stage 2-3: Boss ───────────────────────────────────────
    WaveDef { entries: &[
        e(EnemyKind::Guardian, 3),
        e(EnemyKind::Wasp,     2),
        e(EnemyKind::Titan,    1),
        e(EnemyKind::Guardian, 3),
        e(EnemyKind::Hunter,   2),
    ]},

    // ── Wave 6 — Stage 3-1: Iron Vanguard (adds STALKER) ───────────────
    WaveDef { entries: &[
        e(EnemyKind::Stalker,  2),
        e(EnemyKind::Hunter,   3),
        e(EnemyKind::Stalker,  2),
        e(EnemyKind::Guardian, 2),
        e(EnemyKind::Wasp,     2),
    ]},

    // ── Wave 7 — Stage 3-2 ─────────────────────────────────────────────
    WaveDef { entries: &[
        e(EnemyKind::Stalker,  3),
        e(EnemyKind::Hunter,   2),
        e(EnemyKind::Guardian, 3),
        e(EnemyKind::Stalker,  2),
        e(EnemyKind::Hunter,   3),
    ]},

    // ── Wave 8 — Stage 3-3: Boss ───────────────────────────────────────
    WaveDef { entries: &[
        e(EnemyKind::Stalker,  2),
        e(EnemyKind::Guardian, 2),
        e(EnemyKind::Titan,    1),
        e(EnemyKind::Stalker,  2),
        e(EnemyKind::Hunter,   2),
    ]},

    // ── Wave 9 — Stage 4-1: Twin Iron (adds DRIFTER + TANGERINE) ───────
    WaveDef { entries: &[
        e(EnemyKind::Drifter,   2),
        e(EnemyKind::Hunter,    2),
        e(EnemyKind::Tangerine, 2),
        e(EnemyKind::Wasp,      2),
        e(EnemyKind::Drifter,   2),
        e(EnemyKind::Tangerine, 2),
        e(EnemyKind::Hunter,    2),
    ]},

    // ── Wave 10 — Stage 5 combined: Triple Threat (WEAVER + SENTINEL) ──
    WaveDef { entries: &[
        e(EnemyKind::Weaver,   2),
        e(EnemyKind::Wasp,     3),
        e(EnemyKind::Sentinel, 2),
        e(EnemyKind::Guardian, 2),
        e(EnemyKind::Weaver,   2),
        e(EnemyKind::Stalker,  2),
        e(EnemyKind::Sentinel, 2),
    ]},

    // ── Wave 11 — Stage 6 combined: Iron Quartet (full roster + PROWLER)
    WaveDef { entries: &[
        e(EnemyKind::Prowler,   2),
        e(EnemyKind::Hunter,    3),
        e(EnemyKind::Sentinel,  2),
        e(EnemyKind::Weaver,    2),
        e(EnemyKind::Stalker,   2),
        e(EnemyKind::Tangerine, 2),
        e(EnemyKind::Drifter,   2),
        e(EnemyKind::Prowler,   2),
        e(EnemyKind::Guardian,  2),
        e(EnemyKind::Wasp,      3),
        e(EnemyKind::Titan,     2),
        e(EnemyKind::Prowler,   2),
        e(EnemyKind::Guardian,  2),
    ]},
];

// ---------------------------------------------------------------------------
// Tuning constants
// ---------------------------------------------------------------------------

/// Seconds between spawning individual enemies within a wave.
const SPAWN_STAGGER: f32 = 0.5;

/// Seconds of pause after a wave's last spawn before the next wave begins.
const BETWEEN_WAVE_PAUSE: f32 = 3.0;

// ---------------------------------------------------------------------------
// Wave resource
// ---------------------------------------------------------------------------

/// Wave state — tracks progression through the static wave table.
///
/// `loop_bonus` increments by 1 each time we've exhausted the full table,
/// adding that many extra enemies to each `SpawnEntry` count so the game
/// grows harder on successive loops.
#[derive(Resource)]
pub struct Wave {
    /// Index into `WAVES` (the current wave definition).
    wave_idx:     usize,
    /// Flat enemy cursor within the expanded spawn list for this wave.
    /// One unit = one enemy (entries are expanded per `count`).
    enemy_cursor: u32,
    /// Total enemies to spawn this wave (expanded from entries + loop_bonus).
    enemy_total:  u32,
    /// Per-enemy stagger countdown (seconds).
    spawn_timer:  f32,
    /// Between-wave pause countdown (seconds). Active only when
    /// `enemy_cursor >= enemy_total`.
    pause_timer:  f32,
    /// Number of full loops through `WAVES` completed. Added to each entry
    /// count so endless play escalates.
    loop_bonus:   u32,
    /// Running counter used for deterministic x-position spread.
    spawn_seq:    u32,
}

impl Default for Wave {
    fn default() -> Self {
        let (total, _) = expanded_total(0, 0);
        Self {
            wave_idx:     0,
            enemy_cursor: 0,
            enemy_total:  total,
            spawn_timer:  0.0,
            pause_timer:  BETWEEN_WAVE_PAUSE,
            loop_bonus:   0,
            spawn_seq:    0,
        }
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Compute the total enemy count for a wave given the loop bonus, and also
/// return the expanded entry list so `spawn_waves` can index into it.
fn expanded_total(wave_idx: usize, loop_bonus: u32) -> (u32, Vec<EnemyKind>) {
    let def = &WAVES[wave_idx % WAVES.len()];
    let mut list = Vec::new();
    for entry in def.entries {
        let n = entry.count + loop_bonus;
        for _ in 0..n {
            list.push(entry.kind);
        }
    }
    (list.len() as u32, list)
}

/// Deterministic spawn x using a `sin`-based hash of the sequence counter.
/// Returns a value in `[-half_x * 0.85, half_x * 0.85]`.
fn spawn_x(seq: u32, half_x: f32) -> f32 {
    // Two superimposed frequencies give a pseudo-random spread without `rand`.
    let a = (seq as f32 * 97.13_f32).sin();
    let b = (seq as f32 * 43.71_f32).cos();
    (a * 0.6 + b * 0.4) * half_x * 0.85
}

// ---------------------------------------------------------------------------
// Public systems
// ---------------------------------------------------------------------------

/// Reset wave state — called on `OnEnter(Playing)` so every new game starts
/// fresh from wave 0 with no loop bonus.
pub fn reset(mut wave: ResMut<Wave>) {
    *wave = Wave::default();
}

/// Per-tick spawn driver. Stagger-spawns enemies from the current wave;
/// pauses between waves; advances to the next wave when the pause elapses.
pub fn spawn_waves(
    time:   Res<Time>,
    bounds: Res<PlayBounds>,
    mut wave: ResMut<Wave>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();

    // ── Phase A: between-wave pause ──────────────────────────────────────
    if wave.enemy_cursor >= wave.enemy_total {
        wave.pause_timer -= dt;
        if wave.pause_timer > 0.0 {
            return;
        }
        // Advance to next wave.
        let next_idx = wave.wave_idx + 1;
        let (new_loop_bonus, new_wave_idx) = if next_idx >= WAVES.len() {
            (wave.loop_bonus + 1, 0)
        } else {
            (wave.loop_bonus, next_idx)
        };
        let (total, _) = expanded_total(new_wave_idx, new_loop_bonus);
        *wave = Wave {
            wave_idx:     new_wave_idx,
            enemy_cursor: 0,
            enemy_total:  total,
            spawn_timer:  0.0,
            pause_timer:  BETWEEN_WAVE_PAUSE,
            loop_bonus:   new_loop_bonus,
            spawn_seq:    wave.spawn_seq, // carry sequence counter across waves
        };
    }

    // ── Phase B: stagger timer ───────────────────────────────────────────
    wave.spawn_timer -= dt;
    if wave.spawn_timer > 0.0 {
        return;
    }
    wave.spawn_timer = SPAWN_STAGGER;

    // ── Phase C: pick the next enemy from the expanded list ──────────────
    let (_, expanded) = expanded_total(wave.wave_idx, wave.loop_bonus);
    let cursor = wave.enemy_cursor as usize;
    if cursor >= expanded.len() {
        // Should not happen, but guard defensively.
        return;
    }
    let kind = expanded[cursor];
    wave.enemy_cursor += 1;
    wave.spawn_seq += 1;

    // ── Phase D: compute spawn position (top edge, varied x) ─────────────
    let x = spawn_x(wave.spawn_seq, bounds.half.x);
    let y = bounds.half.y - 30.0;
    enemy::spawn(&mut commands, kind, Vec2::new(x, y));
}
