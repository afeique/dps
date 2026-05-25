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

use crate::messages::{Collision, Damage, Death, Fire, Knockback, PlayerHurt};
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
            .init_resource::<crate::resources::KillStreak>()
            .init_resource::<crate::resources::GameRng>()
            .init_resource::<crate::resources::EnergyMeter>()
            .init_resource::<crate::resources::DamageClock>()
            .init_resource::<crate::resources::LastStandUsed>()
            .init_resource::<systems::wave::Wave>()
            .init_resource::<systems::weapons::CurrentWeapon>()
            .init_resource::<systems::power_weapon::PowerWeapon>()
            .init_resource::<systems::skills::Skills>()
            .init_resource::<systems::shop::Upgrades>()
            .init_resource::<systems::shop::ShopSel>()
            .init_resource::<systems::drops::HealthDropTimer>()
            .init_resource::<systems::survivor::SurvivorChoice>()
            .init_resource::<systems::missions::Mission>()
            .init_resource::<systems::items::LootFeed>()
            .init_resource::<systems::items::Equipment>()
            .init_resource::<systems::formations::Formations>()
            .init_resource::<crate::combat::reaction::PendingReactions>()
            .init_resource::<systems::hitstop::Hitstop>()
            .init_resource::<render::shake::ScreenShake>()
            .init_resource::<render::flash::ScreenFlash>()
            .insert_resource(ClearColor(Color::srgb(0.015, 0.01, 0.03)))
            // ── game events (Bevy 0.18: buffered "messages") ────────────
            .add_message::<Collision>()
            .add_message::<Damage>()
            .add_message::<Death>()
            .add_message::<Fire>()
            .add_message::<Knockback>()
            .add_message::<PlayerHurt>()
            .add_message::<crate::messages::Crit>()
            // ── one-time setup ──────────────────────────────────────────
            .add_systems(
                Startup,
                (
                    render::spawn_camera,
                    render::explosion::setup_explosion_effect,
                    render::bullets::setup_bullet_assets,
                    render::starfield::spawn_starfield,
                    render::nebula::spawn_nebula,
                    render::hud::setup_hud,
                    render::loot_feed::setup_loot_feed,
                    render::gear_panel::setup_gear_panel,
                    render::minimap::setup_minimap,
                    render::cursor::spawn_crosshair,
                    render::flash::setup_screen_flash.after(render::spawn_camera),
                    systems::asteroids::setup_asteroid_material,
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
                    render::nebula::parallax_nebula,
                    render::hud::update_hud,
                    render::cursor::update_crosshair,
                    render::damage_numbers::spawn_damage_numbers,
                    render::damage_numbers::float_damage_numbers,
                    // Item UI: left-edge loot feed (drain → cards, age out) +
                    // the right-edge equipped-gear panel. Nested as one element
                    // so the outer Update tuple stays within Bevy's 20 limit.
                    (
                        render::loot_feed::drain_loot_feed,
                        render::loot_feed::age_loot_cards,
                        render::gear_panel::update_gear_panel,
                    )
                        .chain(),
                    render::minimap::update_minimap,
                    render::wave_title::tick_wave_title,
                    render::wave_title::tick_pulse_toast,
                    // Enemy-state visuals: burn/stun auras + the boss-rage
                    // telegraph ring pulse. Nested as one element so the outer
                    // Update tuple stays within Bevy's 20-element limit.
                    (
                        render::status_fx::spawn_status_auras,
                        render::status_fx::update_status_auras,
                        render::telegraph_fx::pulse_telegraph_rings,
                    )
                        .chain(),
                    // 3D-tumble + rebuild the asteroid wireframes (spec VI.1).
                    systems::asteroids::tumble_asteroids,
                    // Camera screen shake on deaths / player hits (spec I.2).
                    (
                        render::shake::trigger_screen_shake,
                        render::shake::apply_screen_shake,
                    )
                        .chain(),
                    // Screen flash: white on boss rage (spec I.2 / IV.7), gold on
                    // a Last Stand cheat-death (spec I.2 gold channel).
                    (
                        render::flash::trigger_screen_flash,
                        render::flash::trigger_last_stand_flash,
                        render::flash::apply_screen_flash,
                    )
                        .chain(),
                    audio::play_shoot,
                    audio::play_explosion,
                    audio::play_player_hit,
                ),
            )
            // ── title screen ────────────────────────────────────────────
            .add_systems(OnEnter(GameState::Title), systems::flow::enter_title)
            .add_systems(
                Update,
                systems::flow::title_input.run_if(in_state(GameState::Title)),
            )
            // ── start a fresh run on leaving the title (NOT on shop-close,
            //    which also re-enters Playing) ────────────────────────────
            .add_systems(
                OnExit(GameState::Title),
                (
                    systems::flow::despawn_screen::<systems::flow::TitleScreen>,
                    systems::flow::reset_run,
                    systems::spawn::spawn_player,
                    systems::wave::reset,
                    systems::power_weapon::reset_energy,
                    systems::formations::clear_formations,
                ),
            )
            // ── shop (on-demand; pauses the sim) ────────────────────────
            .add_systems(OnEnter(GameState::Shop), systems::shop::spawn_shop_ui)
            .add_systems(
                OnExit(GameState::Shop),
                systems::flow::despawn_screen::<systems::shop::ShopPanel>,
            )
            .add_systems(
                Update,
                (systems::shop::shop_ui_update, systems::shop::shop_input)
                    .run_if(in_state(GameState::Shop)),
            )
            // ── input: read devices every frame → Intent ────────────────
            .add_systems(
                Update,
                (
                    (systems::input::gather_input, systems::input::update_aim).chain(),
                    systems::weapons::cycle_weapon,
                    systems::power_weapon::cycle_power_weapon,
                    systems::power_weapon::fire_power_weapon,
                    systems::skills::use_skills,
                    systems::skills::emp_pulse,
                    systems::skills::cast_deflectors,
                    systems::shop::open_shop,
                    systems::flow::open_pause,
                )
                    .run_if(in_state(GameState::Playing)),
            )
            // ── pause overlay ───────────────────────────────────────────
            .add_systems(OnEnter(GameState::Paused), systems::flow::enter_paused)
            .add_systems(
                OnExit(GameState::Paused),
                systems::flow::despawn_screen::<systems::flow::PausedScreen>,
            )
            .add_systems(
                Update,
                systems::flow::pause_input.run_if(in_state(GameState::Paused)),
            )
            // ── death → GameOver → title flow ───────────────────────────
            .add_systems(OnEnter(GameState::GameOver), systems::flow::enter_game_over)
            .add_systems(
                OnExit(GameState::GameOver),
                systems::flow::despawn_screen::<systems::flow::GameOverScreen>,
            )
            .add_systems(
                Update,
                systems::flow::game_over_input.run_if(in_state(GameState::GameOver)),
            )
            // ── campaign cleared → GameComplete → title flow ────────────
            .add_systems(
                Update,
                (
                    systems::flow::check_campaign_complete,
                    systems::survivor::check_survivor,
                    systems::missions::update_missions,
                    render::wave_title::show_wave_title,
                    render::wave_title::show_pulse_toast,
                )
                    .run_if(in_state(GameState::Playing)),
            )
            // ── survivor-card pick (wave clear; pauses the sim) ─────────
            .add_systems(OnEnter(GameState::Survivor), systems::survivor::enter_survivor)
            .add_systems(
                OnExit(GameState::Survivor),
                systems::flow::despawn_screen::<systems::survivor::SurvivorScreen>,
            )
            .add_systems(
                Update,
                (
                    systems::survivor::survivor_ui_update,
                    systems::survivor::survivor_input,
                )
                    .run_if(in_state(GameState::Survivor)),
            )
            .add_systems(
                OnEnter(GameState::GameComplete),
                systems::flow::enter_game_complete,
            )
            .add_systems(
                OnExit(GameState::GameComplete),
                systems::flow::despawn_screen::<systems::flow::GameCompleteScreen>,
            )
            .add_systems(
                Update,
                systems::flow::game_complete_input.run_if(in_state(GameState::GameComplete)),
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
                        systems::enemy::boss_rage,
                        // Rage telegraph window → activate_rage when it lapses (IV.7).
                        systems::enemy::tick_rage_telegraph,
                        systems::power_weapon::homing_steer,
                        // Raged-boss bullets curve toward the player (IV.7).
                        systems::enemy::rage_homing_steer,
                    )
                        .chain(),
                    // Spawner phase (one chained sub-group to stay under Bevy's
                    // 20-element outer-tuple limit): waves + the EN timer-driven
                    // spawns/auras.
                    (
                        // Enemies + the wave's asteroid budget.
                        systems::wave::spawn_waves,
                        // Spore Carriers birth Wasp drones on a timer (EN).
                        systems::enemy::mechanics::spore_spawner,
                        // Plaguebearers drip a Toxic acid trail (EN hazard fields).
                        systems::hazard::drop_hazards,
                        // Lumen Drones shield nearby allies (EN support aura).
                        systems::enemy::mechanics::lumen_aura,
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
                    // Generic formations (spec IV.6): override bound members'
                    // movement toward their slot targets, after integrate.
                    systems::formations::update_formations,
                    // Keep deflector orbs orbiting before collisions resolve.
                    systems::skills::orbit_deflectors,
                    // Collisions → Damage.
                    (
                        systems::collision::bullet_hits_enemy,
                        // Elemental reactions (E4b): resolve shatter/oil-flare
                        // seeds from the hit above (writes Damage + statuses).
                        systems::reactions::resolve_reactions,
                        // Orbs + tractor absorb enemy bullets before they reach
                        // the player.
                        systems::skills::deflector_blocks,
                        systems::skills::tractor_absorb,
                        systems::collision::enemy_bullet_hits_player,
                        systems::collision::enemy_contact_player,
                        systems::asteroids::asteroid_hits,
                        systems::power_weapon::update_nova,
                        systems::power_weapon::update_mines,
                        systems::power_weapon::update_beams,
                        systems::status::tick_burning,
                        // Bleed (TOXIC poison) DoT — sibling of burn (E3).
                        systems::status::tick_bleed,
                        // Player burn DoT (E5 — enemy PYRO attacks).
                        systems::player_status::tick_player_burn,
                        // Hazard fields tick element damage on the player (EN).
                        systems::hazard::tick_hazards,
                        // Static Discharge: periodic AoE pulse around the player.
                        systems::passives::tick_static_discharge,
                        // Whirlwind: orbiting damage zone.
                        systems::passives::tick_whirlwind,
                        // Apply all queued shoves last (bullet _KNOCK + Nova/Mine
                        // knockback) so they land the same tick.
                        systems::collision::apply_knockback,
                    )
                        .chain(),
                    systems::damage::apply_damage,
                    // THORNS: reflect landed player damage to the nearest enemy.
                    systems::damage::apply_thorns,
                    // Boss-pair link: a boss death rages surviving bosses (IV.7).
                    systems::enemy::boss_pair_rage,
                    // Ashen Detonator death-flare: a PYRO blast on death (EN).
                    systems::enemy::mechanics::ashen_death_flare,
                    // Combat Medic: a kill after taking a hit heals the player.
                    systems::passives::tick_combat_medic,
                    // Hitstop: a boss/mini-boss `Death` freezes the sim a few
                    // frames (spec I.1) — fires on the kill tick (chain running).
                    systems::hitstop::trigger_hitstop,
                    // Drops — runs after apply_damage so `Death` is available.
                    (
                        systems::drops::spawn_drops,
                        // Item-affix rolls on kill → loot feed (spec VI.5).
                        systems::items::roll_item_drops_on_death,
                        systems::powerups::spawn_powerups,
                        systems::drops::attract_orbs,
                        systems::drops::collect_orbs,
                        systems::powerups::collect_powerups,
                    )
                        .chain(),
                    // Per-tick status/health upkeep, nested as one chained
                    // sub-group so the outer tuple stays under Bevy's 20 limit.
                    (
                        systems::damage::tick_invulnerability,
                        systems::damage::tick_streak,
                        systems::status::tick_stun,
                        // Count down the timer-based elemental statuses (E3).
                        systems::status::tick_status_timers,
                        // Count down the player chill/corrode timers (E5).
                        systems::player_status::tick_player_statuses,
                        // Expire ally-shield buffs (EN — drops when the Lumen dies).
                        systems::enemy::mechanics::tick_ally_shield,
                        // Decay Warden adaptive resistances over time (EN).
                        systems::enemy::mechanics::decay_warden_resist,
                        // Count down the Overdrive primary-buff (W).
                        systems::power_weapon::tick_overdrive,
                        systems::skills::tick_bulwark,
                        systems::skills::tick_repair,
                        systems::skills::tick_tractor,
                        // Passive regen (after 4 s no-damage) then overheal → tanks.
                        systems::damage::passive_regen,
                        // Reconcile MAX HP against equipped HP affixes (spec VI.5).
                        systems::items::apply_item_hp,
                        systems::damage::overheal_to_tanks,
                    )
                        .chain(),
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
                    // Gate the whole sim off while a hitstop freeze is active
                    // (spec I.1) — presentation (Update) keeps animating.
                    .run_if(in_state(GameState::Playing).and(systems::hitstop::sim_active)),
            )
            // Drain the freeze every FixedUpdate tick — UNGATED by `sim_active`
            // (gating it would deadlock the freeze), only by the Playing state.
            .add_systems(
                FixedUpdate,
                systems::hitstop::tick_hitstop.run_if(in_state(GameState::Playing)),
            );
    }
}
