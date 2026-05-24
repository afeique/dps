//! Simulation + input systems.
//!
//! Convention: everything here except `input` runs in `FixedUpdate` and is the
//! *simulation*. Simulation systems must not touch rendering. `input` runs in
//! `Update` (read devices every frame) and only writes the `Intent` component.

pub mod asteroids;
pub mod cleanup;
pub mod collision;
pub mod damage;
pub mod drops;
pub mod enemy;
pub mod enemy_ai;
pub mod flow;
pub mod input;
pub mod items;
pub mod missions;
pub mod movement;
pub mod passives;
pub mod power_weapon;
pub mod powerups;
pub mod shop;
pub mod skills;
pub mod spawn;
pub mod status;
pub mod survivor;
pub mod wave;
pub mod weapons;
