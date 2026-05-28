//! Simulation + input systems.
//!
//! Convention: everything here except `input` runs in `FixedUpdate` and is the
//! *simulation*. Simulation systems must not touch rendering. `input` runs in
//! `Update` (read devices every frame) and only writes the `Intent` component.

pub mod abilities;
pub mod armory;
pub mod asteroids;
pub mod build_screen;
pub mod cleanup;
pub mod collision;
pub mod cores;
pub mod damage;
pub mod drops;
pub mod enemy;
pub mod enemy_ai;
pub mod flow;
pub mod formations;
pub mod hazard;
pub mod hitstop;
pub mod input;
pub mod items;
pub mod loadout_screen;
pub mod missions;
pub mod movement;
pub mod passives;
pub mod player_status;
pub mod power_weapon;
pub mod powerups;
pub mod reactions;
pub mod shop;
pub mod skills;
pub mod sp_alloc;
pub mod spawn;
pub mod status;
pub mod survivor;
pub mod wave;
pub mod weapons;
