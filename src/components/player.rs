//! Player-ship components. Ported from `js/modules/player/*` and
//! `js/modules/combat/weapon-data.js`.

use bevy::prelude::*;

/// The player ship: movement tuning + marker. One per run (for now).
#[derive(Component, Debug)]
pub struct Ship {
    /// Forward acceleration (world units / sec²).
    pub thrust: f32,
    /// Hard speed cap (world units / sec).
    pub max_speed: f32,
    /// Rotation rate from strafe input (rad / sec).
    pub turn_rate: f32,
}

impl Default for Ship {
    fn default() -> Self {
        Self {
            thrust: 1100.0,
            max_speed: 520.0,
            turn_rate: 4.5,
        }
    }
}

/// Per-frame player intent written by `systems::input` and consumed by the
/// sim. Decouples device handling from gameplay (mirrors the role of
/// `js/modules/ui/input-handler.js`). Phase 4 adds mouse-aim + gamepad.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct Intent {
    /// Forward/back throttle, -1..=1.
    pub thrust: f32,
    /// Left/right (rotation) input, -1..=1.
    pub strafe: f32,
    /// World-space aim point (mouse) — unused until Phase 4.
    pub aim: Vec2,
    /// Primary fire held.
    pub firing: bool,
}

/// Primary weapon state. The 5 primary + 5 power weapons (with multishot,
/// homing, etc.) are ported in Phase 3 from `combat/weapon-data.js`.
#[derive(Component, Debug)]
pub struct Weapon {
    /// Seconds between shots.
    pub cooldown: f32,
    /// Counts down to 0; ready to fire at 0.
    pub timer: f32,
    pub bullet_speed: f32,
    pub damage: f32,
}

impl Default for Weapon {
    fn default() -> Self {
        Self {
            cooldown: 0.12,
            timer: 0.0,
            bullet_speed: 950.0,
            damage: 10.0,
        }
    }
}
