//! Top-level game flow as Bevy `States`.
//!
//! Boot lands on `Title`; ENTER starts a run (`Playing`). A run ends in
//! `GameOver` (death) or `GameComplete` (all 30 waves cleared), both of which
//! return to `Title`. `Paused` is reserved but not yet wired. Screens + their
//! transitions live in `systems::flow`.

use bevy::prelude::*;

#[derive(States, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum GameState {
    /// Start screen — boot lands here; ENTER → `Playing`.
    #[default]
    Title,
    /// Active gameplay.
    Playing,
    /// Reserved — pause overlay (not yet wired).
    Paused,
    /// On-demand upgrade shop (pauses the sim); B/Esc → `Playing`.
    Shop,
    /// Armory — spend persistent account-gold on permanent unlocks. Opened with
    /// `A` from the title; `Esc` → `Title`.
    Armory,
    /// Wave-clear survivor-card pick (pauses the sim); 1/2/3 → `Playing`.
    Survivor,
    /// Death / results screen; ENTER → `Title`.
    GameOver,
    /// Campaign cleared (all 30 waves); ENTER → `Title`.
    GameComplete,
}
