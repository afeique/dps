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
