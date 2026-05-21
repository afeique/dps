//! Dark Prism Solid (`dps`) — native Rust + Bevy port of Rainboids (solo).
//!
//! Phase 1 scaffold: the ECS architecture + a playable vertical slice
//! (fly a ship, shoot, kill a drifter, take damage). See
//! `docs/port-plan.md` for the staged plan and the OOP-JS → ECS mapping.
//!
//! The module layout mirrors the plan:
//!   states / resources / messages   — global flow, shared data, game events
//!   components/                      — ECS components (player, enemy, projectile, common)
//!   systems/                         — simulation systems (FixedUpdate) + input (Update)
//!   render/                          — HDR camera + bloom, glow helpers
//!   app.rs                           — GamePlugin: wires it all together

// Scaffold stage: several types are forward-declared for phases 3–5 (extra
// enemy kinds, fire cooldowns, score economy) and not yet read everywhere.
#![allow(dead_code)]

mod app;
mod components;
mod messages;
mod render;
mod resources;
mod states;
mod systems;

#[cfg(test)]
mod gate_tests;
#[cfg(test)]
mod wave_tests;

use app::GamePlugin;
use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Dark Prism Solid".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(GamePlugin)
        .add_plugins(render::screenshot::ScreenshotPlugin)
        .run();
}
