//! Device input → `Intent` (runs in `Update`, every frame).
//!
//! Mirrors `js/modules/ui/input-handler.js`. Keyboard **and** gamepad both feed
//! the same `Intent`, OR-combined, so either device drives the ship. Keeping
//! input isolated here means the sim only ever reads `Intent`, never raw
//! devices. (Mouse-aim / twin-stick aim is a later Phase-4 increment.)

use crate::components::Intent;
use bevy::prelude::*;

/// Below this magnitude the analog stick is treated as centered (guards against
/// resting drift on top of Bevy's built-in deadzone).
const STICK_DEADZONE: f32 = 0.12;

pub fn gather_input(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut q: Query<&mut Intent>,
) {
    let mut thrust = 0.0;
    let mut strafe = 0.0;
    let mut firing = false;

    // ── Keyboard (WASD / arrows + Space) ────────────────────────────────
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        thrust += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
        thrust -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        strafe += 1.0;
    }
    if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
        strafe -= 1.0;
    }
    if keys.pressed(KeyCode::Space) {
        firing = true;
    }

    // ── Gamepad (first connected): left stick steers, South / RT fires ──
    if let Some(gamepad) = gamepads.iter().next() {
        let stick = gamepad.left_stick();
        if stick.y.abs() > STICK_DEADZONE {
            thrust += stick.y; // up = forward
        }
        if stick.x.abs() > STICK_DEADZONE {
            strafe += stick.x; // right = strafe right
        }
        if gamepad.pressed(GamepadButton::South)
            || gamepad.pressed(GamepadButton::RightTrigger2)
        {
            firing = true;
        }
    }

    let thrust = thrust.clamp(-1.0, 1.0);
    let strafe = strafe.clamp(-1.0, 1.0);

    for mut intent in &mut q {
        intent.thrust = thrust;
        intent.strafe = strafe;
        intent.firing = firing;
    }
}
