//! Simulation + input systems.
//!
//! Convention: everything here except `input` runs in `FixedUpdate` and is the
//! *simulation*. Simulation systems must not touch rendering. `input` runs in
//! `Update` (read devices every frame) and only writes the `Intent` component.

pub mod cleanup;
pub mod collision;
pub mod damage;
pub mod enemy_ai;
pub mod enemy_fire;
pub mod flow;
pub mod input;
pub mod movement;
pub mod spawn;
pub mod weapons;
