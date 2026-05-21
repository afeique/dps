//! Device input → `Intent` (runs in `Update`, every frame).
//!
//! Twin-stick scheme: **WASD / arrows** (or a gamepad left stick) set a
//! screen-space MOVE direction; the **mouse** aims (`update_aim` → the ship
//! faces the cursor and fires toward it). Keyboard + gamepad are OR-combined.
//! Keeping input isolated here means the sim only ever reads `Intent`.

use crate::components::Intent;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// Below this magnitude the analog stick is treated as centered (guards against
/// resting drift on top of Bevy's built-in deadzone).
const STICK_DEADZONE: f32 = 0.12;

pub fn gather_input(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut q: Query<&mut Intent>,
) {
    let mut move_dir = Vec2::ZERO;
    let mut firing = false;

    // ── Keyboard: WASD / arrows → screen-space movement (W = up, D = right) ──
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        move_dir.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
        move_dir.y -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        move_dir.x += 1.0;
    }
    if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
        move_dir.x -= 1.0;
    }
    if keys.pressed(KeyCode::Space) {
        firing = true;
    }

    // ── Gamepad (first connected): left stick moves, South / RT fires ───────
    if let Some(gamepad) = gamepads.iter().next() {
        let stick = gamepad.left_stick();
        if stick.length() > STICK_DEADZONE {
            move_dir += stick; // x = right, y = up
        }
        if gamepad.pressed(GamepadButton::South) || gamepad.pressed(GamepadButton::RightTrigger2) {
            firing = true;
        }
    }

    // Clamp to unit length so diagonals aren't faster than cardinals.
    let move_dir = move_dir.clamp_length_max(1.0);

    for mut intent in &mut q {
        intent.move_dir = move_dir;
        intent.firing = firing;
    }
}

/// Mouse aiming: project the cursor into world space and write it to `Intent`,
/// flagging `aim_active`. `ship_control` then faces the ship at the cursor and
/// the player fires along that facing. Cursor outside the window (or no camera)
/// → `aim_active = false`. Runs after `gather_input`.
pub fn update_aim(
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mut q: Query<&mut Intent>,
) {
    let world = windows
        .single()
        .ok()
        .and_then(|w| w.cursor_position())
        .zip(cameras.single().ok())
        .and_then(|(cursor, (cam, cam_tf))| cam.viewport_to_world_2d(cam_tf, cursor).ok());

    for mut intent in &mut q {
        match world {
            Some(p) => {
                intent.aim = p;
                intent.aim_active = true;
            }
            None => intent.aim_active = false,
        }
    }
}
