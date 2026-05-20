//! `GamePlugin` — registers state, resources, messages, and the system
//! schedules that make up the Phase 1 core loop.
//!
//! Schedule discipline (kept from day one):
//!   - `Startup`     : one-time setup (camera).
//!   - `OnEnter(Playing)` : spawn the slice (player + enemy).
//!   - `Update`      : input gathering (read devices every frame).
//!   - `FixedUpdate` : the *simulation* — deterministic-ish fixed-dt step,
//!                     ordered with `.chain()`. Simulation systems never
//!                     touch rendering; rendering/UI never mutate sim state.

use bevy::prelude::*;

use crate::messages::{Collision, Damage, Death, Fire};
use crate::resources::{PlayBounds, Score};
use crate::states::GameState;
use crate::{render, systems};

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app
            // ── global flow + shared data ───────────────────────────────
            .init_state::<GameState>()
            .init_resource::<PlayBounds>()
            .init_resource::<Score>()
            .insert_resource(ClearColor(Color::srgb(0.015, 0.01, 0.03)))
            // ── game events (Bevy 0.18: buffered "messages") ────────────
            .add_message::<Collision>()
            .add_message::<Damage>()
            .add_message::<Death>()
            .add_message::<Fire>()
            // ── one-time setup ──────────────────────────────────────────
            .add_systems(Startup, render::spawn_camera)
            // ── spawn the slice on entering Playing ─────────────────────
            .add_systems(
                OnEnter(GameState::Playing),
                (systems::spawn::spawn_player, systems::spawn::spawn_enemy),
            )
            // ── input: read devices every frame → Intent ────────────────
            .add_systems(
                Update,
                systems::input::gather_input.run_if(in_state(GameState::Playing)),
            )
            // ── death → GameOver → restart flow ─────────────────────────
            .add_systems(
                OnEnter(GameState::GameOver),
                systems::flow::game_over_cleanup,
            )
            .add_systems(
                Update,
                systems::flow::restart_input.run_if(in_state(GameState::GameOver)),
            )
            // ── simulation: fixed timestep, ordered ─────────────────────
            .add_systems(
                FixedUpdate,
                (
                    systems::movement::ship_control,
                    systems::movement::integrate,
                    systems::movement::confine_player,
                    systems::enemy_ai::drifter_ai,
                    systems::enemy_fire::enemy_fire,
                    systems::weapons::player_fire,
                    systems::weapons::spawn_bullets,
                    systems::collision::bullet_hits_enemy,
                    systems::collision::enemy_bullet_hits_player,
                    systems::collision::enemy_contact_player,
                    systems::damage::apply_damage,
                    systems::damage::tick_invulnerability,
                    systems::cleanup::tick_lifetimes,
                    systems::cleanup::despawn_offscreen_bullets,
                )
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            );
    }
}
