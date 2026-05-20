//! Top-level game flow as Bevy `States`.
//!
//! Phase 1 boots straight into `Playing` (there is no title screen until the
//! UI phase). `Paused`, `Shop`, `GameOver`, and a real `Title` come online in
//! later phases — see `docs/port-plan.md` §1.

use bevy::prelude::*;

#[derive(States, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum GameState {
    /// Active gameplay. Default for now so `cargo run` drops into the slice.
    #[default]
    Playing,
    /// Reserved — pause overlay (Phase 5).
    Paused,
    /// Reserved — death/results screen (Phase 5).
    GameOver,
}
