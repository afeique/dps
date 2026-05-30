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
use crate::{audio, components, render, systems};

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app
            // ── third-party plugins ─────────────────────────────────────
            // lyon vector-path tessellation → Mesh2d for the silhouettes;
            // hanabi GPU-compute particles for explosions.
            .add_plugins((ShapePlugin, HanabiPlugin))
            // egui for the BUILD screen (Phase UI).
            .add_plugins(bevy_egui::EguiPlugin::default())
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
            .init_resource::<systems::weapons::Attunements>()
            .init_resource::<systems::weapons::ElementInfusion>()
            .init_resource::<systems::weapons::BloodlustTimer>()
            .init_resource::<systems::power_weapon::PowerWeapon>()
            .init_resource::<systems::skills::Skills>()
            .init_resource::<components::loadout::EquippedAbilities>()
            .init_resource::<components::loadout::AbilityCooldowns>()
            .init_resource::<systems::shop::Upgrades>()
            .init_resource::<systems::shop::ShopSel>()
            .init_resource::<systems::drops::HealthDropTimer>()
            .init_resource::<systems::survivor::SurvivorChoice>()
            .init_resource::<systems::missions::Mission>()
            .init_resource::<systems::items::LootFeed>()
            .init_resource::<systems::items::Equipment>()
            .init_resource::<systems::formations::Formations>()
            .init_resource::<crate::combat::reaction::PendingReactions>()
            // Meta-progression (ME): load the persistent account profile at boot.
            .insert_resource(crate::meta::load_meta())
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
            .add_message::<crate::messages::Pickup>()
            .add_message::<crate::messages::Dodged>()
            .add_message::<crate::messages::Shard>()
            .add_message::<crate::messages::Reaction>()
            .add_message::<crate::messages::AsteroidShatter>()
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
                    // Apply the saved ability loadout from Meta (ME persistence).
                    systems::loadout_screen::apply_saved_loadout,
                    // Restore the saved weapon + attunement (ME persistence).
                    systems::weapons::apply_saved_build,
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
                    render::hud::update_boss_bar,
                    render::hud::update_ability_bar,
                    render::cursor::update_crosshair,
                    // Floating combat text (damage / dodge / heal) — nested as one
                    // element so the outer Update tuple stays under Bevy's 20 limit.
                    (
                        render::damage_numbers::spawn_damage_numbers,
                        render::damage_numbers::spawn_dodge_text,
                        render::damage_numbers::spawn_crit_text,
                        render::damage_numbers::heal_numbers,
                        render::damage_numbers::float_damage_numbers,
                    ),
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
                    // Enemy-state visuals: status auras, the boss-rage telegraph
                    // ring pulse, and elemental-reaction shockwaves. Nested as one
                    // element so the outer Update tuple stays under Bevy's 20 limit.
                    (
                        render::status_fx::spawn_status_auras,
                        render::status_fx::update_status_auras,
                        // Crackling lightning between nearby conducting enemies.
                        render::status_fx::conduct_arcs,
                        render::telegraph_fx::pulse_telegraph_rings,
                        render::reaction_fx::spawn_reaction_fx,
                        // Element-tinted wavefront rings on every enemy death.
                        render::reaction_fx::spawn_death_rings,
                        render::reaction_fx::tick_shockwaves,
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
                        render::flash::trigger_player_hurt_flash,
                        render::flash::low_hp_warning,
                        render::flash::apply_screen_flash,
                    )
                        .chain(),
                    // SFX playback, nested as one element so the outer Update
                    // tuple stays within Bevy's 20-element limit.
                    (
                        audio::play_shoot,
                        audio::play_explosion,
                        audio::play_player_hit,
                        audio::play_reaction,
                        audio::play_crit,
                        audio::play_pickup,
                        // Rock-crunch on every asteroid shatter.
                        audio::play_asteroid_destroy,
                    ),
                ),
            )
            // ── title screen ────────────────────────────────────────────
            .add_systems(OnEnter(GameState::Title), systems::flow::enter_title)
            .add_systems(
                Update,
                systems::flow::title_input.run_if(in_state(GameState::Title)),
            )
            // Despawn the title overlay whenever we leave it (→ Playing or Armory).
            .add_systems(
                OnExit(GameState::Title),
                systems::flow::despawn_screen::<systems::flow::TitleScreen>,
            )
            // Fresh-run setup ONLY on Title→Playing — not on Title→Armory, and
            // not on shop-close (which re-enters Playing without exiting Title).
            .add_systems(
                OnTransition {
                    exited: GameState::Title,
                    entered: GameState::Playing,
                },
                (
                    systems::flow::reset_run,
                    systems::spawn::spawn_player,
                    systems::wave::reset,
                    systems::power_weapon::reset_energy,
                    systems::formations::clear_formations,
                ),
            )
            // ── armory (spend account-gold on unlocks; opened with A on title) ─
            .init_resource::<systems::armory::ArmorySel>()
            .add_systems(
                Update,
                systems::armory::open_armory.run_if(in_state(GameState::Title)),
            )
            .add_systems(OnEnter(GameState::Armory), systems::armory::spawn_armory_ui)
            .add_systems(
                OnExit(GameState::Armory),
                systems::flow::despawn_screen::<systems::armory::ArmoryPanel>,
            )
            .add_systems(
                Update,
                (
                    systems::armory::armory_ui_update,
                    systems::armory::armory_input,
                )
                    .run_if(in_state(GameState::Armory)),
            )
            // ── skill points (allocate account SP; opened with S on title) ─────
            .init_resource::<systems::sp_alloc::SpAllocSel>()
            .add_systems(
                Update,
                systems::sp_alloc::open_sp_alloc.run_if(in_state(GameState::Title)),
            )
            .add_systems(
                OnEnter(GameState::SpAllocation),
                systems::sp_alloc::spawn_sp_ui,
            )
            .add_systems(
                OnExit(GameState::SpAllocation),
                systems::flow::despawn_screen::<systems::sp_alloc::SpPanel>,
            )
            .add_systems(
                Update,
                (
                    systems::sp_alloc::sp_ui_update,
                    systems::sp_alloc::sp_input,
                )
                    .run_if(in_state(GameState::SpAllocation)),
            )
            // ── loadout picker (assign abilities to the 4 slots; L on title) ───
            .init_resource::<systems::loadout_screen::LoadoutSel>()
            .add_systems(
                Update,
                systems::loadout_screen::open_loadout.run_if(in_state(GameState::Title)),
            )
            .add_systems(
                OnEnter(GameState::Loadout),
                systems::loadout_screen::spawn_loadout_ui,
            )
            .add_systems(
                OnExit(GameState::Loadout),
                systems::flow::despawn_screen::<systems::loadout_screen::LoadoutPanel>,
            )
            .add_systems(
                Update,
                (
                    systems::loadout_screen::loadout_ui_update,
                    systems::loadout_screen::loadout_input,
                )
                    .run_if(in_state(GameState::Loadout)),
            )
            // ── egui BUILD screen (tabbed; B on title) ─────────────────────────
            .init_resource::<systems::build_screen::BuildTab>()
            .add_systems(
                Update,
                (
                    systems::build_screen::open_build.run_if(in_state(GameState::Title)),
                    systems::build_screen::build_input.run_if(in_state(GameState::Build)),
                ),
            )
            .add_systems(
                bevy_egui::EguiPrimaryContextPass,
                systems::build_screen::build_screen_ui.run_if(in_state(GameState::Build)),
            )
            // ── player engine thrust trail (graphical parity) ──────────────────
            .add_systems(
                Update,
                (
                    render::engine_trail::emit_engine_trail,
                    render::engine_trail::fade_engine_exhaust,
                    // Muzzle flash at the nose on each player shot (graphical parity).
                    render::muzzle_flash::emit_muzzle_flash,
                    render::muzzle_flash::fade_muzzle_flash,
                    // Localized hit-flash burst on the hull when the player is hit.
                    render::hit_flash::emit_player_hit_flash,
                    render::hit_flash::fade_hit_flash,
                    // Energy charge-glow halo on the hull (tracks the energy meter).
                    render::charge_glow::update_charge_glow,
                    // Dash afterimage: faint hull ghosts while dashing.
                    render::dash_trail::emit_dash_trail,
                    render::dash_trail::fade_dash_ghosts,
                    // Invuln shield bubble: pulses into view during i-frames.
                    render::shield_bubble::update_shield_bubble,
                    // Overdrive aura: pulses while the primary-buff is active.
                    render::overdrive_aura::update_overdrive_aura,
                    // Golden aura when the account levels up mid-run.
                    render::level_up_aura::level_up_aura,
                )
                    .run_if(in_state(GameState::Playing)),
            )
            // ── bullet-impact sparks on every landed hit (graphical parity) ────
            .add_systems(
                Update,
                (
                    render::impact_spark::spawn_impact_sparks,
                    render::impact_spark::fade_impact_sparks,
                    // Asteroid death burst: hue-matched 3D wireframe shards + a
                    // colored ring on every shatter (port of `createDebris` §7b).
                    render::asteroid_debris::spawn_asteroid_debris,
                    // Element-tinted shrapnel shards on every enemy death (same
                    // wireframe-shard primitive, tinted to the kill's element).
                    render::asteroid_debris::spawn_enemy_shrapnel,
                    render::asteroid_debris::tumble_shards,
                    // Line debris: the rock's broken wireframe struts drift apart.
                    render::asteroid_debris::tick_line_debris,
                    // Lingering ember afterglow at the shatter point.
                    render::asteroid_debris::fade_embers,
                    // Idle orbs spin + pulse so they read as live collectibles.
                    systems::drops::animate_orbs,
                    // Enemy spawn-in warp: a cyan ring + a grow-to-full materialize.
                    render::warp_in::flash_warp_in,
                    render::warp_in::tick_warp_in,
                    // Breathe the (now-visible) hazard danger zones.
                    systems::hazard::animate_hazards,
                    // Rising fire embers off burning enemies.
                    render::asteroid_debris::emit_burn_embers,
                    // Glinting ice sparkles on frozen enemies.
                    render::asteroid_debris::emit_frozen_glints,
                    // Corrode acid drips / bleed flecks / void-mark wisps.
                    render::asteroid_debris::emit_status_motes,
                )
                    .chain()
                    .run_if(in_state(GameState::Playing)),
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
                    systems::weapons::cycle_attunement,
                    systems::power_weapon::cycle_power_weapon,
                    systems::power_weapon::fire_power_weapon,
                    systems::skills::use_skills,
                    systems::skills::emp_pulse,
                    systems::skills::cast_deflectors,
                    systems::abilities::activate_loadout,
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
            .add_systems(
                OnEnter(GameState::GameOver),
                (
                    systems::flow::enter_game_over,
                    crate::meta::bank_run,
                    systems::weapons::persist_build,
                ),
            )
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
                (
                    systems::flow::enter_game_complete,
                    crate::meta::bank_run,
                    systems::weapons::persist_build,
                ),
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
                        // Mid-fight frenzy escalation at ≤66% HP (phase 1).
                        systems::enemy::boss_frenzy,
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
                        // Sentry Drones (AB) emit player Fire before spawn_bullets.
                        systems::abilities::tick_sentry_drones,
                        systems::enemy::firing::enemy_firing,
                        systems::weapons::player_fire,
                        systems::weapons::spawn_bullets,
                    )
                        .chain(),
                    // Bullet trajectory systems (W), grouped to keep the outer
                    // tuple under Bevy's 20-element limit — run before integrate.
                    (
                        // Gravity Lance orbs drag nearby enemies in.
                        systems::weapons::gravity_pull,
                        // Boomerang discs curve back to the player.
                        systems::weapons::boomerang_return,
                        // Flak bullets airburst into a shrapnel ring at range.
                        systems::weapons::flak_airburst,
                    )
                        .chain(),
                    systems::movement::integrate,
                    systems::movement::confine_player,
                    // Generic formations (spec IV.6): override bound members'
                    // movement toward their slot targets, after integrate.
                    systems::formations::update_formations,
                    // Pre-collision positioning (nested to keep the outer tuple
                    // under Bevy's 20-element limit): deflector orbs orbit, and
                    // boss weak-point parts park at their boss + offset (BO).
                    (
                        systems::skills::orbit_deflectors,
                        systems::enemy::mechanics::update_boss_parts,
                    )
                        .chain(),
                    // Collisions → Damage.
                    (
                        systems::collision::bullet_hits_enemy,
                        // Mitosis/Flak shards from the hits above (needs BulletAssets).
                        systems::weapons::spawn_shards,
                        // Elemental reactions (E4b): resolve shatter/oil-flare
                        // seeds from the hit above (writes Damage + statuses).
                        systems::reactions::resolve_reactions,
                        // Orbs + tractor absorb enemy bullets before they reach
                        // the player.
                        systems::skills::deflector_blocks,
                        systems::skills::tractor_absorb,
                        systems::collision::enemy_bullet_hits_player,
                        // Hull contacts: enemy ram + asteroid carom — nested into
                        // one slot to stay under Bevy's 20-element tuple ceiling.
                        (
                            systems::collision::enemy_contact_player,
                            systems::collision::player_hits_asteroid,
                        ),
                        systems::asteroids::asteroid_hits,
                        systems::power_weapon::update_nova,
                        systems::power_weapon::update_mines,
                        systems::power_weapon::update_beams,
                        // Singularity pull + collapse (W).
                        systems::power_weapon::update_singularity,
                        // Orbital Strike telegraph → column AoE (W).
                        systems::power_weapon::update_orbital_strike,
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
                    // On-kill `Death` readers, nested as one chained sub-group so
                    // the outer tuple stays under Bevy's 20-element limit.
                    (
                        // Award account XP per kill (ME).
                        crate::meta::award_xp,
                        // Bloodlust: a kill briefly speeds up fire (keystone passive).
                        systems::weapons::bloodlust_on_kill,
                    )
                        .chain(),
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
                        // Boss core invulnerable while its weak-point parts live (BO).
                        systems::enemy::mechanics::update_core_shield,
                        // Scale freshly-spawned enemies' HP by the run difficulty (X).
                        systems::difficulty::apply_difficulty,
                        // Count down the Overdrive primary-buff (W).
                        systems::power_weapon::tick_overdrive,
                        // Expire an active Elemental Infusion (AB).
                        systems::weapons::tick_element_infusion,
                        systems::skills::tick_bulwark,
                        systems::skills::tick_repair,
                        systems::skills::tick_tractor,
                        // Drop-zone ability fields (AB): re-apply status in radius.
                        systems::abilities::tick_ability_fields,
                        // Passive regen (after 4 s no-damage) then overheal → tanks.
                        systems::damage::passive_regen,
                        // Reconcile MAX HP against equipped HP affixes (spec VI.5).
                        systems::items::apply_item_hp,
                        // Reconcile player elemental resist from resist affixes (E7).
                        systems::items::apply_item_resist,
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
