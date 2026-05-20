//! Basic wave spawner — the first cut of `js/modules/wave/*`. Spawns one enemy
//! on a timer at the top edge, cycling through the ported roster. The real
//! data-driven wave tables (`wave-data.js` / `wave-manager.js`) are a later
//! Phase-3 increment.

use crate::components::EnemyKind;
use crate::resources::PlayBounds;
use crate::systems::enemy;
use bevy::prelude::*;

/// Spawn cadence + cursor. Reset on entering `Playing`.
#[derive(Resource)]
pub struct Wave {
    timer: f32,
    interval: f32,
    spawned: u32,
}

impl Default for Wave {
    fn default() -> Self {
        Self {
            timer: 0.0,
            interval: 1.2,
            spawned: 0,
        }
    }
}

/// Roster spawned by the basic cadence — the full 10-kind roster.
const ROSTER: [EnemyKind; 10] = [
    EnemyKind::Drifter,
    EnemyKind::Hunter,
    EnemyKind::Guardian,
    EnemyKind::Wasp,
    EnemyKind::Stalker,
    EnemyKind::Prowler,
    EnemyKind::Weaver,
    EnemyKind::Sentinel,
    EnemyKind::Tangerine,
    EnemyKind::Titan,
];

/// Reset the wave state — run on `OnEnter(Playing)` so restarts start fresh.
pub fn reset(mut wave: ResMut<Wave>) {
    *wave = Wave::default();
}

/// Tick the spawn timer; drop the next enemy in at the top edge.
pub fn spawn_waves(
    time: Res<Time>,
    bounds: Res<PlayBounds>,
    mut wave: ResMut<Wave>,
    mut commands: Commands,
) {
    wave.timer -= time.delta_secs();
    if wave.timer > 0.0 {
        return;
    }
    wave.timer = wave.interval;
    let kind = ROSTER[wave.spawned as usize % ROSTER.len()];
    wave.spawned += 1;

    // Pseudo-random x along the top edge (deterministic, no rng dep).
    let x = (wave.spawned as f32 * 97.13).sin() * bounds.half.x * 0.8;
    let y = bounds.half.y - 30.0;
    enemy::spawn(&mut commands, kind, Vec2::new(x, y));
}
