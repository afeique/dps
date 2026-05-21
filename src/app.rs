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
use bevy_hanabi::prelude::HanabiPlugin;
use bevy_prototype_lyon::prelude::ShapePlugin;

use crate::messages::{Collision, Damage, Death, Fire};
use crate::resources::{PlayBounds, Score};
use crate::states::GameState;
use crate::{audio, render, systems};

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app
            // ── third-party plugins ─────────────────────────────────────
            // lyon vector-path tessellation → Mesh2d for the silhouettes;
            // hanabi GPU-compute particles for explosions.
            .add_plugins((ShapePlugin, HanabiPlugin))
            // ── global flow + shared data ───────────────────────────────
            .init_state::<GameState>()
            .init_resource::<PlayBounds>()
            .init_resource::<Score>()
            .init_resource::<systems::wave::Wave>()
            .init_resource::<systems::asteroids::AsteroidSpawner>()
            .init_resource::<systems::weapons::CurrentWeapon>()
            .init_resource::<systems::power_weapon::PowerWeaponCooldown>()
            .init_resource::<systems::skills::Skills>()
            .insert_resource(ClearColor(Color::srgb(0.015, 0.01, 0.03)))
            // ── game events (Bevy 0.18: buffered "messages") ────────────
            .add_message::<Collision>()
            .add_message::<Damage>()
            .add_message::<Death>()
            .add_message::<Fire>()
            // ── one-time setup ──────────────────────────────────────────
            .add_systems(
                Startup,
                (
                    render::spawn_camera,
                    render::explosion::setup_explosion_effect,
                    render::bullets::setup_bullet_assets,
                    render::starfield::spawn_starfield,
                    render::nebula::spawn_nebula,
                    audio::setup_sfx,
                ),
            )
            // ── presentation: death FX + parallax starfield ─────────────
            .add_systems(
                Update,
                (
                    render::explosion::spawn_on_death,
                    render::explosion::tick_explosion_timers,
                    render::starfield::parallax_stars,
                    audio::play_shoot,
                    audio::play_explosion,
                    audio::play_player_hit,
                ),
            )
            // ── spawn the slice on entering Playing ─────────────────────
            .add_systems(
                OnEnter(GameState::Playing),
                (systems::spawn::spawn_player, systems::wave::reset),
            )
            // ── input: read devices every frame → Intent ────────────────
            .add_systems(
                Update,
                (
                    (systems::input::gather_input, systems::input::update_aim).chain(),
                    systems::weapons::cycle_weapon,
                    systems::power_weapon::fire_power_weapon,
                    systems::skills::use_skills,
                )
                    .run_if(in_state(GameState::Playing)),
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
                    // Per-kind enemy AI, nested as one chained sub-group so the
                    // outer tuple stays under Bevy's 20-element limit. Runs
                    // before `integrate` so steering applies the same tick.
                    (
                        systems::enemy_ai::drifter_ai,
                        systems::enemy::hunter::ai,
                        systems::enemy::guardian::ai,
                        systems::enemy::wasp::ai,
                        systems::enemy::stalker::ai,
                        systems::enemy::prowler::ai,
                        systems::enemy::weaver::ai,
                        systems::enemy::sentinel::ai,
                        systems::enemy::tangerine::ai,
                        systems::enemy::titan::ai,
                        systems::power_weapon::homing_steer,
                    )
                        .chain(),
                    // Spawners (enemies + asteroids).
                    (
                        systems::wave::spawn_waves,
                        systems::asteroids::spawn_asteroids,
                    )
                        .chain(),
                    // Fire intent → bullets.
                    (
                        systems::enemy::firing::enemy_firing,
                        systems::weapons::player_fire,
                        systems::weapons::spawn_bullets,
                    )
                        .chain(),
                    systems::movement::integrate,
                    systems::movement::confine_player,
                    // Collisions → Damage.
                    (
                        systems::collision::bullet_hits_enemy,
                        systems::collision::enemy_bullet_hits_player,
                        systems::collision::enemy_contact_player,
                        systems::asteroids::asteroid_hits,
                    )
                        .chain(),
                    systems::damage::apply_damage,
                    // Drops — runs after apply_damage so `Death` is available.
                    (
                        systems::drops::spawn_drops,
                        systems::powerups::spawn_powerups,
                        systems::drops::attract_orbs,
                        systems::drops::collect_orbs,
                        systems::powerups::collect_powerups,
                    )
                        .chain(),
                    systems::damage::tick_invulnerability,
                    // Cleanup.
                    (
                        systems::cleanup::tick_lifetimes,
                        systems::cleanup::despawn_offscreen_bullets,
                        systems::cleanup::despawn_offscreen_enemies,
                        systems::asteroids::cull_asteroids,
                    )
                        .chain(),
                )
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            );
    }
}
