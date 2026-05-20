//! Device input → `Intent` (runs in `Update`, every frame).
//!
//! Mirrors `js/modules/ui/input-handler.js`. Phase 1 is keyboard only;
//! mouse-aim and gamepad (`gilrs`) arrive in Phase 4. Keeping input isolated
//! here means the sim only ever reads `Intent`, never raw devices.

use crate::components::Intent;
use bevy::prelude::*;

pub fn gather_input(keys: Res<ButtonInput<KeyCode>>, mut q: Query<&mut Intent>) {
    let mut thrust = 0.0;
    let mut strafe = 0.0;

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
    let firing = keys.pressed(KeyCode::Space);

    for mut intent in &mut q {
        intent.thrust = thrust;
        intent.strafe = strafe;
        intent.firing = firing;
    }
}
